//! Entry path for aarch64. Includes the entry asm.
//!
//! See `../asm/entry.S` for the actual code — it handles the EL2→EL1
//! drop, sets up per-hart kernel stack from MPIDR_EL1.Aff0, then
//! branches to Rust `kmain`.

core::arch::global_asm!(include_str!("../asm/entry.S"));
