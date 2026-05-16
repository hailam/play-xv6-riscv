//! Embedded user binary. Only `initcode` is baked in — every other
//! user program now lives on disk (loaded via `sys_exec` → `fs::namei`
//! → `readi`). `initcode` is the bootstrap that calls
//! `exec("/sh", ...)`.

pub const INITCODE: &[u8] = include_bytes!(env!("INITCODE_BIN_PATH"));
