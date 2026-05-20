//! virtio-mmio block device driver for QEMU virt.
//!
//! Phase 6a: a synchronous read/write API plus the IRQ glue that wakes
//! the busy-waiter when the device puts the request on the used ring.
//! Phase 6b will wrap this in async wakers.
//!
//! Reference: xv6-riscv `kernel/virtio_disk.c`, plus the virtio 1.x
//! "MMIO Device" spec.

use core::future::Future;
use core::pin::Pin;
use core::ptr::{addr_of, addr_of_mut, read_volatile, write_volatile};
use core::sync::atomic::{fence, AtomicBool, AtomicUsize, Ordering};
use core::task::{Context, Poll};

use hal::Hal;

use crate::arch::Arch;
use crate::sync::SpinLock;
use crate::wait::WakerCell;

#[cfg(target_arch = "riscv64")]
use crate::arch::VIRTIO0;

pub const SECTOR_SIZE: usize = 512;
pub const NUM: usize = 8;

const VIRTIO_MAGIC: u32 = 0x74726976;
const VIRTIO_VERSION: u32 = 2;
const VIRTIO_BLK_DEVICE_ID: u32 = 2;

const MMIO_MAGIC_VALUE: usize = 0x000;
const MMIO_VERSION: usize = 0x004;
const MMIO_DEVICE_ID: usize = 0x008;
const MMIO_DEVICE_FEATURES: usize = 0x010;
const MMIO_DRIVER_FEATURES: usize = 0x020;
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

const STATUS_ACKNOWLEDGE: u32 = 1;
const STATUS_DRIVER: u32 = 2;
const STATUS_DRIVER_OK: u32 = 4;
const STATUS_FEATURES_OK: u32 = 8;

const F_BLK_RO: u32 = 5;
const F_BLK_SCSI: u32 = 7;
const F_BLK_CONFIG_WCE: u32 = 11;
const F_BLK_MQ: u32 = 12;
const F_ANY_LAYOUT: u32 = 27;
const F_RING_INDIRECT: u32 = 28;
const F_RING_EVENT_IDX: u32 = 29;

const VIRTIO_BLK_T_IN: u32 = 0;
const VIRTIO_BLK_T_OUT: u32 = 1;

const DESC_F_NEXT: u16 = 1;
const DESC_F_WRITE: u16 = 2;

#[inline]
unsafe fn reg(off: usize) -> *mut u32 {
    (VIRTIO0 + off) as *mut u32
}

#[inline]
unsafe fn read_reg(off: usize) -> u32 {
    read_volatile(reg(off))
}

#[inline]
unsafe fn write_reg(off: usize, val: u32) {
    write_volatile(reg(off), val);
}

// ---------- ring structures ------------------------------------------------

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
struct DescTable {
    descs: [VirtqDesc; NUM],
}

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
impl VirtqUsedElem {
    const ZERO: Self = Self { id: 0, len: 0 };
}

