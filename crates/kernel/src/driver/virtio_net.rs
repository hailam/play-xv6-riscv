//! virtio-net mmio driver (second virtio slot) + a smoltcp `Device`.
//!
//! Probe-and-disable: if qemu wasn't given `-device virtio-net-device`
//! the magic/ID check fails and the kernel runs loopback-only — every
//! TCP test that doesn't need a real NIC still works.
//!
//! Modern (mmio v2) device → every frame is prefixed by the 12-byte
//! `virtio_net_hdr` (we negotiate no offloads, so it's all zeros on
//! TX and skipped on RX).
//!
//! RX: 8 pre-posted 2 KiB buffers; the IRQ handler moves completed
//! ones onto a ready queue and kicks the net task. TX: 8 buffers with
//! a free bitmap; exhaustion is backpressure (smoltcp retries on the
//! next poll).

use core::ptr::{addr_of, addr_of_mut, read_volatile, write_volatile};
use core::sync::atomic::{fence, AtomicBool, Ordering};

use alloc::collections::VecDeque;

use hal::Hal;

use crate::arch::Arch;
use crate::sync::SpinLock;

const BASE: usize = <Arch as Hal>::VIRTIO1;

const NUM: usize = 8;
const BUF_LEN: usize = 2048;
const NET_HDR_LEN: usize = 12;

const VIRTIO_MAGIC: u32 = 0x7472_6976;
const VIRTIO_VERSION: u32 = 2;
const VIRTIO_NET_DEVICE_ID: u32 = 1;

const MMIO_MAGIC_VALUE: usize = 0x000;
const MMIO_VERSION: usize = 0x004;
const MMIO_DEVICE_ID: usize = 0x008;
const MMIO_DEVICE_FEATURES: usize = 0x010;
const MMIO_DEVICE_FEATURES_SEL: usize = 0x014;
const MMIO_DRIVER_FEATURES: usize = 0x020;
const MMIO_DRIVER_FEATURES_SEL: usize = 0x024;
const MMIO_QUEUE_SEL: usize = 0x030;
const MMIO_QUEUE_NUM_MAX: usize = 0x034;
const MMIO_QUEUE_NUM: usize = 0x038;
const MMIO_QUEUE_READY: usize = 0x044;
const MMIO_QUEUE_NOTIFY: usize = 0x050;
const MMIO_INTERRUPT_STATUS: usize = 0x060;
const MMIO_INTERRUPT_ACK: usize = 0x064;
const MMIO_STATUS: usize = 0x070;
const MMIO_QUEUE_DESC_LOW: usize = 0x080;
const MMIO_QUEUE_DESC_HIGH: usize = 0x084;
const MMIO_DRIVER_DESC_LOW: usize = 0x090;
const MMIO_DRIVER_DESC_HIGH: usize = 0x094;
const MMIO_DEVICE_DESC_LOW: usize = 0x0a0;
const MMIO_DEVICE_DESC_HIGH: usize = 0x0a4;
const MMIO_CONFIG: usize = 0x100;

const STATUS_ACKNOWLEDGE: u32 = 1;
const STATUS_DRIVER: u32 = 2;
const STATUS_DRIVER_OK: u32 = 4;
const STATUS_FEATURES_OK: u32 = 8;

const F_NET_MAC: u32 = 5;
/// VIRTIO_F_VERSION_1 = bit 32 (bit 0 of feature word 1). A v2 mmio
/// device is *modern* and the driver MUST negotiate this, or the
/// virtqueue layout / 12-byte net header it expects won't match what
/// we drive — frames arrive but parse as garbage.
const F_VERSION_1_BIT: u32 = 0; // within feature word 1

const DESC_F_WRITE: u16 = 2;

#[inline]
unsafe fn read_reg(off: usize) -> u32 {
    read_volatile((BASE + off) as *const u32)
}

#[inline]
unsafe fn write_reg(off: usize, val: u32) {
    write_volatile((BASE + off) as *mut u32, val);
}

#[repr(C)]
#[derive(Clone, Copy)]
struct VirtqDesc {
    addr: u64,
    len: u32,
    flags: u16,
    next: u16,
}
impl VirtqDesc {
    const ZERO: Self = Self { addr: 0, len: 0, flags: 0, next: 0 };
}

