#![no_std]

//! `Hal` impl for QEMU `-machine virt` riscv64.

use hal::{Hal, TrapFrameAccess, UserTrapCause};

mod csr;
pub mod memlayout;
mod pagetable;
pub mod plic;
mod start;
mod trap;
mod trap_hook;
pub mod trapframe;
pub mod uart;

pub use pagetable::{install_free_frame, PageTable};
pub use trap::{arm_timer, handle_external_irq, init_kernel_trap_vec, TIMER_INTERVAL};
pub use trapframe::TrapFrame;

core::arch::global_asm!(include_str!("../asm/entry.S"));
core::arch::global_asm!(include_str!("../asm/kernelvec.S"));
core::arch::global_asm!(include_str!("../asm/trampoline.S"));

pub struct Riscv64;

pub const MAX_CPUS: usize = 8;

extern "C" {
    pub fn trampoline();
    pub fn uservec();
    pub fn userret();
}

#[inline]
pub fn trampoline_pa() -> usize {
    trampoline as *const () as usize
}

#[inline]
pub fn uservec_offset() -> usize {
    uservec as *const () as usize - trampoline as *const () as usize
}

#[inline]
pub fn userret_offset() -> usize {
    userret as *const () as usize - trampoline as *const () as usize
}

impl Hal for Riscv64 {
    type PageTable = PageTable;
    type TrapFrame = TrapFrame;

    const PGSIZE: usize = memlayout::PGSIZE;
    const KERNBASE: usize = memlayout::KERNBASE;
    const PHYSTOP: usize = memlayout::PHYSTOP;
    const TRAMPOLINE: usize = memlayout::TRAMPOLINE;
    const TRAPFRAME: usize = memlayout::TRAPFRAME;
    const TIMER_INTERVAL: u64 = trap::TIMER_INTERVAL;

    const UART0: usize = memlayout::UART0;
    const UART0_SIZE: usize = memlayout::UART0_SIZE;
    const VIRTIO0: usize = memlayout::VIRTIO0;
    const VIRTIO0_SIZE: usize = memlayout::VIRTIO0_SIZE;
    const INTC_BASE: usize = memlayout::PLIC;
    const INTC_SIZE: usize = memlayout::PLIC_SIZE;
    const UART0_IRQ: usize = memlayout::UART0_IRQ;
    const VIRTIO0_IRQ: usize = memlayout::VIRTIO0_IRQ;

    fn trampoline_pa() -> usize {
        trampoline_pa()
    }
    fn uservec_offset() -> usize {
        uservec_offset()
    }
    fn userret_offset() -> usize {
        userret_offset()
    }

    #[inline(always)]
    fn hartid() -> usize {
        csr::read_tp()
    }

    fn ncpus() -> usize {
        MAX_CPUS
    }

    unsafe fn intr_off() {
        csr::intr_off();
    }
    unsafe fn intr_on() {
        csr::intr_on();
    }
    fn intr_get() -> bool {
        csr::intr_get()
    }
    unsafe fn wfi() {
        csr::wfi();
    }
    unsafe fn send_ipi(_hart_mask: u64) {}

    fn console_putc(c: u8) {
        uart::putc(c);
    }

    fn console_try_getc() -> Option<u8> {
        uart::try_getc()
    }

    fn now_ticks() -> u64 {
        csr::read_time() as u64
    }

    unsafe fn install_pagetable(pt: &PageTable) {
        Self::write_satp(pagetable::satp_value(pt));
    }

    fn pagetable_satp(pt: &PageTable) -> usize {
        pagetable::satp_value(pt)
    }

    unsafe fn write_satp(satp: usize) {
        csr::write_satp(satp);
        csr::sfence_vma();
    }

    unsafe fn on_user_trap_entry() {
        // Redirect stvec away from `uservec` (the user-mode entry) and
        // back to `kernelvec` so that any kernel-mode trap from now on
        // doesn't bounce through the user-mode save path.
        extern "C" {
            fn kernelvec();
        }
        unsafe { csr::write_stvec(kernelvec as *const () as usize) };
    }

