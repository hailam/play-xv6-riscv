# 31: Split monolithic `syscall.rs` into domain submodules

**Status:** Pending — needs discovery & verification
**Scope:** `crates/kernel/src/syscall.rs` (~4,300 LoC / 146 KB)

## Problem statement

All ~68 syscall implementations, user-memory copy/translation helpers (`copyin`, `copyout`, `copyinstr`, `translate_user_perm`), file-descriptor table helpers, and the top-level syscall dispatcher currently live in a single ~4,300-line file (`crates/kernel/src/syscall.rs`). Navigating, reviewing, and maintaining unrelated subsystems (e.g., `AF_UNIX`/`smoltcp` socket calls vs. VFS path operations vs. signal delivery) in one file is increasingly unwieldy.

## Discovery & research checklist

1. **Inventory functions and shared internal helpers in `syscall.rs`:**
   - Catalog every `async fn sys_*` / `fn sys_*` and group by subsystem:
     - `uaccess`: `translate_user_perm`, `copyin`, `copyout`, `copyinstr`, user slice/struct readers and writers.
     - `proc`: `fork`, `exit`, `wait`, `waitpid`, `wait4`, `kill`, `getpid`, `getppid`, `getuid`/`setuid`/`getgid`/`setgid`/`umask`, `exec`, `execve`.
     - `mm`: `sbrk`, `brk`, `mmap`, `munmap`, `handle_page_fault`.
     - `fs`: `open`, `close`, `read`, `write`, `lseek`, `pread`, `pwrite`, `dup`, `dup2`, `fcntl`, `pipe`, `mkdir`, `rmdir`, `unlink`, `link`, `symlink`, `readlink`, `chdir`, `getcwd`, `rename`, `chmod`, `chown`, `stat`, `fstat`, `lstat`, `getdents`, `ftruncate`, `truncate`.
     - `signal`: `sigaction`, `sigprocmask`, `sigreturn`, `alarm`, `pause`.
     - `time`: `sleep`, `nanosleep`, `uptime`, `gettimeofday`, `clock_gettime`.
     - `io`: `ioctl`, `poll` (`PollWait` future).
     - `net`: `socket`, `bind`, `listen`, `accept`, `connect`, `sendto`, `recvfrom` (`AF_UNIX` and `AF_INET`).
   - Identify cross-cutting helpers (e.g., fd allocation/lookup, `create_at_path`, `ERR_*` constants) and determine where they belong so submodules don't require circular `pub(super)` imports.
2. **Verify external call sites:**
   - Check what symbols outside `syscall` import from `crate::syscall::*` (e.g., `proc.rs` calling `syscall::dispatch`, `syscall::handle_page_fault`, `syscall::sys_exit`) and re-export them from `syscall/mod.rs` to keep the rest of the kernel untouched.

## Verification

- Pure structural refactoring with no functional changes.
- Verify `cargo build --release` on both `riscv64gc-unknown-none-elf` and `aarch64-unknown-none-softfloat` with zero warnings.
- Run the full 74-test suite (`usertests` + standalone tests) and `make test-net`, `make test-fb`, `make test-input`, `make test-gui`.
