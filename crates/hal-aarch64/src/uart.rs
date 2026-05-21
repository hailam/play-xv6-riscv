//! PL011 UART driver — QEMU virt aarch64 console.

use core::ptr::{read_volatile, write_volatile};

use crate::memlayout::UART0;

// PL011 register offsets.
const DR: usize = UART0 + 0x000; // data register
const FR: usize = UART0 + 0x018; // flag register (RO)
const IBRD: usize = UART0 + 0x024; // integer baud rate divisor
const FBRD: usize = UART0 + 0x028; // fractional baud rate divisor
const LCRH: usize = UART0 + 0x02C; // line control
const CR: usize = UART0 + 0x030; // control register
const IMSC: usize = UART0 + 0x038; // interrupt mask set/clear
const ICR: usize = UART0 + 0x044; // interrupt clear register

// FR (flag register) bits.
const FR_RXFE: u8 = 1 << 4; // RX FIFO empty
const FR_TXFF: u8 = 1 << 5; // TX FIFO full

// CR (control register) bits.
const CR_UARTEN: u32 = 1 << 0;
const CR_TXE: u32 = 1 << 8;
const CR_RXE: u32 = 1 << 9;

// LCRH (line control) bits.
const LCRH_FEN: u32 = 1 << 4; // FIFO enable
const LCRH_WLEN_8: u32 = 3 << 5; // 8 bits per character

// IMSC (interrupt mask).
const IMSC_RXIM: u32 = 1 << 4; // RX interrupt enable

/// One-time global init — defensive (QEMU usually leaves PL011
/// enabled at boot, but doing the full sequence here matches what
/// the riscv64 path does for NS16550 and works regardless of QEMU
/// state).
pub unsafe fn init() {
    unsafe {
        // 1) Disable while we configure.
        write_volatile(CR as *mut u32, 0);
        // 2) Clear pending interrupts (write 1 to all bits we care about).
        write_volatile(ICR as *mut u32, 0x7FF);
        // 3) Baud divisor for 115200 baud @ 24 MHz UARTCLK.
        //    div = 24_000_000 / (16 * 115200) ≈ 13.0208
        //    IBRD = 13, FBRD = round(0.0208 * 64) = 1
        write_volatile(IBRD as *mut u32, 13);
        write_volatile(FBRD as *mut u32, 1);
        // 4) Line control: 8N1 *without* the FIFO. PL011 RX IRQ only
        //    fires when the FIFO crosses its trigger level (default
        //    1/8 ≈ 4 bytes); typed characters get stuck waiting for
        //    enough buddies to arrive. Disabling FEN makes the
        //    receiver behave as a 1-deep holding register, firing an
        //    IRQ on every byte — same behaviour as our NS16550 path
        //    on riscv.
        write_volatile(LCRH as *mut u32, LCRH_WLEN_8);
        // 5) Unmask RX interrupt.
        write_volatile(IMSC as *mut u32, IMSC_RXIM);
        // 6) Enable UART (TX + RX).
        write_volatile(CR as *mut u32, CR_UARTEN | CR_TXE | CR_RXE);
    }
}

pub fn putc(c: u8) {
    unsafe {
        while (read_volatile(FR as *const u8) & FR_TXFF) != 0 {
            core::hint::spin_loop();
        }
        write_volatile(DR as *mut u8, c);
    }
}

pub fn try_getc() -> Option<u8> {
    unsafe {
        if (read_volatile(FR as *const u8) & FR_RXFE) != 0 {
            None
        } else {
            Some(read_volatile(DR as *const u8))
        }
    }
}
