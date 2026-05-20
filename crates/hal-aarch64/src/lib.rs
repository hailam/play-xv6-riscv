#![no_std]

//! `Hal` impl skeleton for QEMU `-machine virt` aarch64.
//!
//! **Status: skeleton.** The trait impl compiles and the crate
//! builds standalone for `aarch64-unknown-none-softfloat`. The
//! kernel does *not* yet build for aarch64 — see the
//! `aarch64-completion` todo for the remaining work (trap vectors,
//! actual pagetable populate, GIC, EL2→EL1 drop, kernel-side
//! `#[cfg(target_arch="riscv64")]` scrubbing).

use hal::Hal;

mod csr;
pub mod memlayout;
mod pagetable;
pub mod uart;

pub use pagetable::PageTable;

pub struct AArch64;

pub const MAX_CPUS: usize = 8;

impl Hal for AArch64 {
    type PageTable = PageTable;

    const PGSIZE: usize = memlayout::PGSIZE;
    const PHYSTOP: usize = memlayout::PHYSTOP;
    const TRAMPOLINE: usize = memlayout::TRAMPOLINE;
    const TRAPFRAME: usize = memlayout::TRAPFRAME;
    /// Skeleton placeholder — real value comes from the ARM generic
    /// timer counter frequency once the boot path is wired.
    const TIMER_INTERVAL: u64 = 1_000_000;

    /// Trampoline address resolution comes with the boot follow-up.
    fn trampoline_pa() -> usize {
        0
    }
    fn uservec_offset() -> usize {
        0
    }
    fn userret_offset() -> usize {
        0
    }

    #[inline(always)]
    fn hartid() -> usize {
        csr::read_mpidr_el1()
    }

    fn ncpus() -> usize {
        MAX_CPUS
    }

    unsafe fn intr_off() {
        unsafe { csr::intr_off() };
    }
    unsafe fn intr_on() {
        unsafe { csr::intr_on() };
    }
    fn intr_get() -> bool {
        csr::intr_get()
    }
    unsafe fn wfi() {
        unsafe { csr::wfi() };
    }
    unsafe fn send_ipi(_hart_mask: u64) {
        // TODO: GIC SGI write. Skeleton no-op (mirrors riscv64).
    }

    fn console_putc(c: u8) {
        uart::putc(c);
    }

    fn now_ticks() -> u64 {
        csr::read_cntvct_el0()
    }

    unsafe fn install_pagetable(pt: &PageTable) {
        unsafe { Self::write_satp(pagetable::ttbr0_value(pt)) };
    }

    fn pagetable_satp(pt: &PageTable) -> usize {
        pagetable::ttbr0_value(pt)
    }

    /// Named `write_satp` to match the trait — on aarch64 this writes
    /// TTBR0_EL1. The riscv-flavoured name is a temporary quirk;
    /// renaming the trait method is in the follow-up todo.
    unsafe fn write_satp(satp: usize) {
        unsafe { csr::write_ttbr0_el1(satp) };
    }
}
