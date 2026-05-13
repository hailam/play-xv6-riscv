//! Kernel-side trap orchestration.

use core::sync::atomic::{AtomicUsize, Ordering};

#[cfg(target_arch = "riscv64")]
use hal_riscv64::{arm_timer, init_kernel_trap_vec};

pub static TICKS: AtomicUsize = AtomicUsize::new(0);

pub fn init_this_hart() {
    unsafe { init_kernel_trap_vec() };
    arm_timer();
}

/// Called by the HAL trap handler on supervisor timer interrupt while
/// the kernel is running (i.e. no user proc in user mode on this hart).
#[no_mangle]
pub extern "C" fn kernel_on_timer() {
    TICKS.fetch_add(1, Ordering::Relaxed);
    crate::time::drain_expired();
    arm_timer();
}
