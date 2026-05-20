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
mod gic;
pub mod memlayout;
mod pagetable;
mod start;
mod trap;
pub mod trapframe;
pub mod uart;

core::arch::global_asm!(include_str!("../asm/kernelvec.S"));

pub use pagetable::{install_free_frame, PageTable};
pub use trapframe::TrapFrame;

extern "C" {
    fn _trampoline();
}

/// Physical address of the trampoline page (the kernel-resident page
/// that hosts uservec/userret; mapped at fixed VA TRAMPOLINE in both
/// kernel and user pagetables). Defined by `kernel-aarch64.ld`.
#[inline]
pub fn trampoline_pa() -> usize {
    _trampoline as *const () as usize
}

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
    const INTC_SIZE: usize = memlayout::INTC_RANGE_SIZE;
    const UART0_IRQ: usize = memlayout::UART0_IRQ;
    const VIRTIO0_IRQ: usize = memlayout::VIRTIO0_IRQ;
    /// Skeleton placeholder — real value comes from the ARM generic
    /// timer counter frequency once the boot path is wired.
    const TIMER_INTERVAL: u64 = 1_000_000;

    fn trampoline_pa() -> usize {
        trampoline_pa()
    }
    fn uservec_offset() -> usize {
        0 // Phase E will fill in.
    }
    fn userret_offset() -> usize {
        0 // Phase E will fill in.
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
        // First-time MMU enable. Subsequent calls (e.g., per-hart
        // bring-up, or pagetable swaps) just re-write TTBR0_EL1
        // and flush. The MMU-on bit in SCTLR_EL1 is idempotent.
        unsafe { Self::write_satp(pagetable::ttbr0_value(pt)) };
    }

    fn pagetable_satp(pt: &PageTable) -> usize {
        pagetable::ttbr0_value(pt)
    }

    /// Trait method is named `write_satp` for parity with riscv. On
    /// aarch64 this writes TTBR0_EL1 plus, on first call, brings the
    /// MMU online by setting MAIR_EL1/TCR_EL1/SCTLR_EL1.
    unsafe fn write_satp(satp: usize) {
        unsafe {
            // Make sure MAIR and TCR are set before TTBR0 — writes
            // to them are harmless if already set (idempotent).
            csr::write_mair_el1(pagetable::MAIR_EL1_VAL);
            csr::write_tcr_el1(pagetable::TCR_EL1_VAL);
            csr::write_ttbr0_el1(satp as u64);
            // Invalidate stale TLB entries from before the new
            // mapping, then make the write globally visible.
            csr::dsb_ish();
            csr::tlbi_vmalle1is();
            csr::dsb_ish();
            csr::isb();
            // Enable MMU + caches if not already on. SCTLR_EL1.M = bit 0,
            // C = bit 2, I = bit 12. Writing already-set bits is fine.
            let mut sctlr = csr::read_sctlr_el1();
            sctlr |= (1 << 0) | (1 << 2) | (1 << 12);
            csr::write_sctlr_el1(sctlr);
            csr::isb();
        }
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
        // Phase E will fill this in (read ESR_EL1, FAR_EL1, ELR_EL1).
        unimplemented!("aarch64 decode_user_trap — Phase E")
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

    unsafe fn return_to_user(_tf: &mut Self::TrapFrame, _user_satp: usize) -> ! {
        unimplemented!("aarch64 return_to_user — Phase E")
    }

    unsafe fn init_console() {
        uart::init();
    }

    unsafe fn init_intc_global() {
        unsafe { trap::init_intc_global() };
    }

    unsafe fn init_intc_per_hart() {
        unsafe { trap::init_intc_per_hart() };
    }

    unsafe fn install_free_frame(free: unsafe fn(usize)) {
        pagetable::install_free_frame(free);
    }
}
