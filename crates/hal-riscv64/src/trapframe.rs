//! Per-process trap frame — laid out to match `trampoline.S`. Modifying
//! this struct without updating the asm offsets WILL cause silent corruption.

use hal::TrapFrameAccess;

#[repr(C)]
#[derive(Debug, Default, Clone, Copy)]
pub struct TrapFrame {
    // 0
    pub kernel_satp: u64,
    pub kernel_sp: u64,
    pub kernel_trap: u64,
    pub epc: u64,
    pub kernel_hartid: u64,
    // 40
    pub ra: u64,
    pub sp: u64,
    pub gp: u64,
    pub tp: u64,
    pub t0: u64,
    pub t1: u64,
    pub t2: u64,
    pub s0: u64,
    pub s1: u64,
    // 112
    pub a0: u64,
    pub a1: u64,
    pub a2: u64,
    pub a3: u64,
    pub a4: u64,
    pub a5: u64,
    pub a6: u64,
    pub a7: u64,
    // 176
    pub s2: u64,
    pub s3: u64,
    pub s4: u64,
    pub s5: u64,
    pub s6: u64,
    pub s7: u64,
    pub s8: u64,
    pub s9: u64,
    pub s10: u64,
    pub s11: u64,
    // 256
    pub t3: u64,
    pub t4: u64,
    pub t5: u64,
    pub t6: u64,
}

impl TrapFrameAccess for TrapFrame {
    #[inline]
    fn epc(&self) -> u64 {
        self.epc
    }
    #[inline]
    fn set_epc(&mut self, v: u64) {
        self.epc = v;
    }

    #[inline]
    fn sp(&self) -> u64 {
        self.sp
    }
    #[inline]
    fn set_sp(&mut self, v: u64) {
        self.sp = v;
    }

    #[inline]
    fn arg(&self, n: usize) -> u64 {
        match n {
            0 => self.a0,
            1 => self.a1,
            2 => self.a2,
            3 => self.a3,
            4 => self.a4,
            5 => self.a5,
            _ => 0,
        }
    }
    #[inline]
    fn set_arg(&mut self, n: usize, v: u64) {
        match n {
            0 => self.a0 = v,
            1 => self.a1 = v,
            2 => self.a2 = v,
            3 => self.a3 = v,
            _ => {}
        }
    }

    #[inline]
    fn syscall_nr(&self) -> u64 {
        self.a7
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

// Compile-time check that the layout matches what trampoline.S assumes.
const _: () = {
    assert!(core::mem::size_of::<TrapFrame>() == 288);
    assert!(core::mem::offset_of!(TrapFrame, kernel_satp) == 0);
    assert!(core::mem::offset_of!(TrapFrame, kernel_sp) == 8);
    assert!(core::mem::offset_of!(TrapFrame, kernel_trap) == 16);
    assert!(core::mem::offset_of!(TrapFrame, epc) == 24);
    assert!(core::mem::offset_of!(TrapFrame, kernel_hartid) == 32);
    assert!(core::mem::offset_of!(TrapFrame, ra) == 40);
    assert!(core::mem::offset_of!(TrapFrame, a0) == 112);
    assert!(core::mem::offset_of!(TrapFrame, t6) == 280);
};
