//! Kernel-side trap orchestration.

use core::sync::atomic::{AtomicUsize, Ordering};

use hal::Hal;

use crate::arch::Arch;

pub static TICKS: AtomicUsize = AtomicUsize::new(0);

pub fn init_this_hart() {
    unsafe { Arch::init_kernel_trap_vec() };
    Arch::arm_timer();
}

#[no_mangle]
pub extern "C" fn kernel_on_timer() {
    TICKS.fetch_add(1, Ordering::Relaxed);
    crate::time::drain_expired();
    Arch::arm_timer();
}

#[no_mangle]
pub extern "C" fn kernel_on_external(src: u32) {
    let src = src as usize;
    if src == <Arch as Hal>::UART0_IRQ {
        while let Some(c) = Arch::console_try_getc() {
            crate::console_in::push(c);
        }
    } else if src == <Arch as Hal>::VIRTIO0_IRQ {
        crate::driver::virtio_blk::on_irq();
    } else {
        crate::println!("external IRQ {} (no handler)", src);
    }
}
