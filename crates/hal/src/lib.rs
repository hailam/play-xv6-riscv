#![no_std]

//! Hardware Abstraction Layer trait — the arch-independent surface the
//! kernel sees. Each supported architecture provides a single zero-sized
//! type that implements `Hal`.

/// Arch-neutral classification of a user-mode trap that just fired.
/// Each impl decodes its arch's cause register (scause on RISC-V,
/// ESR_EL1 on aarch64) into one of these.
#[derive(Debug, Clone, Copy)]
pub enum UserTrapCause {
    /// System call (RISC-V `ecall` from U-mode, aarch64 `svc`).
    Syscall,
    /// Timer interrupt — the Hal already re-armed the timer.
    Timer,
    /// External device interrupt — the Hal has *not* yet ack'd.
    Devintr,
    /// Load / store fault. `write = true` for store-side faults.
    PageFault { va: usize, write: bool },
    /// Anything else; kernel should kill the proc.
    Unknown { code: usize, va: usize },
}

/// Portable view of a saved trap frame. Lets the arch-independent
/// kernel read syscall args + tweak the return-to-user fields
/// without knowing the arch's actual struct layout.
///
/// Each Hal impl provides a concrete `TrapFrame` type (matched to
/// its trampoline asm) that implements this trait.
pub trait TrapFrameAccess: Default + Copy + 'static {
    fn epc(&self) -> u64;
    fn set_epc(&mut self, v: u64);

    fn sp(&self) -> u64;
    fn set_sp(&mut self, v: u64);

    // Syscall args / return value live in argument registers. RISC-V
    // uses a0..a7; aarch64 uses x0..x7 — same idea, same indexes.
    fn arg(&self, n: usize) -> u64;
    fn set_arg(&mut self, n: usize, v: u64);
    /// Number of the requested syscall (RISC-V: `a7`, aarch64: `x8`).
    fn syscall_nr(&self) -> u64;

    // Kernel-handover slots written by `return_to_user` before the
    // S/EL1 → U/EL0 jump so the trampoline knows where to land on
    // the next trap.
    fn set_kernel_satp(&mut self, v: u64);
    fn set_kernel_sp(&mut self, v: u64);
    fn set_kernel_trap(&mut self, v: u64);
    fn set_kernel_hartid(&mut self, v: u64);
}

pub trait Hal: 'static {
    type PageTable: PageTableOps;
    type TrapFrame: TrapFrameAccess;

    // ----- arch-specific constants -----
    //
    // Exposed here so kernel code can read them as
    // `<Arch as Hal>::PGSIZE` etc., avoiding direct
    // `use hal_riscv64::*` imports.
    const PGSIZE: usize;
    const KERNBASE: usize;
    const PHYSTOP: usize;
    const TRAMPOLINE: usize;
    const TRAPFRAME: usize;
    const TIMER_INTERVAL: u64;

    // MMIO. Names are arch-generic; the riscv64 impl maps INTC_BASE
    // to PLIC, the aarch64 impl maps it to the GICv2 distributor.
    const UART0: usize;
    const UART0_SIZE: usize;
    const VIRTIO0: usize;
    const VIRTIO0_SIZE: usize;
    const INTC_BASE: usize;
    const INTC_SIZE: usize;

    /// External-IRQ numbers the kernel dispatches on. The platform's
    /// IRQ controller delivers these IDs as a `u32` source through
    /// `kernel_on_external` after Hal::handle_external_irq has
    /// claimed them.
    const UART0_IRQ: usize;
    const VIRTIO0_IRQ: usize;

    // ----- arch helpers tied to constants -----
    fn trampoline_pa() -> usize;
    fn uservec_offset() -> usize;
    fn userret_offset() -> usize;

    fn hartid() -> usize;
    fn ncpus() -> usize;

    unsafe fn intr_off();
    unsafe fn intr_on();
    fn intr_get() -> bool;
    unsafe fn wfi();
    unsafe fn send_ipi(hart_mask: u64);

    fn console_putc(c: u8);

    /// Non-blocking console input — `None` if no byte available.
    fn console_try_getc() -> Option<u8>;

    fn now_ticks() -> u64;

    /// Install the given pagetable on this hart (set `satp` + flush TLB).
    /// Safety: the pagetable must remain live (i.e. its backing frames
    /// must not be freed) for as long as it is installed on any hart.
    unsafe fn install_pagetable(pt: &Self::PageTable);

    /// Encode a pagetable as the satp value to install.
    fn pagetable_satp(pt: &Self::PageTable) -> usize;

    /// Write the satp CSR directly. Useful when other harts want to
    /// install the same pagetable as hart 0 without owning the
    /// `PageTable` value themselves.
    unsafe fn write_satp(satp: usize);

    // ----- trap-handling surface -----
    //
    // Used by the kernel's `usertrap.rs` so it doesn't need to read
    // arch-specific CSRs (scause, ESR_EL1, etc.) directly.

    /// Called at the top of the kernel's user-trap dispatcher. The
    /// HAL gets a chance to redirect the trap-vector register back
    /// to the kernel-mode vector (RISC-V's `stvec`, etc.). aarch64's
    /// single VBAR_EL1 dispatches by entry slot, so this is a no-op
    /// there.
    unsafe fn on_user_trap_entry();

    /// Decode the most recent user-mode trap. On entry, the
    /// trapframe's saved register state is already in place; this
    /// reads the arch cause register, may advance `tf.epc` past
    /// the syscall instruction, and returns a portable cause.
    fn decode_user_trap(tf: &mut Self::TrapFrame) -> UserTrapCause;

    /// Re-arm the per-hart timer for the next tick. Called from
    /// the timer-interrupt arm of the user-trap dispatcher and from
    /// the kernel-timer hook.
    fn arm_timer();

    /// Dispatch an external (device) interrupt to the right driver
    /// (UART, virtio-blk, etc.). The HAL knows which IRQ controller
    /// to query (PLIC on RISC-V, GIC on aarch64).
    fn handle_external_irq();

    /// Install the kernel-mode trap vector. Called once per hart.
    unsafe fn init_kernel_trap_vec();

    /// Set up `tf` and arch CSRs for a return-to-user, then jump
    /// through the trampoline. Noreturn — control resumes in U-mode
    /// at `tf.epc()` with the given `user_satp`.
    unsafe fn return_to_user(tf: &mut Self::TrapFrame, user_satp: usize) -> !;

    // ----- platform init (hart 0 + per-hart, called from `kmain`) -----

    /// One-time global UART init (baud, FIFO, RX-IRQ enable on RISC-V).
    unsafe fn init_console();

    /// One-time global IRQ-controller init.
    unsafe fn init_intc_global();

    /// Per-hart IRQ-controller init (PLIC context / GIC CPU interface).
    unsafe fn init_intc_per_hart();

    /// Install the kernel's frame-free callback used by
    /// `Drop for PageTable` to reclaim user pages. Stored in a static
    /// inside the HAL so the pagetable destructor (which has no
    /// reference to KFRAMES) can find it.
    unsafe fn install_free_frame(free: unsafe fn(usize));
}

