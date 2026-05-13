//! NS16550A driver for the QEMU virt UART at 0x1000_0000.
//!
//! Polled TX (writes spin on LSR.TX_IDLE); IRQ-driven RX once `init`
//! enables the RX-data-available interrupt.

use core::ptr::{read_volatile, write_volatile};

const UART0: usize = 0x1000_0000;
const RHR: usize = 0;
const THR: usize = 0;
const IER: usize = 1;
const FCR: usize = 2;
const LCR: usize = 3;
const LSR: usize = 5;

const LSR_RX_READY: u8 = 1 << 0;
const LSR_TX_IDLE: u8 = 1 << 5;

const IER_RX_ENABLE: u8 = 1 << 0;

#[inline]
fn reg(off: usize) -> *mut u8 {
    (UART0 + off) as *mut u8
}

pub fn init() {
    unsafe {
        // Disable IER during config.
        write_volatile(reg(IER), 0);
        // DLAB=1 to set divisor; QEMU mostly ignores baud but follow ritual.
        write_volatile(reg(LCR), 1 << 7);
        write_volatile(reg(0), 0x03); // DLL
        write_volatile(reg(1), 0x00); // DLM
        // 8N1, DLAB=0.
        write_volatile(reg(LCR), 3);
        // Enable + clear FIFOs.
        write_volatile(reg(FCR), 0x07);
        // Enable RX-data-available IRQ.
        write_volatile(reg(IER), IER_RX_ENABLE);
    }
}

pub fn putc(c: u8) {
    unsafe {
        while read_volatile(reg(LSR)) & LSR_TX_IDLE == 0 {
            core::hint::spin_loop();
        }
        write_volatile(reg(THR), c);
    }
}

/// Non-blocking RX. Used by the IRQ handler to drain the FIFO.
pub fn try_getc() -> Option<u8> {
    unsafe {
        if read_volatile(reg(LSR)) & LSR_RX_READY == 0 {
            None
        } else {
            Some(read_volatile(reg(RHR)))
        }
    }
}
