//! Physical memory layout for QEMU `-machine virt` (riscv64).

pub const PGSIZE: usize = 4096;
pub const PGSHIFT: usize = 12;

pub const KERNBASE: usize = 0x8000_0000;
pub const PHYSTOP: usize = KERNBASE + 128 * 1024 * 1024;
pub const MAXVA: usize = 1usize << 38; // Sv39

pub const UART0: usize = 0x1000_0000;
pub const UART0_SIZE: usize = PGSIZE;
pub const UART0_IRQ: usize = 10;

pub const VIRTIO0: usize = 0x1000_1000;
pub const VIRTIO0_SIZE: usize = PGSIZE;
pub const VIRTIO0_IRQ: usize = 1;

pub const PLIC: usize = 0x0c00_0000;
pub const PLIC_SIZE: usize = 0x40_0000;

pub const CLINT: usize = 0x0200_0000;
pub const CLINT_MTIMECMP_BASE: usize = CLINT + 0x4000;
pub const CLINT_MTIME: usize = CLINT + 0xbff8;

pub const TRAMPOLINE: usize = MAXVA - PGSIZE;
pub const TRAPFRAME: usize = TRAMPOLINE - PGSIZE;

#[inline]
pub fn kstack(p: usize) -> usize {
    TRAMPOLINE - (p + 1) * 2 * PGSIZE
}