#[repr(C, align(4096))]
struct DescTable([VirtqDesc; NUM]);

#[repr(C, align(4096))]
struct AvailRing {
    flags: u16,
    idx: u16,
    ring: [u16; NUM],
    used_event: u16,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct VirtqUsedElem {
    id: u32,
    len: u32,
}

#[repr(C, align(4096))]
struct UsedRing {
    flags: u16,
    idx: u16,
    ring: [VirtqUsedElem; NUM],
    avail_event: u16,
}

#[repr(C, align(16))]
struct Bufs([[u8; BUF_LEN]; NUM]);

// RX queue (0).
static mut RX_DESC: DescTable = DescTable([VirtqDesc::ZERO; NUM]);
static mut RX_AVAIL: AvailRing =
    AvailRing { flags: 0, idx: 0, ring: [0; NUM], used_event: 0 };
static mut RX_USED: UsedRing = UsedRing {
    flags: 0,
    idx: 0,
    ring: [VirtqUsedElem { id: 0, len: 0 }; NUM],
    avail_event: 0,
};
static mut RX_BUFS: Bufs = Bufs([[0; BUF_LEN]; NUM]);

// TX queue (1).
static mut TX_DESC: DescTable = DescTable([VirtqDesc::ZERO; NUM]);
static mut TX_AVAIL: AvailRing =
    AvailRing { flags: 0, idx: 0, ring: [0; NUM], used_event: 0 };
static mut TX_USED: UsedRing = UsedRing {
    flags: 0,
    idx: 0,
    ring: [VirtqUsedElem { id: 0, len: 0 }; NUM],
    avail_event: 0,
};
static mut TX_BUFS: Bufs = Bufs([[0; BUF_LEN]; NUM]);

struct NetState {
    rx_used_idx: u16,
    tx_used_idx: u16,
    tx_free: u8, // bitmap, bit i = TX buffer i free
    /// RX completions: (buffer index, total length incl. net hdr).
    rx_ready: VecDeque<(u16, u32)>,
}

static STATE: SpinLock<NetState> = SpinLock::new(NetState {
    rx_used_idx: 0,
    tx_used_idx: 0,
    tx_free: 0xff,
    rx_ready: VecDeque::new(),
});

static PRESENT: AtomicBool = AtomicBool::new(false);
static mut MAC: [u8; 6] = [0; 6];

pub fn present() -> bool {
    PRESENT.load(Ordering::Acquire)
}

pub fn mac() -> [u8; 6] {
    unsafe { MAC }
}

/// Probe + bring up the NIC. Quietly does nothing if the slot is
/// empty or holds a different device.
pub fn init() {
    unsafe {
        if read_reg(MMIO_MAGIC_VALUE) != VIRTIO_MAGIC
            || read_reg(MMIO_VERSION) != VIRTIO_VERSION
            || read_reg(MMIO_DEVICE_ID) != VIRTIO_NET_DEVICE_ID
        {
            return;
        }

        let mut status: u32 = 0;
        write_reg(MMIO_STATUS, status);
        status |= STATUS_ACKNOWLEDGE;
        write_reg(MMIO_STATUS, status);
        status |= STATUS_DRIVER;
        write_reg(MMIO_STATUS, status);

        // Feature word 0: keep only MAC (no offloads, no mergeable RX).
        write_reg(MMIO_DEVICE_FEATURES_SEL, 0);
        let lo = read_reg(MMIO_DEVICE_FEATURES) & (1 << F_NET_MAC);
        write_reg(MMIO_DRIVER_FEATURES_SEL, 0);
        write_reg(MMIO_DRIVER_FEATURES, lo);
        // Feature word 1: MUST ack VIRTIO_F_VERSION_1 for a modern
        // (v2 mmio) device.
        write_reg(MMIO_DEVICE_FEATURES_SEL, 1);
        let hi = read_reg(MMIO_DEVICE_FEATURES) & (1 << F_VERSION_1_BIT);
        write_reg(MMIO_DRIVER_FEATURES_SEL, 1);
        write_reg(MMIO_DRIVER_FEATURES, hi);
        status |= STATUS_FEATURES_OK;
        write_reg(MMIO_STATUS, status);
        if read_reg(MMIO_STATUS) & STATUS_FEATURES_OK == 0 {
            return;
        }

        // RX queue 0.
        write_reg(MMIO_QUEUE_SEL, 0);
        if (read_reg(MMIO_QUEUE_NUM_MAX) as usize) < NUM {
            return;
        }
        write_reg(MMIO_QUEUE_NUM, NUM as u32);
        let pa = addr_of!(RX_DESC) as u64;
        write_reg(MMIO_QUEUE_DESC_LOW, pa as u32);
        write_reg(MMIO_QUEUE_DESC_HIGH, (pa >> 32) as u32);
        let pa = addr_of!(RX_AVAIL) as u64;
        write_reg(MMIO_DRIVER_DESC_LOW, pa as u32);
        write_reg(MMIO_DRIVER_DESC_HIGH, (pa >> 32) as u32);
        let pa = addr_of!(RX_USED) as u64;
        write_reg(MMIO_DEVICE_DESC_LOW, pa as u32);
        write_reg(MMIO_DEVICE_DESC_HIGH, (pa >> 32) as u32);
        write_reg(MMIO_QUEUE_READY, 1);

        // TX queue 1.
        write_reg(MMIO_QUEUE_SEL, 1);
        if (read_reg(MMIO_QUEUE_NUM_MAX) as usize) < NUM {
            return;
        }
        write_reg(MMIO_QUEUE_NUM, NUM as u32);
        let pa = addr_of!(TX_DESC) as u64;
        write_reg(MMIO_QUEUE_DESC_LOW, pa as u32);
        write_reg(MMIO_QUEUE_DESC_HIGH, (pa >> 32) as u32);
        let pa = addr_of!(TX_AVAIL) as u64;
        write_reg(MMIO_DRIVER_DESC_LOW, pa as u32);
        write_reg(MMIO_DRIVER_DESC_HIGH, (pa >> 32) as u32);
        let pa = addr_of!(TX_USED) as u64;
        write_reg(MMIO_DEVICE_DESC_LOW, pa as u32);
        write_reg(MMIO_DEVICE_DESC_HIGH, (pa >> 32) as u32);
        write_reg(MMIO_QUEUE_READY, 1);

        status |= STATUS_DRIVER_OK;
        write_reg(MMIO_STATUS, status);

        for i in 0..6 {
            MAC[i] = read_volatile((BASE + MMIO_CONFIG + i) as *const u8);
        }

        // Pre-post every RX buffer.
        for i in 0..NUM {
            (*addr_of_mut!(RX_DESC)).0[i] = VirtqDesc {
                addr: addr_of!(RX_BUFS.0[i]) as u64,
                len: BUF_LEN as u32,
                flags: DESC_F_WRITE,
                next: 0,
            };
            let avail = &mut *addr_of_mut!(RX_AVAIL);
            avail.ring[avail.idx as usize % NUM] = i as u16;
            fence(Ordering::SeqCst);
            avail.idx = avail.idx.wrapping_add(1);
        }
        fence(Ordering::SeqCst);
        write_reg(MMIO_QUEUE_NOTIFY, 0);
    }

    PRESENT.store(true, Ordering::Release);
    crate::println!(
        "virtio_net: ready (mac={:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x})",
        unsafe { MAC[0] },
        unsafe { MAC[1] },
        unsafe { MAC[2] },
        unsafe { MAC[3] },
        unsafe { MAC[4] },
        unsafe { MAC[5] },
    );
}

pub fn on_irq() {
    unsafe {
        let intr = read_reg(MMIO_INTERRUPT_STATUS) & 0x3;
        write_reg(MMIO_INTERRUPT_ACK, intr);
        fence(Ordering::SeqCst);
    }
    let mut st = STATE.lock();
    // RX completions → ready queue.
    let rx_now = unsafe { (*addr_of!(RX_USED)).idx };
    while st.rx_used_idx != rx_now {
        let pos = st.rx_used_idx as usize % NUM;
        let e = unsafe { (*addr_of!(RX_USED)).ring[pos] };
        st.rx_ready.push_back((e.id as u16, e.len));
        st.rx_used_idx = st.rx_used_idx.wrapping_add(1);
    }
    // TX completions → free the buffers.
    let tx_now = unsafe { (*addr_of!(TX_USED)).idx };
    while st.tx_used_idx != tx_now {
        let pos = st.tx_used_idx as usize % NUM;
        let e = unsafe { (*addr_of!(TX_USED)).ring[pos] };
        st.tx_free |= 1 << (e.id as u8);
        st.tx_used_idx = st.tx_used_idx.wrapping_add(1);
    }
    drop(st);
    crate::net::kick();
}

/// Repost a consumed RX buffer to the device.
fn repost_rx(idx: u16) {
    unsafe {
        let avail = &mut *addr_of_mut!(RX_AVAIL);
        avail.ring[avail.idx as usize % NUM] = idx;
        fence(Ordering::SeqCst);
        avail.idx = avail.idx.wrapping_add(1);
        fence(Ordering::SeqCst);
        write_reg(MMIO_QUEUE_NOTIFY, 0);
    }
}

// ---------- smoltcp Device ------------------------------------------------

use smoltcp::phy::{Checksum, Device, DeviceCapabilities, Medium, RxToken, TxToken};
use smoltcp::time::Instant;

pub struct NetDev;

pub struct NetRxToken {
    idx: u16,
    len: u32,
}

pub struct NetTxToken;

impl Device for NetDev {
    type RxToken<'a> = NetRxToken;
    type TxToken<'a> = NetTxToken;