#[repr(C, align(4096))]
struct UsedRing {
    flags: u16,
    idx: u16,
    ring: [VirtqUsedElem; NUM],
    avail_event: u16,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct VirtioBlkReq {
    typ: u32,
    reserved: u32,
    sector: u64,
}
impl VirtioBlkReq {
    const ZERO: Self = Self { typ: 0, reserved: 0, sector: 0 };
}

#[repr(C, align(16))]
struct OpsTable {
    reqs: [VirtioBlkReq; NUM],
}

#[repr(C, align(8))]
struct StatusTable {
    bytes: [u8; NUM],
}

// All ring memory lives in static storage so we don't depend on the
// frame allocator during boot. Each table is page-aligned (`#[repr(C,
// align(4096))]`) so virtio sees clean physical addresses.
static mut DESC: DescTable = DescTable { descs: [VirtqDesc::ZERO; NUM] };
static mut AVAIL: AvailRing = AvailRing {
    flags: 0,
    idx: 0,
    ring: [0; NUM],
    used_event: 0,
};
static mut USED: UsedRing = UsedRing {
    flags: 0,
    idx: 0,
    ring: [VirtqUsedElem::ZERO; NUM],
    avail_event: 0,
};
static mut OPS: OpsTable = OpsTable { reqs: [VirtioBlkReq::ZERO; NUM] };
static mut STATUS_BYTES: StatusTable = StatusTable { bytes: [0; NUM] };

struct DiskState {
    free: [bool; NUM],
    used_idx: u16,
}

static DISK: SpinLock<DiskState> = SpinLock::new(DiskState {
    free: [true; NUM],
    used_idx: 0,
});

static COMPLETED: [AtomicBool; NUM] = [const { AtomicBool::new(false) }; NUM];
static WAKERS: [WakerCell; NUM] = [const { WakerCell::new() }; NUM];

/// Count of submitted I/O ops since boot — used by bio's smoke test
/// to observe cache hits (no new I/O issued).
pub static IO_COUNT: AtomicUsize = AtomicUsize::new(0);

#[derive(Debug)]
pub enum DiskError {
    NoFreeDescriptor,
    // Status payload is for `Debug`-printing on panic; it intentionally
    // isn't read by code so silence the dead-code warning.
    #[allow(dead_code)]
    Status(u8),
}

// ---------- init -----------------------------------------------------------

pub fn init() {
    // Probe.
    unsafe {
        let magic = read_reg(MMIO_MAGIC_VALUE);
        assert_eq!(magic, VIRTIO_MAGIC, "virtio: bad magic {magic:#x}");
        let version = read_reg(MMIO_VERSION);
        assert_eq!(version, VIRTIO_VERSION, "virtio: bad version {version}");
        let device = read_reg(MMIO_DEVICE_ID);
        assert_eq!(device, VIRTIO_BLK_DEVICE_ID, "virtio: not a block device");
    }

    let mut status: u32 = 0;
    unsafe {
        // Reset.
        write_reg(MMIO_STATUS, status);

        status |= STATUS_ACKNOWLEDGE;
        write_reg(MMIO_STATUS, status);

        status |= STATUS_DRIVER;
        write_reg(MMIO_STATUS, status);

        // Disable features we don't implement.
        let mut features = read_reg(MMIO_DEVICE_FEATURES);
        features &= !(1u32 << F_BLK_RO);
        features &= !(1u32 << F_BLK_SCSI);
        features &= !(1u32 << F_BLK_CONFIG_WCE);
        features &= !(1u32 << F_BLK_MQ);
        features &= !(1u32 << F_ANY_LAYOUT);
        features &= !(1u32 << F_RING_EVENT_IDX);
        features &= !(1u32 << F_RING_INDIRECT);
        write_reg(MMIO_DRIVER_FEATURES, features);

        status |= STATUS_FEATURES_OK;
        write_reg(MMIO_STATUS, status);

        let s = read_reg(MMIO_STATUS);
        assert!(s & STATUS_FEATURES_OK != 0, "virtio FEATURES_OK clear");

        // Queue 0 setup.
        write_reg(MMIO_QUEUE_SEL, 0);
        assert!(read_reg(MMIO_QUEUE_READY) == 0, "virtio queue already ready");
        let max = read_reg(MMIO_QUEUE_NUM_MAX);
        assert!(max as usize >= NUM, "virtio queue too small (max={max})");
        write_reg(MMIO_QUEUE_NUM, NUM as u32);

        let desc_pa = addr_of!(DESC) as u64;
        let avail_pa = addr_of!(AVAIL) as u64;
        let used_pa = addr_of!(USED) as u64;
        write_reg(MMIO_QUEUE_DESC_LOW, desc_pa as u32);
        write_reg(MMIO_QUEUE_DESC_HIGH, (desc_pa >> 32) as u32);
        write_reg(MMIO_DRIVER_DESC_LOW, avail_pa as u32);
        write_reg(MMIO_DRIVER_DESC_HIGH, (avail_pa >> 32) as u32);
        write_reg(MMIO_DEVICE_DESC_LOW, used_pa as u32);
        write_reg(MMIO_DEVICE_DESC_HIGH, (used_pa >> 32) as u32);

        write_reg(MMIO_QUEUE_READY, 1);

        status |= STATUS_DRIVER_OK;
        write_reg(MMIO_STATUS, status);
    }

    crate::println!(
        "virtio_blk: ready (desc@{:#x} avail@{:#x} used@{:#x})",
        addr_of!(DESC) as usize,
        addr_of!(AVAIL) as usize,
        addr_of!(USED) as usize,
    );
}

// ---------- descriptor allocator ------------------------------------------

fn alloc_three(state: &mut DiskState) -> Option<[usize; 3]> {
    let mut idx = [0usize; 3];
    for i in 0..3 {
        let mut found = None;
        for j in 0..NUM {
            if state.free[j] {
                state.free[j] = false;
                found = Some(j);
                break;
            }
        }
        match found {
            Some(j) => idx[i] = j,
            None => {
                for k in 0..i {
                    state.free[idx[k]] = true;
                }
                return None;
            }
        }
    }
    Some(idx)
}

fn free_chain(state: &mut DiskState, head: usize) {
    let mut i = head;
    loop {
        let (flags, next) = unsafe {
            let d = &(*addr_of!(DESC)).descs[i];
            (d.flags, d.next)
        };
        // Zero the descriptor.
        unsafe {
            (*addr_of_mut!(DESC)).descs[i] = VirtqDesc::ZERO;
        }
        state.free[i] = true;
        if flags & DESC_F_NEXT == 0 {
            break;
        }
        i = next as usize;
    }
}

// ---------- read / write --------------------------------------------------

// Kept as an early-boot diagnostic fallback (see `revisit/sync-virtio-fallback`).
// No live callers in the async path.
#[allow(dead_code)]
pub fn sync_read_block(sector: u64, buf: &mut [u8; SECTOR_SIZE]) -> Result<(), DiskError> {
    let head = submit_chain(sector, buf.as_mut_ptr(), false)?;
    // Spin-with-wfi until IRQ flips our completion flag.
    while !COMPLETED[head].load(Ordering::Acquire) {
        unsafe { Arch::wfi() };
    }
    finish(head)
}

#[allow(dead_code)]
pub fn sync_write_block(sector: u64, buf: &[u8; SECTOR_SIZE]) -> Result<(), DiskError> {
    let head = submit_chain(sector, buf.as_ptr() as *mut u8, true)?;
    while !COMPLETED[head].load(Ordering::Acquire) {
        unsafe { Arch::wfi() };
    }
    finish(head)
}

/// Async-friendly read. Parks the current task on a per-descriptor
/// `WakerCell`; `on_irq` wakes it.
///
/// `buf_addr` is taken as `usize` (not `*mut u8`) so the returned
/// `Future` is `Send` — needed for the executor's task-storage bound.
/// The caller computes `buf.as_mut_ptr() as usize`.
pub async fn read_block_async(
    sector: u64,
    buf_addr: usize,
) -> Result<(), DiskError> {
    let head = submit_chain(sector, buf_addr as *mut u8, false)?;
    BlockOp { head }.await;
    finish(head)
}

#[allow(dead_code)]
pub async fn write_block_async(
    sector: u64,
    buf_addr: usize,
) -> Result<(), DiskError> {
    let head = submit_chain(sector, buf_addr as *mut u8, true)?;
    BlockOp { head }.await;
    finish(head)
}

struct BlockOp {
    head: usize,
}

impl Future for BlockOp {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        // Register first to close the wake-loss race.
        WAKERS[self.head].register(cx.waker());
        if COMPLETED[self.head].load(Ordering::Acquire) {
            return Poll::Ready(());
        }
        Poll::Pending
    }
}

