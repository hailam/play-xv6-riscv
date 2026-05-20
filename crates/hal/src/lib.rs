#![no_std]

//! Hardware Abstraction Layer trait — the arch-independent surface the
//! kernel sees. Each supported architecture provides a single zero-sized
//! type that implements `Hal`.

pub trait Hal: 'static {
    type PageTable: PageTableOps;

    // ----- arch-specific constants -----
    //
    // Exposed here so kernel code can read them as
    // `<Arch as Hal>::PGSIZE` etc., avoiding direct
    // `use hal_riscv64::*` imports.
    const PGSIZE: usize;
    const PHYSTOP: usize;
    const TRAMPOLINE: usize;
    const TRAPFRAME: usize;
    const TIMER_INTERVAL: u64;

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
