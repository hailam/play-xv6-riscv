//! GIC v2 driver — distributor + CPU interface.
//!
//! Layout on QEMU virt (`-machine virt -cpu cortex-a72`):
//!   GICD at 0x0800_0000 (64 KiB) — distributor (global state)
//!   GICC at 0x0801_0000 (64 KiB) — CPU interface (per-hart view)

use core::ptr::{read_volatile, write_volatile};

use crate::memlayout::GICD;

const GICC_BASE: usize = 0x0801_0000;

// Distributor (GICD) offsets.
const GICD_CTLR: usize = GICD + 0x000;
const GICD_ISENABLER: usize = GICD + 0x100; // 1 bit per IRQ
const GICD_IPRIORITYR: usize = GICD + 0x400; // 8 bits per IRQ
const GICD_ITARGETSR: usize = GICD + 0x800; // 8 bits per IRQ (SPIs)
const _GICD_ICFGR: usize = GICD + 0xC00; // 2 bits per IRQ
const GICD_SGIR: usize = GICD + 0xF00; // SGI dispatch

// CPU interface (GICC) offsets.
const GICC_CTLR: usize = GICC_BASE + 0x000;
const GICC_PMR: usize = GICC_BASE + 0x004;
const GICC_BPR: usize = GICC_BASE + 0x008;
const GICC_IAR: usize = GICC_BASE + 0x00C;
const GICC_EOIR: usize = GICC_BASE + 0x010;

const SPURIOUS_INTID: u32 = 1023;

/// Global init — hart 0 only.
pub unsafe fn init(uart_irq: usize, virtio_irq: usize) {
    unsafe {
        // Disable distributor while we configure.
        write_volatile(GICD_CTLR as *mut u32, 0);

        // For each SPI we use, set mid priority and target hart 0.
        for &spi in &[uart_irq, virtio_irq] {
            // Priority byte.
            write_volatile((GICD_IPRIORITYR + spi) as *mut u8, 0xA0);
            // Target CPU mask (hart 0 = bit 0). SPIs only — PPIs/SGIs
            // are banked.
            write_volatile((GICD_ITARGETSR + spi) as *mut u8, 0x01);
        }

        // Enable distributor (Group 0 — IRQ at EL1NS).
        write_volatile(GICD_CTLR as *mut u32, 1);
    }
}

/// Per-hart init. Enables our PPIs and SPIs from the local CPU
/// interface's perspective.
pub unsafe fn init_for_hart(uart_irq: usize, virtio_irq: usize, timer_ppi: usize) {
    unsafe {
        // Priority mask wide open.
        write_volatile(GICC_PMR as *mut u32, 0xFF);
        write_volatile(GICC_BPR as *mut u32, 0);
        // Enable CPU interface.
        write_volatile(GICC_CTLR as *mut u32, 1);

        // Enable our IRQs (write 1-to-set in ISENABLER). PPIs are
        // banked per-hart in the same register, so each hart writes
        // its own copy.
        enable_irq(uart_irq);
        enable_irq(virtio_irq);
        enable_irq(timer_ppi);
        // SGI #0 is our IPI. SGIs are banked per-hart in the same
        // ISENABLER0 (low 16 bits); every hart needs to enable its
        // own copy to actually receive cross-hart wakers.
        enable_irq(0);
    }
}

unsafe fn enable_irq(id: usize) {
    unsafe {
        let reg = GICD_ISENABLER + (id / 32) * 4;
        let bit = 1u32 << (id % 32);
        write_volatile(reg as *mut u32, bit);
    }
}

/// Claim the top pending IRQ. Returns the INTID, or `SPURIOUS_INTID`
/// (1023) when there's no work. Caller must `complete` on a
/// non-spurious return.
pub fn claim() -> u32 {
    unsafe { read_volatile(GICC_IAR as *const u32) }
}

pub fn complete(intid: u32) {
    unsafe { write_volatile(GICC_EOIR as *mut u32, intid) };
}

#[inline]
pub fn is_spurious(intid: u32) -> bool {
    intid == SPURIOUS_INTID
}

/// Send an SGI to all harts except this one. Reserved for future
/// IPI work.
#[allow(dead_code)]
pub unsafe fn sgi_all_except_self(intid: u32) {
    unsafe {
        let val = (0b01u32 << 24) | (intid & 0xF);
        write_volatile(GICD_SGIR as *mut u32, val);
    }
}