fn submit_chain(
    sector: u64,
    buf_ptr: *mut u8,
    write: bool,
) -> Result<usize, DiskError> {
    IO_COUNT.fetch_add(1, Ordering::Relaxed);
    let idx;
    {
        let mut state = DISK.lock();
        idx = alloc_three(&mut state).ok_or(DiskError::NoFreeDescriptor)?;

        let [i0, i1, i2] = idx;

        unsafe {
            (*addr_of_mut!(OPS)).reqs[i0] = VirtioBlkReq {
                typ: if write { VIRTIO_BLK_T_OUT } else { VIRTIO_BLK_T_IN },
                reserved: 0,
                sector,
            };
            let ops_pa = addr_of!((*addr_of!(OPS)).reqs[i0]) as u64;
            (*addr_of_mut!(DESC)).descs[i0] = VirtqDesc {
                addr: ops_pa,
                len: core::mem::size_of::<VirtioBlkReq>() as u32,
                flags: DESC_F_NEXT,
                next: i1 as u16,
            };
        }

        let data_flags = if write {
            DESC_F_NEXT
        } else {
            DESC_F_NEXT | DESC_F_WRITE
        };
        unsafe {
            (*addr_of_mut!(DESC)).descs[i1] = VirtqDesc {
                addr: buf_ptr as u64,
                len: SECTOR_SIZE as u32,
                flags: data_flags,
                next: i2 as u16,
            };
        }

        unsafe {
            (*addr_of_mut!(STATUS_BYTES)).bytes[i0] = 0xff;
            let status_pa = addr_of!((*addr_of!(STATUS_BYTES)).bytes[i0]) as u64;
            (*addr_of_mut!(DESC)).descs[i2] = VirtqDesc {
                addr: status_pa,
                len: 1,
                flags: DESC_F_WRITE,
                next: 0,
            };
        }

        COMPLETED[i0].store(false, Ordering::Release);

        unsafe {
            let avail = &mut *addr_of_mut!(AVAIL);
            let pos = avail.idx as usize % NUM;
            avail.ring[pos] = i0 as u16;
            fence(Ordering::SeqCst);
            avail.idx = avail.idx.wrapping_add(1);
        }
        fence(Ordering::SeqCst);
    }

    unsafe { write_reg(MMIO_QUEUE_NOTIFY, 0) };

    Ok(idx[0])
}

