//! virtio-input keyboard driver (third virtio-mmio slot) feeding the
//! `/dev/input/0` event stream (todo 12 M3).
//!
//! Probe-and-disable like the other virtio drivers: no
//! `-device virtio-keyboard-device` → the kernel runs without input.
//!
//! The device mirrors Linux evdev: each completion is exactly one
//! 8-byte `virtio_input_event { le16 type, le16 code, le32 value }`
//! (a key press arrives as EV_KEY[code,1] + EV_SYN, release as
//! EV_KEY[code,0] + EV_SYN). We deliver the raw event stream to
//! readers and let userspace filter — same contract as evdev.
//!
//! Only the eventq (queue 0) is configured; the statusq (queue 1, LED
//! feedback) is legitimately left unready — QEMU activates the device
//! on DRIVER_OK without any queue-ready checks, and never initiates
//! statusq traffic.

use core::ptr::{addr_of, addr_of_mut, read_volatile, write_volatile};
use core::sync::atomic::{fence, AtomicBool, Ordering};
use core::task::Waker;

use alloc::collections::VecDeque;

use hal::Hal;

use crate::arch::Arch;
use crate::sync::SpinLock;
use crate::wait::WakerList;

const BASE: usize = <Arch as Hal>::VIRTIO2;

const NUM: usize = 16; // eventq ring size (device max is 64)
const EVENT_LEN: usize = 8;
/// Cooked-queue cap: 256 events. Past that we drop new events (the
/// spec lets the device drop too; readers that lag lose input, not
/// memory).
const COOKED_CAP: usize = 256 * EVENT_LEN;

const VIRTIO_MAGIC: u32 = 0x7472_6976;
const VIRTIO_VERSION: u32 = 2;
const VIRTIO_INPUT_DEVICE_ID: u32 = 18;

const MMIO_MAGIC_VALUE: usize = 0x000;
const MMIO_VERSION: usize = 0x004;
const MMIO_DEVICE_ID: usize = 0x008;
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
const MMIO_DEVICE_FEATURES: usize = 0x010;

const STATUS_ACKNOWLEDGE: u32 = 1;
const STATUS_DRIVER: u32 = 2;
const STATUS_DRIVER_OK: u32 = 4;
const STATUS_FEATURES_OK: u32 = 8;

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
struct EventBufs([[u8; EVENT_LEN]; NUM]);

static mut EV_DESC: DescTable = DescTable([VirtqDesc::ZERO; NUM]);
static mut EV_AVAIL: AvailRing =
    AvailRing { flags: 0, idx: 0, ring: [0; NUM], used_event: 0 };
static mut EV_USED: UsedRing = UsedRing {
    flags: 0,
    idx: 0,
    ring: [VirtqUsedElem { id: 0, len: 0 }; NUM],
    avail_event: 0,
};
static mut EV_BUFS: EventBufs = EventBufs([[0; EVENT_LEN]; NUM]);

struct InputState {
    used_idx: u16,
    /// Raw 8-byte events ready for readers, in arrival order.
    cooked: VecDeque<u8>,
}

static STATE: SpinLock<InputState> = SpinLock::new(InputState {
    used_idx: 0,
    cooked: VecDeque::new(),
});

/// Multi-waiter: any number of procs may block reading /dev/input/0.
static READERS: WakerList = WakerList::new();

static PRESENT: AtomicBool = AtomicBool::new(false);

pub fn present() -> bool {
    PRESENT.load(Ordering::Acquire)
}

