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
mod psci;
mod start;
mod trap;
pub mod trapframe;
pub mod uart;

core::arch::global_asm!(include_str!("../asm/kernelvec.S"));
core::arch::global_asm!(include_str!("../asm/trampoline.S"));

pub use pagetable::{install_free_frame, PageTable};
pub use trapframe::TrapFrame;

extern "C" {
    fn _trampoline();
    fn trampoline_vector_table();
    fn trampoline_userret();
}

/// Physical address of the trampoline page.
#[inline]
pub fn trampoline_pa() -> usize {
    _trampoline as *const () as usize
}

/// Offset of `userret` from the trampoline page base, used by
/// `Hal::return_to_user` to construct a callable VA.
#[inline]
fn userret_offset_internal() -> usize {
    trampoline_userret as *const () as usize
        - trampoline_vector_table as *const () as usize
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
        // VBAR_EL1 vector slot for "Lower EL using AArch64,
        // Synchronous" is at offset 0x400 within the table.
        0x400
    }
    fn userret_offset() -> usize {
        userret_offset_internal()
    }

    #[inline(always)]
    fn hartid() -> usize {
        csr::read_mpidr_el1()
    }

    fn ncpus() -> usize {
        MAX_CPUS
    }

    unsafe fn start_secondary_harts(ncpus: usize) {
        // QEMU virt parks secondary harts at boot — they need
        // PSCI CPU_ON to start. Walk 1..ncpus, each gets dispatched
        // to `_entry` so it reruns the normal MPIDR-based stack
        // setup and EL2→EL1 drop. We may walk past the actual
        // -smp count when MAX_CPUS > smp; PSCI returns
        // INVALID_PARAMETERS for those, which is benign.
        extern "C" {
            fn _entry();
        }
        let entry = _entry as usize;
        for hart in 1..ncpus {
            let mpidr = hart as u64;
            let _ = unsafe { psci::cpu_on(mpidr, entry, 0) };
        }
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
        // SGI #0 is our IPI INTID. Broadcasting to "all except self"
        // is a superset of the precise hart_mask the caller asked
        // for — fine because the recipients re-check their local
        // ready queue on receipt. Refining to a true mask requires
        // splitting the SGIR write per target bit; defer.
        unsafe { gic::sgi_all_except_self(0) };
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
        // Switch VBAR_EL1 back to the kernel-mode vector table
        // now that we're running in kernel context. (Trampoline
        // can't do this itself because that would require an
        // external-symbol reference inside .trampsec, and the
        // linker can't resolve a PC-relative reloc between the
        // trampoline page's runtime VA — high in user space —
        // and the kernel image VA.)
        unsafe {
            csr::write_vbar_el1(trap::vector_table_addr());
            csr::isb();
        }
    }

    fn decode_user_trap(_tf: &mut Self::TrapFrame) -> UserTrapCause {
        // The trampoline tagged this trap as sync (0) or IRQ (1).
        // IRQ entry path leaves ESR_EL1 in an UNKNOWN state per
        // ARM ARM, so we *must* dispatch on trap_kind, not ESR.
        if _tf.trap_kind == 1 {
            // IRQ from EL0. Defer the GIC-claim / dispatch to
            // Hal::handle_external_irq (the same path kernel-mode
            // IRQs use). Just signal Devintr; the executor's
            // usertrap loop will call handle_external_irq.
            return UserTrapCause::Devintr;
        }
        // ESR captured at trap time by trampoline (`tf.esr_el1`).
        // Reading the live CSR was racy: an IRQ during the
        // trap-dispatch window leaves ESR_EL1 UNKNOWN. FAR_EL1 is
        // overwritten only by faulting accesses, so reading it
        // from CSR is still fine for page-fault paths.
        let esr = _tf.esr_el1;
        let far = csr::read_far_el1();
        let ec = (esr >> 26) & 0x3F;
        match ec {
            // SVC from AArch64 — syscall. ELR_EL1 already points
            // PAST the SVC instruction (ARM ARM D1.10.5), so the
            // trapframe's elr_el1 (= epc) is already correct.
            0x15 => UserTrapCause::Syscall,
            // Instruction abort from EL0.
            0x20 => UserTrapCause::PageFault {
                va: far as usize,
                write: false,
            },
            // Data abort from EL0. ISS[6] = WnR (1 = write).
            0x24 => UserTrapCause::PageFault {
                va: far as usize,
                write: (esr & (1 << 6)) != 0,
            },
            _ => UserTrapCause::Unknown {
                code: esr as usize,
                va: far as usize,
            },
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

    unsafe fn return_to_user(_tf: &mut Self::TrapFrame, user_satp: usize) -> ! {
        // The kernel filled in tf.kernel_{satp,sp,trap,hartid} and
        // tf.{elr_el1,spsr_el1,sp_el0,x[..]} before calling us. Jump
        // into the trampoline at userret_offset with:
        //   x0 = TRAPFRAME VA (the fixed user VA; mapped in both PTs)
        //   x1 = user_satp (TTBR0 value for the user pagetable)
        let userret_va = memlayout::TRAMPOLINE + userret_offset_internal();
        let userret_fn: extern "C" fn(usize, usize) -> ! =
            unsafe { core::mem::transmute(userret_va) };
        userret_fn(memlayout::TRAPFRAME, user_satp);
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

