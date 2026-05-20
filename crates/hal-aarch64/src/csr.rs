//! Minimal ARMv8 system-register helpers. Just enough for `impl Hal`.
//!
//! Skeleton phase: stubs that return zero / no-op. Real implementations
//! land with the follow-up boot todo.

#[inline]
pub fn read_mpidr_el1() -> usize {
    let v: usize;
    unsafe {
        core::arch::asm!("mrs {}, mpidr_el1", out(reg) v, options(nomem, nostack, preserves_flags));
    }
    v & 0xff // aff0
}

#[inline]
pub unsafe fn intr_off() {
    unsafe {
        core::arch::asm!("msr daifset, #2", options(nomem, nostack, preserves_flags));
    }
}

#[inline]
pub unsafe fn intr_on() {
    unsafe {
        core::arch::asm!("msr daifclr, #2", options(nomem, nostack, preserves_flags));
    }
}

#[inline]
pub fn intr_get() -> bool {
    let daif: usize;
    unsafe {
        core::arch::asm!("mrs {}, daif", out(reg) daif, options(nomem, nostack, preserves_flags));
    }
    // I-bit is bit 7 (DAIF.I). When set, IRQs are masked.
    (daif & (1 << 7)) == 0
}

#[inline]
pub unsafe fn wfi() {
    unsafe { core::arch::asm!("wfi", options(nomem, nostack, preserves_flags)) }
}

#[inline]
pub fn read_cntvct_el0() -> u64 {
    let v: u64;
    unsafe {
        core::arch::asm!("mrs {}, cntvct_el0", out(reg) v, options(nomem, nostack, preserves_flags));
    }
    v
}

/// Install a translation-table root in TTBR0_EL1 + invalidate TLB.
/// Skeleton — real impl will need ISB / TLBI VMALLE1IS.
#[inline]
pub unsafe fn write_ttbr0_el1(_root_pa: usize) {
    // TODO: actual write + isb + tlbi
}
