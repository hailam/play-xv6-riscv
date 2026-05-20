#![no_std]

//! `Hal` impl skeleton for QEMU `-machine virt` aarch64.
//!
//! **Status: skeleton.** The trait impl compiles and the crate
//! builds standalone for `aarch64-unknown-none-softfloat`. The
//! kernel does *not* yet build for aarch64 — see the
//! `aarch64-completion` todo for the remaining work (trap vectors,
//! actual pagetable populate, GIC, EL2→EL1 drop, kernel-side
//! `#[cfg(target_arch="riscv64")]` scrubbing).

use hal::{Hal, UserTrapCause};

mod csr;
pub mod memlayout;
mod pagetable;
mod start;
pub mod trapframe;
pub mod uart;

pub use pagetable::PageTable;
pub use trapframe::TrapFrame;

pub struct AArch64;

pub const MAX_CPUS: usize = 8;

impl Hal for AArch64 {
    type PageTable = PageTable;
    type TrapFrame = TrapFrame;

    const PGSIZE: usize = memlayout::PGSIZE;
    const KERNBASE: usize = memlayout::KERNBASE;
    const PHYSTOP: usize = memlayout::PHYSTOP;
    const TRAMPOLINE: usize = memlayout::TRAMPOLINE;
    const TRAPFRAME: usize = memlayout::TRAPFRAME;

    const UART0: usize = memlayout::UART0;
    const UART0_SIZE: usize = memlayout::UART0_SIZE;
    const VIRTIO0: usize = memlayout::VIRTIO0;
    const VIRTIO0_SIZE: usize = memlayout::VIRTIO0_SIZE;
    const INTC_BASE: usize = memlayout::GICD;
    const INTC_SIZE: usize = memlayout::GICD_SIZE;
    const UART0_IRQ: usize = memlayout::UART0_IRQ;
    const VIRTIO0_IRQ: usize = memlayout::VIRTIO0_IRQ;
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

    fn console_try_getc() -> Option<u8> {
        uart::try_getc()
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

    // ----- trap surface — skeleton -----
    //
    // None of these have real implementations yet. They exist so the
    // arch-independent kernel builds. Each unimplemented arm panics
    // explicitly so the missing piece is obvious if anyone tries to
    // actually run.

    unsafe fn on_user_trap_entry() {
        // aarch64's single VBAR_EL1 handles dispatch by entry slot,
        // so there's no per-entry vector swap. Real impl: nothing.
    }

    fn decode_user_trap(_tf: &mut Self::TrapFrame) -> UserTrapCause {
        unimplemented!("aarch64 decode_user_trap — fill in with ESR_EL1 decode")
    }

    fn arm_timer() {
        unimplemented!("aarch64 arm_timer — program CNTV_TVAL_EL0 / CNTV_CTL_EL0")
    }

    fn handle_external_irq() {
        unimplemented!("aarch64 handle_external_irq — claim from GICC, dispatch, EOI")
    }

    unsafe fn init_kernel_trap_vec() {
        unimplemented!("aarch64 init_kernel_trap_vec — write VBAR_EL1 to kernel vector")
    }

    unsafe fn return_to_user(_tf: &mut Self::TrapFrame, _user_satp: usize) -> ! {
        unimplemented!("aarch64 return_to_user — switch TTBR0_EL1, set ELR_EL1/SPSR_EL1, eret")
    }

    unsafe fn init_console() {
        uart::init();
    }

    unsafe fn init_intc_global() {
        // GICv2 distributor setup lands with the boot follow-up.
    }

    unsafe fn init_intc_per_hart() {
        // GICv2 CPU-interface init.
    }

    unsafe fn install_free_frame(_free: unsafe fn(usize)) {
        // No-op for now — aarch64 pagetable populate isn't done yet,
        // so the page reaper isn't reached.
    }
}
