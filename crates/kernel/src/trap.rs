//! Kernel-side trap orchestration. The actual S-mode trap vector lives
//! in hal-riscv64; here we own the per-hart setup and the timer-tick
//! callback that the HAL invokes from the trap handler.

use core::sync::atomic::{AtomicUsize, Ordering};

use crate::arch::{Arch, Hal};

#[cfg(target_arch = "riscv64")]
use hal_riscv64::{arm_timer, init_kernel_trap_vec};

pub static TICKS: AtomicUsize = AtomicUsize::new(0);

/// Set up the kernel trap vector and arm the first timer tick.
/// Call from each hart after vm install.
pub fn init_this_hart() {
    unsafe { init_kernel_trap_vec() };
    arm_timer();
}

/// Called from the HAL trap handler on supervisor timer interrupt.
/// Re-arms the timer and prints a tick mark every ~1s on hart 0.
#[no_mangle]
pub extern "C" fn kernel_on_timer() {
    let t = TICKS.fetch_add(1, Ordering::Relaxed);
    if Arch::hartid() == 0 && t % 10 == 0 {
        crate::print!(".");
    }
    arm_timer();
}
