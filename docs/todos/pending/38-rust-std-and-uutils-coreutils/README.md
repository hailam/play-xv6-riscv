# 38: Rust `std` userspace & `uutils/coreutils` bring-up

**Status:** Pending — Phase 3 milestone (needs discovery, research & verification)
**Depends on:** `37-linux-musl-abi-and-auxv`
**Scope:** `Cargo.toml`, `crates/kernel/src/syscall/`, `xmake.lua` / `Makefile`

## Problem statement

Once the kernel implements the `*-unknown-linux-musl` syscall and ELF startup ABI (`37-linux-musl-abi-and-auxv`), userspace programs can be written in standard Rust using `std` (`riscv64gc-unknown-linux-musl` and `aarch64-unknown-linux-musl`), and off-the-shelf Rust crates such as [`uutils/coreutils`](https://github.com/uutils/coreutils) can be compiled and packed into `fs.img`.

## Discovery & research checklist

1. **Audit Rust `std` runtime initialization and syscalls:**
   - Trace what syscalls Rust `std`'s pre-`main` init (`library/std/src/sys/pal/unix/mod.rs`) invokes on `*-unknown-linux-musl`:
     - `poll` on fds `0, 1, 2` to ensure standard descriptors are open (`sanitize_standard_fds`).
     - `rt_sigaction(SIGPIPE, SIG_IGN)` and `sigaltstack` setup for stack-overflow handler (`mprotect` guard page).
     - `getrandom` (syscall 278) or `/dev/urandom` fallback for `HashMap` seed generation.
     - `sched_getaffinity` / `/proc/self/maps` or `prlimit64` probes (verify which can return `ENOSYS`/`ENOENT` gracefully).
2. **Test `std` modules systematically:**
   - Write an in-tree `ruststdtest` binary exercising `std::fs` (`File`, `read_dir`, `metadata`, `create_dir_all`, `remove_file`), `std::env` (`args`, `vars`, `current_dir`), `std::process::Command` (`spawn`, `status`, `output` with pipes), `std::net` (`TcpListener`, `TcpStream`), and `std::time::Instant`.
3. **Evaluate `uutils/coreutils` multicall binary:**
   - Test building `uutils/coreutils` (`--no-default-features --features "..."`) for `riscv64gc-unknown-linux-musl` and `aarch64-unknown-linux-musl` with `opt-level = "z"`, `lto = true`, `strip = true`, `panic = "abort"`.
   - Measure stripped binary size and select the core utility set (`cat`, `cp`, `cut`, `date`, `dirname`, `echo`, `env`, `expr`, `false`, `head`, `ln`, `ls`, `mkdir`, `mv`, `printf`, `pwd`, `rm`, `rmdir`, `seq`, `sleep`, `sort`, `tail`, `tee`, `test`, `touch`, `tr`, `true`, `uname`, `uniq`, `wc`, `yes`), linking each `/bin/<tool>` via hardlink or symlink to the single multicall binary.

## Verification

- `ruststdtest` passes on both `riscv64` (`-smp 3`) and `aarch64` (`-smp 4`).
- `uutils` coreutils commands run interactively and in pipelines inside the guest on both architectures.