    fn receive(
        &mut self,
        _ts: Instant,
    ) -> Option<(Self::RxToken<'_>, Self::TxToken<'_>)> {
        let mut st = STATE.lock();
        let (idx, len) = st.rx_ready.pop_front()?;
        if st.tx_free == 0 {
            // smoltcp may need to reply (e.g. ACK) — without a TX
            // slot, put the frame back for the next poll.
            st.rx_ready.push_front((idx, len));
            return None;
        }
        Some((NetRxToken { idx, len }, NetTxToken))
    }

    fn transmit(&mut self, _ts: Instant) -> Option<Self::TxToken<'_>> {
        if STATE.lock().tx_free == 0 {
            return None;
        }
        Some(NetTxToken)
    }

    fn capabilities(&self) -> DeviceCapabilities {
        let mut caps = DeviceCapabilities::default();
        caps.medium = Medium::Ethernet;
        caps.max_transmission_unit = 1514;
        caps.checksum = smoltcp::phy::ChecksumCapabilities::default();
        let _ = Checksum::Both; // (defaults: compute/verify in software)
        caps
    }
}

impl RxToken for NetRxToken {
    fn consume<R, F>(self, f: F) -> R
    where
        F: FnOnce(&[u8]) -> R,
    {
        let total = self.len as usize;
        let payload_end = total.min(BUF_LEN);
        let buf = unsafe { &(*addr_of!(RX_BUFS)).0[self.idx as usize] };
        let r = f(&buf[NET_HDR_LEN..payload_end]);
        repost_rx(self.idx);
        r
    }
}

