//! Kernel heap: first-fit, address-ordered free list with coalescing
//! over a static 16 MiB arena. Replaces the original bump allocator,
//! which never freed and exhausted under usertests-style fork churn
//! (`reparent2` alone forks 800 procs, each permanently leaking its
//! boxed `proc_main` future, `Proc`, and fd table).
//!
//! Layout: every allocation is preceded by a 16-byte header holding
//! the full block size (header + payload + padding, a multiple of
//! 16). Free blocks double as nodes of an address-ordered singly
//! linked list; adjacent free blocks merge on insert, so steady-state
//! fork/exit churn doesn't fragment the arena.
//!
//! Locking: a `SpinLock` (interrupts off while held) guards the list.
//! That makes alloc/dealloc safe from both task and IRQ context, on
//! one hart or several — and the irq-off discipline means a trap
//! handler can never deadlock against an allocation on its own hart.

use core::alloc::{GlobalAlloc, Layout};
use core::ptr::null_mut;

use crate::sync::SpinLock;

const HEAP_SIZE: usize = 16 * 1024 * 1024;
/// Every block size and payload address is a multiple of this; large
/// enough for every `Layout` the kernel actually allocates. Bigger
/// alignments are honoured by front-padding in `alloc`.
const UNIT: usize = 16;
/// Bytes between a block's start and its payload (holds the size).
const HDR: usize = 16;
/// Smallest fragment worth keeping on the free list — must hold a
/// `FreeNode`.
const MIN_FREE: usize = 32;

// The buffer is only accessed through the allocator; the inner field
// is intentionally unread.
#[repr(align(16))]
struct HeapBuf(#[allow(dead_code)] [u8; HEAP_SIZE]);

static mut HEAP_BUF: HeapBuf = HeapBuf([0; HEAP_SIZE]);

#[repr(C, align(16))]
struct FreeNode {
    size: usize,
    next: *mut FreeNode,
}

struct HeapState {
    head: *mut FreeNode,
    initialized: bool,
    used: usize,
    peak: usize,
}

// Raw pointers into the static arena; access is serialized by the lock.
unsafe impl Send for HeapState {}

static HEAP: SpinLock<HeapState> = SpinLock::new(HeapState {
    head: null_mut(),
    initialized: false,
    used: 0,
    peak: 0,
});

#[inline]
fn round_up(n: usize, a: usize) -> usize {
    (n + a - 1) & !(a - 1)
}

struct ListAlloc;

unsafe impl GlobalAlloc for ListAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let align = layout.align().max(UNIT);
        let psize = round_up(layout.size().max(1), UNIT);

        let mut h = HEAP.lock();
        if !h.initialized {
            let base = core::ptr::addr_of_mut!(HEAP_BUF) as usize;
            let node = base as *mut FreeNode;
            (*node).size = HEAP_SIZE;
            (*node).next = null_mut();
            h.head = node;
            h.initialized = true;
        }

        let mut prev: *mut FreeNode = null_mut();
        let mut cur = h.head;
        while !cur.is_null() {
            let b = cur as usize;
            let s = (*cur).size;

            // Payload placement; for align == UNIT this is b + HDR.
            let mut p = round_up(b + HDR, align);
            let mut hdr = p - HDR;
            let mut front = hdr - b;
            if front != 0 && front < MIN_FREE {
                // The front pad must survive as a free node — push the
                // payload up until it can.
                p = round_up(b + MIN_FREE + HDR, align);
                hdr = p - HDR;
                front = hdr - b;
            }
            let mut end = round_up(p + psize, UNIT);

            if end <= b + s {
                let tail = b + s - end;
                if tail < MIN_FREE {
                    // Absorb a too-small remainder into the block.
                    end = b + s;
                }
                // Splice replacement entries (front pad and/or tail)
                // into the list where `cur` was — address order is
                // preserved by construction.
                let tail_node: *mut FreeNode = if end < b + s {
                    let t = end as *mut FreeNode;
                    (*t).size = b + s - end;
                    (*t).next = (*cur).next;
                    t
                } else {
                    (*cur).next
                };
                let first: *mut FreeNode = if front >= MIN_FREE {
                    (*cur).size = front;
                    (*cur).next = tail_node;
                    cur
                } else {
                    tail_node
                };
                if prev.is_null() {
                    h.head = first;
                } else {
                    (*prev).next = first;
                }

                let block = end - hdr;
                *(hdr as *mut usize) = block;
                h.used += block;
                if h.used > h.peak {
                    h.peak = h.used;
                }
                return p as *mut u8;
            }

            prev = cur;
            cur = (*cur).next;
        }
        null_mut()
    }

    unsafe fn dealloc(&self, ptr: *mut u8, _layout: Layout) {
        if ptr.is_null() {
            return;
        }
        let hdr = (ptr as usize) - HDR;
        let size = *(hdr as *const usize);

        let mut h = HEAP.lock();
        debug_assert!(h.used >= size, "heap: dealloc underflow");
        h.used -= size;

        // Address-ordered insert with two-way coalescing.
        let node = hdr as *mut FreeNode;
        (*node).size = size;
        let mut prev: *mut FreeNode = null_mut();
        let mut cur = h.head;
        while !cur.is_null() && (cur as usize) < hdr {
            prev = cur;
            cur = (*cur).next;
        }
        (*node).next = cur;
        if prev.is_null() {
            h.head = node;
        } else {
            (*prev).next = node;
        }
        // Merge with the next block if adjacent.
        if !cur.is_null() && hdr + (*node).size == cur as usize {
            (*node).size += (*cur).size;
            (*node).next = (*cur).next;
        }
        // Merge with the previous block if adjacent.
        if !prev.is_null() && (prev as usize) + (*prev).size == hdr {
            (*prev).size += (*node).size;
            (*prev).next = (*node).next;
        }
    }
}

#[global_allocator]
static ALLOC: ListAlloc = ListAlloc;

/// (used, peak) heap bytes — for diagnostics.
#[allow(dead_code)]
pub fn stats() -> (usize, usize) {
    let h = HEAP.lock();
    (h.used, h.peak)
}
