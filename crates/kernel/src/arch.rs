//! Arch selection — the only place in `kernel/` that may `cfg` on arch.

pub use hal::Hal;

#[cfg(target_arch = "riscv64")]
pub use hal_riscv64::{Riscv64 as Arch, MAX_CPUS};