impl TxToken for NetTxToken {
    fn consume<R, F>(self, len: usize, f: F) -> R
    where
        F: FnOnce(&mut [u8]) -> R,
    {
        // Claim a TX buffer (transmit()/receive() checked tx_free,
        // but claim again under the lock to be airtight).
        let idx = loop {
            let mut st = STATE.lock();
            if st.tx_free != 0 {
                let i = st.tx_free.trailing_zeros() as usize;
                st.tx_free &= !(1 << i);
                break i;
            }
            drop(st);
            core::hint::spin_loop();
        };
        let r = unsafe {
            let buf = &mut (*addr_of_mut!(TX_BUFS)).0[idx];
            buf[..NET_HDR_LEN].fill(0); // no offloads: zero header
            let r = f(&mut buf[NET_HDR_LEN..NET_HDR_LEN + len]);
            (*addr_of_mut!(TX_DESC)).0[idx] = VirtqDesc {
                addr: addr_of!(TX_BUFS.0[idx]) as u64,
                len: (NET_HDR_LEN + len) as u32,
                flags: 0,
                next: 0,
            };
            let avail = &mut *addr_of_mut!(TX_AVAIL);
            avail.ring[avail.idx as usize % NUM] = idx as u16;
            fence(Ordering::SeqCst);
            avail.idx = avail.idx.wrapping_add(1);
            fence(Ordering::SeqCst);
            write_reg(MMIO_QUEUE_NOTIFY, 1);
            r
        };
        r
    }
}
