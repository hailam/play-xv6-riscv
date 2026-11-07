# 37: Linux `asm-generic` (`*-unknown-linux-musl`) syscall ABI, `auxv`, and TLS bootstrap

**Status:** Pending — Phase 3 keystone (needs discovery, research & verification)
**Depends on:** `34-pseudo-devices-and-tty`, `35-fs-long-names-and-capacity`, `36-cow-fork`
**Scope:** `crates/kernel/src/uapi.rs`, `crates/kernel/src/syscall/`, `crates/kernel/src/user_vm.rs`, `crates/kernel/src/elf.rs`, `crates/kernel/src/proc.rs`

## Problem statement

Both `riscv64` and `aarch64` use the unified Linux `include/uapi/asm-generic/unistd.h` syscall table and the same register convention (`a7`/`x8` for syscall number, `a0..a5`/`x0..x5` for arguments). Supporting the `asm-generic` Linux syscall numbers, struct layouts (`kstat`, `iovec`, `linux_dirent64`, `utsname`), ELF `PT_TLS`, and initial stack Auxiliary Vector (`auxv`) allows unmodified binaries compiled for `riscv64gc-unknown-linux-musl` and `aarch64-unknown-linux-musl` (both Rust `std` and C `musl`) to run directly on the kernel.

## Discovery & research checklist

1. **Coexistence with existing xv6 syscall ABI (`1..68`):**
   - Check syscall number overlap between our `SYS_*` (`1..68` in `crates/kernel/src/uapi.rs`) and Linux `asm-generic/unistd.h` (e.g., Linux `SYS_ioctl = 29`, `SYS_mkdirat = 34`, `SYS_unlinkat = 35`, `SYS_openat = 56`, `SYS_close = 57`, `SYS_getdents64 = 61`, `SYS_lseek = 62`, `SYS_read = 63`, `SYS_write = 64`).
   - Because numbers `17..68` overlap with our custom table, decide on the cleanest ABI discriminator on `Proc`:
     - **Option A:** Tag `Proc::abi` (`Abi::Xv6` vs `Abi::Linux`) at `execve` time based on ELF header (`e_ident[EI_OSABI]`, presence of `PT_TLS`, or a custom ELF section/note in our legacy `ulib` linker script).
     - **Option B:** Migrate in-tree `ulib.S` / `ulib-aarch64.S` and `uapi.rs` directly to the Linux `asm-generic` syscall numbers so the kernel has a single unified syscall table.
2. **ELF loader & stack layout for `musl` / Rust `std` (`elf.rs`, `user_vm.rs`):**
   - Inspect `crates/kernel/src/elf.rs` and `crates/kernel/src/user_vm.rs`:
     - Expose loaded ELF header info (`phdr_va`, `phent`, `phnum`, `entry`) so `execve` can populate `auxv`: `AT_PHDR (3)`, `AT_PHENT (4)`, `AT_PHNUM (5)`, `AT_PAGESZ (6)`, `AT_ENTRY (9)`, `AT_UID (11)`, `AT_EUID (12)`, `AT_GID (13)`, `AT_EGID (14)`, `AT_SECURE (23)`, `AT_RANDOM (25)`, `AT_NULL (0)`.
     - Verify exact initial stack layout expected by `musl`'s `_start`: `sp` 16-byte aligned, `[argc, argv[0..argc], 0, envp[0..envc], 0, auxv[0..2*k], AT_NULL, 0]`, with `sp` passed on stack (and `a0 = sp` if needed).
     - Check whether `PT_TLS` program headers need kernel-side mapping or if `musl`'s `__init_tls` allocates and copies the TLS image itself using `AT_PHDR`/`AT_PHNUM`.
     - Verify `set_tid_address` (`96`), `set_robust_list` (`99`), and `prlimit64` (`261`) minimal semantics required during `musl` + Rust `std` startup.
3. **Linux `asm-generic` syscall surface required for `musl` + Rust `std`:**
   - **File `*at` family:** `getcwd (17)`, `dup (23)`, `dup3 (24)`, `fcntl (25)`, `ioctl (29)`, `mkdirat (34)`, `unlinkat (35)`, `symlinkat (36)`, `linkat (37)`, `renameat (38)` / `renameat2 (276)`, `ftruncate (46)`, `faccessat (48)`, `chdir (49)`, `fchmod (52)`, `fchmodat (53)`, `fchownat (54)`, `fchown (55)`, `openat (56)`, `close (57)`, `pipe2 (59)`, `getdents64 (61)`, `lseek (62)`, `read (63)`, `write (64)`, `readv (65)`, `writev (66)`, `pread64 (67)`, `pwrite64 (68)`, `pselect6 (72)`, `ppoll (73)`, `readlinkat (78)`, `newfstatat (79)`, `fstat (80)`, `fsync (82)`.
   - **Process & signals:** `exit (93)`, `exit_group (94)`, `set_tid_address (96)`, `futex (98)`, `nanosleep (101)`, `clock_gettime (113)`, `clock_nanosleep (115)`, `sched_yield (124)`, `kill (129)`, `tkill (130)`, `tgkill (131)`, `sigaltstack (132)`, `rt_sigaction (134)`, `rt_sigprocmask (135)`, `rt_sigreturn (139)`, `setpgid (154)`, `getpgid (155)`, `uname (160)`, `gettimeofday (169)`, `getpid (172)`, `getppid (173)`, `getuid (174)`, `geteuid (175)`, `getgid (176)`, `getegid (177)`, `sysinfo (179)`, `brk (214)`, `munmap (215)`, `mremap (216)`, `clone (220)`, `execve (221)`, `mmap (222)`, `mprotect (226)`, `madvise (233)`, `wait4 (260)`, `prlimit64 (261)`, `getrandom (278)`.
   - **Sockets:** `socket (198)`, `socketpair (199)`, `bind (200)`, `listen (201)`, `accept (202)` / `accept4 (242)`, `connect (203)`, `getsockname (204)`, `getpeername (205)`, `sendto (206)`, `recvfrom (207)`, `setsockopt (208)`, `getsockopt (209)`, `shutdown (210)`.

## Verification

- Compile a minimal static C `musl` binary and a static Rust `std` binary (`fn main() { println!("hello from rust std!"); }`) for `riscv64gc-unknown-linux-musl` and `aarch64-unknown-linux-musl` and verify both boot and run cleanly in QEMU.
- Ensure all 74 existing tests continue to pass on both architectures.
