//! Platform-Level Interrupt Controller (PLIC).
//!
//! xv6/QEMU virt layout:
//!   - source priority: PLIC + 4 * src
//!   - per-context S-mode enable bitmap: PLIC + 0x2000 + 0x80 * ctx
//!     (ctx for hart h = 2*h + 1)
//!   - per-context S-mode threshold: PLIC + 0x200000 + 0x1000 * ctx
//!   - per-context S-mode claim/complete: PLIC + 0x200004 + 0x1000 * ctx

use core::ptr::{read_volatile, write_volatile};

use crate::csr;
use crate::memlayout::PLIC;

const ENABLE_BASE: usize = PLIC + 0x2000;
const CTX_BLOCK_BASE: usize = PLIC + 0x200000;

#[inline]
fn s_context(hartid: usize) -> usize {
    2 * hartid + 1
}

#[inline]
fn enable_ptr(hartid: usize) -> *mut u32 {
    let ctx = s_context(hartid);
    (ENABLE_BASE + ctx * 0x80) as *mut u32
}

#[inline]
fn threshold_ptr(hartid: usize) -> *mut u32 {
    let ctx = s_context(hartid);
    (CTX_BLOCK_BASE + ctx * 0x1000) as *mut u32
}

#[inline]
fn claim_ptr(hartid: usize) -> *mut u32 {
    let ctx = s_context(hartid);
    (CTX_BLOCK_BASE + ctx * 0x1000 + 4) as *mut u32
}

/// Global PLIC init: set per-source priorities. Call once.
pub fn init() {
    unsafe {
        // Set UART (source 10) priority to 1 (anything > 0 = enabled).
        write_volatile((PLIC + 4 * crate::memlayout::UART0_IRQ) as *mut u32, 1);
        // Likewise virtio at source 1.
        write_volatile((PLIC + 4 * crate::memlayout::VIRTIO0_IRQ) as *mut u32, 1);
    }
}

/// Per-hart PLIC init: enable our sources and lower threshold so they
/// actually deliver.
pub fn init_for_hart() {
    let hartid = csr::read_tp();
    let enable_mask: u32 = (1 << crate::memlayout::UART0_IRQ)
        | (1 << crate::memlayout::VIRTIO0_IRQ);
    unsafe {
        write_volatile(enable_ptr(hartid), enable_mask);
        write_volatile(threshold_ptr(hartid), 0);
    }
}

/// Claim a pending IRQ. Returns its source ID (0 if none).
pub fn claim() -> u32 {
    let hartid = csr::read_tp();
    unsafe { read_volatile(claim_ptr(hartid)) }
}

/// Signal completion of a previously claimed IRQ.
pub fn complete(src: u32) {
    let hartid = csr::read_tp();
    unsafe { write_volatile(claim_ptr(hartid), src) }
}
