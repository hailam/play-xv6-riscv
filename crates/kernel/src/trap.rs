//! Kernel-side trap orchestration.

use core::sync::atomic::{AtomicUsize, Ordering};

#[cfg(target_arch = "riscv64")]
use hal_riscv64::{arm_timer, init_kernel_trap_vec, memlayout::UART0_IRQ};

pub static TICKS: AtomicUsize = AtomicUsize::new(0);

pub fn init_this_hart() {
    unsafe { init_kernel_trap_vec() };
    arm_timer();
}

#[no_mangle]
pub extern "C" fn kernel_on_timer() {
    TICKS.fetch_add(1, Ordering::Relaxed);
    crate::time::drain_expired();
    arm_timer();
}

#[no_mangle]
pub extern "C" fn kernel_on_external(src: u32) {
    if src as usize == UART0_IRQ {
        while let Some(c) = hal_riscv64::uart::try_getc() {
            crate::console_in::push(c);
        }
    } else {
        crate::println!("external IRQ {} (no handler)", src);
    }
}
