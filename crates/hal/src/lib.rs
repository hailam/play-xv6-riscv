#![no_std]

//! Hardware Abstraction Layer trait — the arch-independent surface the
//! kernel sees. Each supported architecture provides a single zero-sized
//! type that implements `Hal`.

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