fn finish(head: usize) -> Result<(), DiskError> {
    let status = unsafe { (*addr_of!(STATUS_BYTES)).bytes[head] };
    {
        let mut state = DISK.lock();
        free_chain(&mut state, head);
    }
    if status == 0 {
        Ok(())
    } else {
        Err(DiskError::Status(status))
    }
}

// ---------- IRQ handler ----------------------------------------------------

/// Called from `kernel_on_external` when PLIC delivers VIRTIO0_IRQ.
/// Drains the used ring and flips `COMPLETED[id]` for each finished
/// request. The waker (busy-loop) sees the flag and returns.
pub fn on_irq() {
    unsafe {
        // ACK the device interrupt.
        let intr = read_reg(MMIO_INTERRUPT_STATUS) & 0x3;
        write_reg(MMIO_INTERRUPT_ACK, intr);
        fence(Ordering::SeqCst);
    }

    let mut state = DISK.lock();
    let used_now = unsafe { (*addr_of!(USED)).idx };
    while state.used_idx != used_now {
        let pos = state.used_idx as usize % NUM;
        let id = unsafe { (*addr_of!(USED)).ring[pos].id as usize };
        // Set completion *before* waking — readers register the
        // waker first, then check the flag, so the orderings line up.
        COMPLETED[id].store(true, Ordering::Release);
        WAKERS[id].wake();
        state.used_idx = state.used_idx.wrapping_add(1);
    }
}
