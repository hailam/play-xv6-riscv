//! PSCI 0.2+ client over HVC. QEMU `-machine virt` uses HVC as the
//! PSCI conduit when launched without firmware (we boot at EL2, drop
//! to EL1, and PSCI sits at EL2 in the QEMU stub).
//!
//! We only implement `CPU_ON` — enough to boot secondary harts.

use core::arch::asm;

/// 32-bit PSCI Function IDs (DEN0022D).
const PSCI_CPU_ON_32: u32 = 0x8400_0003;

/// Issue a PSCI call via HVC #0. Returns the PSCI return code in x0.
///
/// Per DEN0022D §5.2, registers x0..x3 carry args, x0 carries result.
/// Caller-saved x4..x17 are clobbered by the SMCCC spec — we mark
/// the asm clobbers conservatively.
#[inline(always)]
unsafe fn hvc(func: u32, arg1: u64, arg2: u64, arg3: u64) -> i64 {
    let ret: i64;
    unsafe {
        asm!(
            "hvc #0",
            inout("x0") func as u64 => ret,
            in("x1") arg1,
            in("x2") arg2,
            in("x3") arg3,
            lateout("x4") _, lateout("x5") _, lateout("x6") _,
            lateout("x7") _, lateout("x8") _, lateout("x9") _,
            lateout("x10") _, lateout("x11") _, lateout("x12") _,
            lateout("x13") _, lateout("x14") _, lateout("x15") _,
            lateout("x16") _, lateout("x17") _,
            options(nostack),
        );
    }
    ret
}

/// Power on a secondary CPU.
///
/// * `target_mpidr` — MPIDR_EL1 value of the target CPU (Aff3/2/1/0).
/// * `entry_pa`     — physical entry address (e.g., `_entry`).
/// * `context_id`   — passed to the target in x0.
///
/// Returns 0 on success or a negative PSCI error code:
///   -1 NOT_SUPPORTED, -2 INVALID_PARAMETERS, -4 DENIED,
///   -5 ALREADY_ON, -6 ON_PENDING, -7 INTERNAL_FAILURE.
pub unsafe fn cpu_on(target_mpidr: u64, entry_pa: usize, context_id: u64) -> i64 {
    unsafe { hvc(PSCI_CPU_ON_32, target_mpidr, entry_pa as u64, context_id) }
}