    fn decode_user_trap(tf: &mut Self::TrapFrame) -> UserTrapCause {
        // Save the trapping PC into the trapframe so the kernel can
        // inspect it / advance it for syscalls.
        let raw_sepc = csr::read_sepc();
        tf.set_epc(raw_sepc as u64);

        const SCAUSE_ECALL_FROM_U: usize = 8;
        const SCAUSE_LOAD_PAGE_FAULT: usize = 13;
        const SCAUSE_STORE_PAGE_FAULT: usize = 15;
        const SCAUSE_INTERRUPT: usize = 1usize << 63;
        const SCAUSE_TIMER: usize = 5;
        const SCAUSE_EXTERNAL: usize = 9;

        let scause = csr::read_scause();
        if scause == SCAUSE_ECALL_FROM_U {
            // Advance past the 4-byte `ecall`.
            tf.set_epc(tf.epc() + 4);
            return UserTrapCause::Syscall;
        }
        if scause & SCAUSE_INTERRUPT != 0 {
            let code = scause & !SCAUSE_INTERRUPT;
            return match code {
                SCAUSE_TIMER => UserTrapCause::Timer,
                SCAUSE_EXTERNAL => UserTrapCause::Devintr,
                _ => UserTrapCause::Unknown { code: scause, va: 0 },
            };
        }
        // Synchronous exception.
        let va = csr::read_stval();
        match scause {
            SCAUSE_LOAD_PAGE_FAULT => UserTrapCause::PageFault { va, write: false },
            SCAUSE_STORE_PAGE_FAULT => UserTrapCause::PageFault { va, write: true },
            _ => UserTrapCause::Unknown { code: scause, va },
        }
    }

    fn arm_timer() {
        trap::arm_timer();
    }

    fn handle_external_irq() {
        trap::handle_external_irq();
    }

    unsafe fn init_kernel_trap_vec() {
        unsafe { trap::init_kernel_trap_vec() };
    }

    unsafe fn init_console() {
        uart::init();
    }

    unsafe fn init_intc_global() {
        plic::init();
    }

    unsafe fn init_intc_per_hart() {
        plic::init_for_hart();
    }

    unsafe fn install_free_frame(free: unsafe fn(usize)) {
        pagetable::install_free_frame(free);
    }

    unsafe fn return_to_user(tf: &mut Self::TrapFrame, user_satp: usize) -> !{
        // sstatus: clear SPP (return to U-mode), set SPIE (re-enable
        // SIE on sret), set FS=Initial so user-mode FP/D doesn't
        // trap. We don't context-switch FP regs across processes
        // yet — fine because only one user task touches FP at a
        // time today (lua). When multi-FP-process scheduling lands,
        // save/restore the F regs in usertrap.
        const SSTATUS_SPP: usize = 1 << 8;
        const SSTATUS_SPIE: usize = 1 << 5;
        const SSTATUS_FS_MASK: usize = 0b11 << 13;
        const SSTATUS_FS_INITIAL: usize = 0b01 << 13;
        let mut sstatus = csr::read_sstatus();
        sstatus &= !SSTATUS_SPP;
        sstatus |= SSTATUS_SPIE;
        sstatus = (sstatus & !SSTATUS_FS_MASK) | SSTATUS_FS_INITIAL;
        unsafe { csr::write_sstatus(sstatus) };

        unsafe { csr::write_sepc(tf.epc() as usize) };

        // Switch stvec to uservec — the trampoline page's user entry.
        let uservec_va = memlayout::TRAMPOLINE + uservec_offset();
        unsafe { csr::write_stvec(uservec_va) };

        // Jump into userret in the trampoline. It does the satp swap
        // + register restore + sret.
        let userret_va = memlayout::TRAMPOLINE + userret_offset();
        let userret_fn: extern "C" fn(usize) -> ! =
            unsafe { core::mem::transmute(userret_va) };
        userret_fn(user_satp);
    }
}

pub mod csr_api {
    use crate::csr;

    pub use csr::{
        intr_off, intr_on, read_scause, read_sepc, read_sstatus, read_stval, sfence_vma,
        write_sepc, write_sstatus, write_stvec,
    };

    pub const SSTATUS_SPP: usize = 1 << 8;
    pub const SSTATUS_SPIE: usize = 1 << 5;
    pub const SSTATUS_SIE: usize = csr::SSTATUS_SIE;
}
