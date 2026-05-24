//! Kernel-side trap plumbing for aarch64.
//!
//! `rust_kerneltrap_irq` is called from the VBAR_EL1 vector slot
//! 0x280 (current EL, SP_ELx, IRQ). It claims from the GIC,
//! dispatches by INTID, and EOIs.
//!
//! `rust_kerneltrap_sync` is called from slot 0x200 (synchronous
//! exception while at EL1) — those are kernel bugs (kernel page
//! fault etc.); we panic with the relevant CSRs.
//!
//! `rust_bad_trap` covers all the other slots (current-EL FIQ,
//! lower-EL AArch32, etc.) that we don't expect to fire.

use core::sync::atomic::{AtomicU64, Ordering};

use crate::csr;
use crate::gic;
use crate::memlayout::{UART0_IRQ, VIRTIO0_IRQ};

/// PPI 27 — virtual generic timer.
const VIRT_TIMER_PPI: usize = 27;

extern "C" {
    pub fn kernel_vector_table();

    // Provided by the kernel binary (see crates/kernel/src/trap.rs).
    fn kernel_on_timer();
    fn kernel_on_external(src: u32);
}

/// Used to confirm timer ticks are firing (Phase D verification).
pub static TIMER_TICKS: AtomicU64 = AtomicU64::new(0);

pub unsafe fn init_kernel_trap_vec() {
    unsafe {
        csr::write_vbar_el1(vector_table_addr());
        csr::isb();
    }
}

/// Public so `Hal::on_user_trap_entry` can swap VBAR_EL1 back to the
/// kernel vector table after a user trap. The address here is a
/// kernel VA reachable only once TTBR0_EL1 has been switched back.
pub fn vector_table_addr() -> u64 {
    kernel_vector_table as *const () as u64
}

pub fn arm_timer() {
    let interval = csr::read_cntfrq_el0() / 100; // 10ms tick
    unsafe {
        csr::write_cntv_tval_el0(interval);
        csr::write_cntv_ctl_el0(1); // ENABLE=1, IMASK=0
    }
}

pub fn handle_external_irq() {
    // Called from the user-trap dispatch (Phase E) for SCAUSE_EXTERNAL
    // equivalent. Mirror the kernel-IRQ handler's logic for non-timer.
    let intid = gic::claim();
    if gic::is_spurious(intid) {
        return;
    }
    let src = intid as u32;
    if (intid as usize) == VIRT_TIMER_PPI {
        // Shouldn't happen via this entry (timer handled inside
        // rust_kerneltrap_irq), but be defensive.
        TIMER_TICKS.fetch_add(1, Ordering::Relaxed);
        unsafe { kernel_on_timer() };
    } else {
        unsafe { kernel_on_external(src) };
    }
    gic::complete(intid);
}

#[no_mangle]
pub extern "C" fn rust_kerneltrap_irq() {
    let intid = gic::claim();
    if gic::is_spurious(intid) {
        return;
    }
    if intid < 16 {
        // SGI (Software-Generated Interrupt) — our cross-hart IPI.
        // The recipient doesn't need to *do* anything explicit; the
        // ack here exits `wfi` and the executor's run loop picks up
        // any newly-queued tasks on its next iteration. Just claim
        // and complete.
    } else if (intid as usize) == VIRT_TIMER_PPI {
        // Push the deadline far out so we don't re-fire inside the
        // hook. The kernel's `kernel_on_timer` (and `arm_timer`) will
        // re-program the proper next tick.
        unsafe { csr::write_cntv_tval_el0(i64::MAX as u64) };
        TIMER_TICKS.fetch_add(1, Ordering::Relaxed);
        unsafe { kernel_on_timer() };
    } else {
        unsafe { kernel_on_external(intid as u32) };
    }
    gic::complete(intid);
}

#[no_mangle]
pub extern "C" fn rust_kerneltrap_sync() -> ! {
    let esr = csr::read_esr_el1();
    let elr = csr::read_elr_el1();
    let far = csr::read_far_el1();
    panic!(
        "kerneltrap: ESR_EL1={:#x} ELR_EL1={:#x} FAR_EL1={:#x} (EC={:#x})",
        esr,
        elr,
        far,
        (esr >> 26) & 0x3F,
    );
}

#[no_mangle]
pub extern "C" fn rust_bad_trap() -> ! {
    let esr = csr::read_esr_el1();
    let elr = csr::read_elr_el1();
    let far = csr::read_far_el1();
    panic!(
        "bad_trap: ESR_EL1={:#x} ELR_EL1={:#x} FAR_EL1={:#x}",
        esr, elr, far,
    );
}

pub use crate::memlayout::{UART0_IRQ as KUART0_IRQ, VIRTIO0_IRQ as KVIRTIO0_IRQ};

/// Convenience init the Hal calls expose.
pub unsafe fn init_intc_global() {
    unsafe { gic::init(UART0_IRQ, VIRTIO0_IRQ) }
}

pub unsafe fn init_intc_per_hart() {
    unsafe {
        gic::init_for_hart(UART0_IRQ, VIRTIO0_IRQ, VIRT_TIMER_PPI);
        enable_fp_simd_for_el0_and_el1();
    }
}

/// Set `CPACR_EL1.FPEN = 0b11` so FP/SIMD/NEON instructions don't
/// trap at EL0 or EL1. entry.S already does this on the EL2→EL1
/// drop path, but secondary-hart bringup via PSCI may land us at
/// EL1 directly without that drop, leaving FPEN at its reset
/// default (which traps). Re-setting here makes every hart safe.
///
/// Why this matters: clang on aarch64 emits NEON loads/stores
/// (e.g. `stp q0, q1, [sp]`) for variadic-arg-passing in `printf`
/// and for any FP code. Without FPEN=0b11, the first such
/// instruction in user space (and the kernel!) faults with
/// ESR.EC=0x07.
unsafe fn enable_fp_simd_for_el0_and_el1() {
    unsafe {
        core::arch::asm!(
            "mrs {t}, cpacr_el1",
            "orr {t}, {t}, #(3 << 20)",
            "msr cpacr_el1, {t}",
            "isb",
            t = out(reg) _,
        );
    }
}
