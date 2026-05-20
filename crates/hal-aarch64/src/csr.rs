//! Minimal ARMv8 system-register helpers.

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
    // I-bit is bit 7. When set, IRQs are masked.
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

// ---------- system-register accessors ----------

macro_rules! sysreg_read {
    ($fn_name:ident, $reg:ident) => {
        #[inline]
        pub fn $fn_name() -> u64 {
            let v: u64;
            unsafe {
                core::arch::asm!(concat!("mrs {}, ", stringify!($reg)),
                                 out(reg) v, options(nomem, nostack, preserves_flags));
            }
            v
        }
    };
}

macro_rules! sysreg_write {
    ($fn_name:ident, $reg:ident) => {
        #[inline]
        pub unsafe fn $fn_name(v: u64) {
            unsafe {
                core::arch::asm!(concat!("msr ", stringify!($reg), ", {}"),
                                 in(reg) v, options(nomem, nostack, preserves_flags));
            }
        }
    };
}

sysreg_read!(read_sctlr_el1, sctlr_el1);
sysreg_write!(write_sctlr_el1, sctlr_el1);

sysreg_write!(write_ttbr0_el1, ttbr0_el1);
sysreg_write!(write_mair_el1, mair_el1);
sysreg_write!(write_tcr_el1, tcr_el1);

sysreg_read!(read_esr_el1, esr_el1);
sysreg_read!(read_far_el1, far_el1);
sysreg_read!(read_elr_el1, elr_el1);
sysreg_write!(write_elr_el1, elr_el1);
sysreg_read!(read_spsr_el1, spsr_el1);
sysreg_write!(write_spsr_el1, spsr_el1);

sysreg_write!(write_vbar_el1, vbar_el1);

// ---------- barriers + TLB invalidate ----------

#[inline]
pub unsafe fn isb() {
    unsafe { core::arch::asm!("isb", options(nostack, preserves_flags)) }
}

#[inline]
pub unsafe fn dsb_ish() {
    unsafe { core::arch::asm!("dsb ish", options(nostack, preserves_flags)) }
}

#[inline]
pub unsafe fn dsb_ishst() {
    unsafe { core::arch::asm!("dsb ishst", options(nostack, preserves_flags)) }
}

/// Invalidate all stage-1 EL1 TLB entries, broadcast to all PEs in the
/// Inner Shareable domain.
#[inline]
pub unsafe fn tlbi_vmalle1is() {
    unsafe { core::arch::asm!("tlbi vmalle1is", options(nostack, preserves_flags)) }
}

// ---------- ARM generic timer ----------

sysreg_read!(read_cntfrq_el0, cntfrq_el0);
sysreg_write!(write_cntv_tval_el0, cntv_tval_el0);
sysreg_write!(write_cntv_ctl_el0, cntv_ctl_el0);
sysreg_read!(read_cntv_ctl_el0, cntv_ctl_el0);
