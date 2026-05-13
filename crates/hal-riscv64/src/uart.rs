//! Minimal NS16550A driver for the QEMU virt UART at 0x1000_0000.
//!
//! Phase 1: polled, no IRQ, no buffering. The caller is expected to hold
//! whatever lock keeps output coherent across harts.

use core::ptr::{read_volatile, write_volatile};

const UART0: usize = 0x1000_0000;
const RHR: usize = 0; // RX holding (when LCR.DLAB=0)
const THR: usize = 0; // TX holding (when LCR.DLAB=0)
const LSR: usize = 5; // line status
const LSR_TX_IDLE: u8 = 1 << 5;

#[inline]
fn reg(off: usize) -> *mut u8 {
    (UART0 + off) as *mut u8
}

pub fn putc(c: u8) {
    unsafe {
        while read_volatile(reg(LSR)) & LSR_TX_IDLE == 0 {
            core::hint::spin_loop();
        }
        write_volatile(reg(THR), c);
    }
}

/// Polled, non-blocking read; returns `None` if no char is ready.
#[allow(dead_code)]
pub fn getc() -> Option<u8> {
    unsafe {
        if read_volatile(reg(LSR)) & 1 == 0 {
            None
        } else {
            Some(read_volatile(reg(RHR)))
        }
    }
}
