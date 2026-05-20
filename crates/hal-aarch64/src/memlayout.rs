//! Physical memory layout for QEMU `-machine virt` (aarch64).
//!
//! Mirrors `hal-riscv64::memlayout` shape so the kernel's
//! arch-independent code can read the same constants by name.

pub const PGSIZE: usize = 4096;
pub const PGSHIFT: usize = 12;

/// QEMU virt aarch64 loads the kernel image at 0x4008_0000 by default
/// (1 MiB above the start of DRAM at 0x4000_0000). We use the bottom
/// of DRAM as the kernel base.
pub const KERNBASE: usize = 0x4000_0000;
pub const PHYSTOP: usize = KERNBASE + 128 * 1024 * 1024;

/// 48-bit VA space (ARMv8 4-level, 4K granule).
pub const MAXVA: usize = 1usize << 47; // user half is [0, 1<<47)

/// PL011 UART (the QEMU virt aarch64 console).
pub const UART0: usize = 0x0900_0000;
pub const UART0_SIZE: usize = PGSIZE;
pub const UART0_IRQ: usize = 33; // GIC SPI 1 (== PPI/SPI base 32 + 1)

/// GIC v2 distributor + CPU interface. The kernel maps both via the
/// single `INTC_BASE..INTC_BASE+INTC_SIZE` range, so `INTC_SIZE` is
/// large enough to cover both regions back-to-back.
pub const GICD: usize = 0x0800_0000;
pub const GICD_SIZE: usize = 0x10000;
pub const GICC: usize = 0x0801_0000;
pub const GICC_SIZE: usize = 0x10000;
/// `GICD + GICC` covered as one contiguous 128 KiB region — see
/// `Hal::INTC_SIZE` consumer in `crate::kernel::vm`.
pub const INTC_RANGE_SIZE: usize = 0x20000;

/// virtio-mmio (first slot — QEMU virt has many; we use the same
/// device the riscv64 path uses).
pub const VIRTIO0: usize = 0x0a00_0000;
pub const VIRTIO0_SIZE: usize = PGSIZE;
pub const VIRTIO0_IRQ: usize = 48;

/// Same trampoline / trapframe arrangement as riscv64: top of user VA
/// space, two pages reserved.
pub const TRAMPOLINE: usize = MAXVA - PGSIZE;
pub const TRAPFRAME: usize = TRAMPOLINE - PGSIZE;

#[inline]
pub fn kstack(p: usize) -> usize {
    TRAMPOLINE - (p + 1) * 2 * PGSIZE
}
