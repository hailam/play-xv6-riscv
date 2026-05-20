#![no_std]

//! `Hal` impl for QEMU `-machine virt` riscv64.

use hal::Hal;

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
