//! Physical 4 KiB frame allocator. Phase 2 implementation: single global
//! spinlock-protected freelist over [kernel_end, PHYSTOP). Per-CPU
//! magazine caches will be added in a later perf-tuning pass.
//!
//! Free frames are linked through their own first 8 bytes (a classic
//! intrusive freelist). The unsafe is concentrated in `Inner::push` and
//! `Inner::pop`; everything above operates on physical addresses as
//! `usize`.

use core::ptr::NonNull;
use core::sync::atomic::{AtomicUsize, Ordering};

use hal::FrameAllocator;

use crate::sync::SpinLock;

use crate::arch::{PGSIZE, PHYSTOP};

struct Run {
    next: Option<NonNull<Run>>,
}

struct Inner {
    head: Option<NonNull<Run>>,
}

unsafe impl Send for Inner {}

impl Inner {
    const fn new() -> Self {
        Self { head: None }
    }

    fn push(&mut self, pa: usize) {
        debug_assert!(pa % PGSIZE == 0);
        // Safety: caller guarantees `pa` is a valid, unaliased, 4 KiB-aligned
        // physical frame within the identity-mapped kernel region.
        unsafe {
            let p = pa as *mut Run;
            (*p).next = self.head;
            self.head = NonNull::new(p);
        }
    }

    fn pop(&mut self) -> Option<usize> {
        let head = self.head.take()?;
        // Safety: head was a frame we previously pushed.
        unsafe {
            self.head = (*head.as_ptr()).next;
        }
        Some(head.as_ptr() as usize)
    }
}

pub struct FrameAllocImpl {
    inner: SpinLock<Inner>,
    count: AtomicUsize,
}

impl FrameAllocImpl {
    pub const fn new() -> Self {
        Self {
            inner: SpinLock::new(Inner::new()),
            count: AtomicUsize::new(0),
        }
    }

    /// Add all 4 KiB-aligned frames in `[start, end)` to the freelist.
    /// `start` is rounded up to the next page; `end` is rounded down.
    pub fn add_range(&self, start: usize, end: usize) {
        let s = (start + PGSIZE - 1) & !(PGSIZE - 1);
        let e = end & !(PGSIZE - 1);
        let mut g = self.inner.lock();
        let mut pa = s;
        let mut n = 0usize;
        while pa + PGSIZE <= e {
            // Zero the frame before adding to the freelist; users get a
            // zeroed frame from `alloc_zeroed`.
            unsafe {
                core::ptr::write_bytes(pa as *mut u8, 0, PGSIZE);
            }
            g.push(pa);
            pa += PGSIZE;
            n += 1;
        }
        self.count.fetch_add(n, Ordering::Relaxed);
    }

    pub fn free_count(&self) -> usize {
        self.count.load(Ordering::Relaxed)
    }
}

impl FrameAllocator for FrameAllocImpl {
    fn alloc_zeroed(&self) -> Option<usize> {
        let pa = self.inner.lock().pop()?;
        self.count.fetch_sub(1, Ordering::Relaxed);
        // The frame was zeroed on insertion. Re-zero defensively in case
        // a future caller wrote to it before freeing without re-zeroing.
        unsafe { core::ptr::write_bytes(pa as *mut u8, 0, PGSIZE) };
        Some(pa)
    }

    unsafe fn free(&self, pa: usize) {
        self.inner.lock().push(pa);
        self.count.fetch_add(1, Ordering::Relaxed);
    }
}

pub static KFRAMES: FrameAllocImpl = FrameAllocImpl::new();

extern "C" {
    static _end: u8;
}

/// Initialize the global frame allocator with the free physical region
/// `[_end, PHYSTOP)`. Call once from hart 0.
pub fn init() {
    let kernel_end = unsafe { &_end as *const u8 as usize };
    KFRAMES.add_range(kernel_end, PHYSTOP);
}
