//! Arch selection — the only place in `kernel/` that may `cfg` on
//! arch (aside from the trapframe-field access in `proc.rs` /
//! `syscall.rs` etc., which still need concrete struct fields).

pub use hal::Hal;

#[cfg(target_arch = "riscv64")]
pub use hal_riscv64::{Riscv64 as Arch, MAX_CPUS};

#[cfg(target_arch = "aarch64")]
pub use hal_aarch64::{AArch64 as Arch, MAX_CPUS};

/// Re-exports of the `Hal`-trait constants under short names so kernel
/// code doesn't have to write `<Arch as Hal>::PGSIZE` everywhere.
pub const PGSIZE: usize = <Arch as Hal>::PGSIZE;
pub const PHYSTOP: usize = <Arch as Hal>::PHYSTOP;
pub const TRAMPOLINE: usize = <Arch as Hal>::TRAMPOLINE;
pub const TRAPFRAME: usize = <Arch as Hal>::TRAPFRAME;
pub const TIMER_INTERVAL: u64 = <Arch as Hal>::TIMER_INTERVAL;

#[inline]
pub fn trampoline_pa() -> usize {
    <Arch as Hal>::trampoline_pa()
}
#[inline]
pub fn uservec_offset() -> usize {
    <Arch as Hal>::uservec_offset()
}
#[inline]
pub fn userret_offset() -> usize {
    <Arch as Hal>::userret_offset()
}