/// Probe + bring up the keyboard. Quietly does nothing if the slot is
/// empty or holds a different device.
pub fn init() {
    unsafe {
        if read_reg(MMIO_MAGIC_VALUE) != VIRTIO_MAGIC
            || read_reg(MMIO_VERSION) != VIRTIO_VERSION
            || read_reg(MMIO_DEVICE_ID) != VIRTIO_INPUT_DEVICE_ID
        {
            return;
        }

        let mut status: u32 = 0;
        write_reg(MMIO_STATUS, status);
        status |= STATUS_ACKNOWLEDGE;
        write_reg(MMIO_STATUS, status);
        status |= STATUS_DRIVER;
        write_reg(MMIO_STATUS, status);

        // No device-specific features; ack only VIRTIO_F_VERSION_1.
        write_reg(MMIO_DEVICE_FEATURES_SEL, 0);
        let _ = read_reg(MMIO_DEVICE_FEATURES);
        write_reg(MMIO_DRIVER_FEATURES_SEL, 0);
        write_reg(MMIO_DRIVER_FEATURES, 0);
        write_reg(MMIO_DEVICE_FEATURES_SEL, 1);
        let hi = read_reg(MMIO_DEVICE_FEATURES) & (1 << F_VERSION_1_BIT);
        write_reg(MMIO_DRIVER_FEATURES_SEL, 1);
        write_reg(MMIO_DRIVER_FEATURES, hi);
        status |= STATUS_FEATURES_OK;
        write_reg(MMIO_STATUS, status);
        if read_reg(MMIO_STATUS) & STATUS_FEATURES_OK == 0 {
            return;
        }

        // eventq (queue 0) only; statusq stays unready.
        write_reg(MMIO_QUEUE_SEL, 0);
        if (read_reg(MMIO_QUEUE_NUM_MAX) as usize) < NUM {
            return;
        }
        write_reg(MMIO_QUEUE_NUM, NUM as u32);
        let pa = addr_of!(EV_DESC) as u64;
        write_reg(MMIO_QUEUE_DESC_LOW, pa as u32);
        write_reg(MMIO_QUEUE_DESC_HIGH, (pa >> 32) as u32);
        let pa = addr_of!(EV_AVAIL) as u64;
        write_reg(MMIO_DRIVER_DESC_LOW, pa as u32);
        write_reg(MMIO_DRIVER_DESC_HIGH, (pa >> 32) as u32);
        let pa = addr_of!(EV_USED) as u64;
        write_reg(MMIO_DEVICE_DESC_LOW, pa as u32);
        write_reg(MMIO_DEVICE_DESC_HIGH, (pa >> 32) as u32);
        write_reg(MMIO_QUEUE_READY, 1);

        status |= STATUS_DRIVER_OK;
        write_reg(MMIO_STATUS, status);

        // Pre-post every event buffer (device fills one event each).
        for i in 0..NUM {
            (*addr_of_mut!(EV_DESC)).0[i] = VirtqDesc {
                addr: addr_of!(EV_BUFS.0[i]) as u64,
                len: EVENT_LEN as u32,
                flags: DESC_F_WRITE,
                next: 0,
            };
            let avail = &mut *addr_of_mut!(EV_AVAIL);
            avail.ring[avail.idx as usize % NUM] = i as u16;
            fence(Ordering::SeqCst);
            avail.idx = avail.idx.wrapping_add(1);
        }
        fence(Ordering::SeqCst);
        write_reg(MMIO_QUEUE_NOTIFY, 0);
    }

    PRESENT.store(true, Ordering::Release);
    crate::println!("virtio_input: keyboard ready");
}

pub fn on_irq() {
    unsafe {
        let intr = read_reg(MMIO_INTERRUPT_STATUS) & 0x3;
        write_reg(MMIO_INTERRUPT_ACK, intr);
        fence(Ordering::SeqCst);
    }
    let mut wake = false;
    {
        let mut st = STATE.lock();
        let now = unsafe { (*addr_of!(EV_USED)).idx };
        while st.used_idx != now {
            let pos = st.used_idx as usize % NUM;
            let e = unsafe { (*addr_of!(EV_USED)).ring[pos] };
            let idx = e.id as usize % NUM;
            if e.len as usize >= EVENT_LEN
                && st.cooked.len() + EVENT_LEN <= COOKED_CAP
            {
                let buf = unsafe { &(*addr_of!(EV_BUFS)).0[idx] };
                st.cooked.extend(buf.iter().copied());
                wake = true;
            }
            // Repost the buffer.
            unsafe {
                let avail = &mut *addr_of_mut!(EV_AVAIL);
                avail.ring[avail.idx as usize % NUM] = idx as u16;
                fence(Ordering::SeqCst);
                avail.idx = avail.idx.wrapping_add(1);
            }
            st.used_idx = st.used_idx.wrapping_add(1);
        }
    }
    unsafe {
        fence(Ordering::SeqCst);
        write_reg(MMIO_QUEUE_NOTIFY, 0);
    }
    if wake {
        READERS.wake_all();
    }
}

/// Pop up to `buf.len()` cooked event bytes. Returns bytes copied
/// (0 = nothing pending). Reads are byte-stream; readers should use
/// multiples of 8 to stay event-aligned.
pub fn read_nonblock(buf: &mut [u8]) -> usize {
    let mut st = STATE.lock();
    let mut n = 0;
    while n < buf.len() {
        match st.cooked.pop_front() {
            Some(b) => {
                buf[n] = b;
                n += 1;
            }
            None => break,
        }
    }
    n
}

/// Unread cooked bytes (for POLLIN).
pub fn pending() -> usize {
    STATE.lock().cooked.len()
}

pub fn register_waker(w: &Waker) {
    READERS.register(w);
}
