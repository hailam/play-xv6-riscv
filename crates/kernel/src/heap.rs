//! Minimal bump allocator backed by a static 1 MiB buffer. Sufficient
//! for Phase 4: we allocate `Proc` and (later) `Pin<Box<dyn Future>>`
//! once-ish and never `dealloc`. A real allocator (linked-list / slab)
//! will replace this when reclaim matters.

use core::alloc::{GlobalAlloc, Layout};
use core::ptr::null_mut;
use core::sync::atomic::{AtomicUsize, Ordering};

const HEAP_SIZE: usize = 1024 * 1024;

#[repr(align(16))]
struct HeapBuf([u8; HEAP_SIZE]);

static mut HEAP_BUF: HeapBuf = HeapBuf([0; HEAP_SIZE]);

struct BumpAlloc {
    next: AtomicUsize,
}

unsafe impl GlobalAlloc for BumpAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let align = layout.align().max(1);
        let size = layout.size();
        let base = core::ptr::addr_of_mut!(HEAP_BUF) as *mut u8 as usize;
        loop {
            let cur = self.next.load(Ordering::Relaxed);
            let start = (base + cur + align - 1) & !(align - 1);
            let new_cur = match (start - base).checked_add(size) {
                Some(v) => v,
                None => return null_mut(),
            };
            if new_cur > HEAP_SIZE {
                return null_mut();
            }
            if self
                .next
                .compare_exchange_weak(cur, new_cur, Ordering::AcqRel, Ordering::Relaxed)
                .is_ok()
            {
                return start as *mut u8;
            }
        }
    }

    unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {
        // bump allocator never frees
    }
}

#[global_allocator]
static ALLOC: BumpAlloc = BumpAlloc {
    next: AtomicUsize::new(0),
};