#[derive(Clone, Copy, Debug)]
pub struct PtePerm(pub u32);

impl PtePerm {
    pub const READ: u32 = 1 << 0;
    pub const WRITE: u32 = 1 << 1;
    pub const EXEC: u32 = 1 << 2;
    pub const USER: u32 = 1 << 3;

    pub const R: Self = Self(Self::READ);
    pub const RW: Self = Self(Self::READ | Self::WRITE);
    pub const RX: Self = Self(Self::READ | Self::EXEC);
    pub const RWX: Self = Self(Self::READ | Self::WRITE | Self::EXEC);
    pub const UR: Self = Self(Self::READ | Self::USER);
    pub const URW: Self = Self(Self::READ | Self::WRITE | Self::USER);
    pub const URX: Self = Self(Self::READ | Self::EXEC | Self::USER);
    pub const URWX: Self = Self(Self::READ | Self::WRITE | Self::EXEC | Self::USER);
}

#[derive(Clone, Copy, Debug)]
pub enum VmError {
    Oom,
    Remap,
    Misaligned,
    OutOfRange,
}

pub trait PageTableOps: Sized {
    fn new(alloc_frame: &dyn FrameAllocator) -> Result<Self, VmError>;

    /// Map `va..va+size` to `pa..pa+size` with the given perms.
    /// `va`, `pa`, `size` must each be page-aligned.
    fn map(
        &mut self,
        va: usize,
        pa: usize,
        size: usize,
        perm: PtePerm,
        alloc: &dyn FrameAllocator,
    ) -> Result<(), VmError>;

    /// Translate a VA to (PA, perm), or None if unmapped.
    fn translate(&self, va: usize) -> Option<(usize, PtePerm)>;

    /// Remove the leaf PTE at `va`. Returns the previously-mapped PA
    /// (the caller decides whether to free it), or `None` if the VA
    /// wasn't mapped to a leaf. Intermediate tables are left in
    /// place — `Drop for PageTable` reclaims them on the next
    /// teardown.
    fn unmap_page(&mut self, va: usize) -> Option<usize>;
}

/// Trait for allocating page-table frames. The kernel owns the allocator;
/// the HAL borrows it through this trait so page-table machinery can stay
/// allocator-agnostic.
pub trait FrameAllocator {
    /// Allocate a 4 KiB physical frame, zeroed. Returns its physical
    /// address, or `None` on OOM.
    fn alloc_zeroed(&self) -> Option<usize>;

    /// Free a 4 KiB physical frame.
    /// Safety: `pa` must have been returned by a previous `alloc_zeroed`
    /// and must not still be referenced.
    unsafe fn free(&self, pa: usize);
}
