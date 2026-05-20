//! PL011 UART skeleton. QEMU virt aarch64 console.
//!
//! Real init wires up the FIFO, baud, control register, and the
//! RX-interrupt enable. The `putc` here just spins on the TX-FIFO
//! flag; that's the only path the skeleton needs for early prints.

use core::ptr::{read_volatile, write_volatile};

use crate::memlayout::UART0;

const DR: usize = UART0 + 0x000; // data
const FR: usize = UART0 + 0x018; // flag register
const FR_TXFF: u8 = 1 << 5; // TX FIFO full

pub fn putc(c: u8) {
    unsafe {
        while (read_volatile(FR as *const u8) & FR_TXFF) != 0 {
            core::hint::spin_loop();
        }
        write_volatile(DR as *mut u8, c);
    }
}

/// No-op init. Real impl: disable UART, set baud, enable RX+TX,
/// program LCRH, enable UART. PL011 init is ~30 lines.
pub fn init() {}
