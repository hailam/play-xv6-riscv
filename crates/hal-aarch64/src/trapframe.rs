//! Per-process trap frame skeleton for aarch64. Field layout will
//! match the (future) EL0↔EL1 trampoline; for now this is just
//! enough for the trait surface to compile.

use hal::TrapFrameAccess;

#[repr(C)]
#[derive(Debug, Default, Clone, Copy)]
pub struct TrapFrame {
    pub kernel_satp: u64,    // TTBR0_EL1 on aarch64
    pub kernel_sp: u64,
    pub kernel_trap: u64,
    pub kernel_hartid: u64,
    pub elr_el1: u64,        // ELR_EL1 = epc
    pub sp_el0: u64,         // SP_EL0 = user sp
    pub spsr_el1: u64,
    pub x: [u64; 31],        // x0..x30  — offsets 56..304
    /// ESR_EL1 captured at trap time by the trampoline. We must
    /// stash this because per ARM ARM, an IRQ taken to EL1 leaves
    /// ESR_EL1 in an UNKNOWN state, and even a brief window
    /// between the user-mode SVC and `decode_user_trap` is enough
    /// to lose the original syscall EC bits — surfacing as
    /// sporadic "unknown syscall nr" log lines under any load.
    pub esr_el1: u64,        // offset 304
    /// Trap kind, written by the trampoline:
    ///   0 = synchronous (use esr_el1 for EC decode)
    ///   1 = IRQ (esr_el1 is UNKNOWN per ARM ARM; ignore it)
    pub trap_kind: u64,      // offset 312
}

impl TrapFrameAccess for TrapFrame {
    #[inline]
    fn epc(&self) -> u64 {
        self.elr_el1
    }
    #[inline]
    fn set_epc(&mut self, v: u64) {
        self.elr_el1 = v;
    }

    #[inline]
    fn sp(&self) -> u64 {
        self.sp_el0
    }
    #[inline]
    fn set_sp(&mut self, v: u64) {
        self.sp_el0 = v;
    }

    #[inline]
    fn arg(&self, n: usize) -> u64 {
        if n < 8 { self.x[n] } else { 0 }
    }
    #[inline]
    fn set_arg(&mut self, n: usize, v: u64) {
        if n < 8 {
            self.x[n] = v;
        }
    }

    /// Linux/aarch64 convention: syscall number in `x8`.
    #[inline]
    fn syscall_nr(&self) -> u64 {
        self.x[8]
    }

    #[inline]
    fn set_kernel_satp(&mut self, v: u64) {
        self.kernel_satp = v;
    }
    #[inline]
    fn set_kernel_sp(&mut self, v: u64) {
        self.kernel_sp = v;
    }
    #[inline]
    fn set_kernel_trap(&mut self, v: u64) {
        self.kernel_trap = v;
    }
    #[inline]
    fn set_kernel_hartid(&mut self, v: u64) {
        self.kernel_hartid = v;
    }
}
