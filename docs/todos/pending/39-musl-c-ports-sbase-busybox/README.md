# 39: `musl` C toolchain & upstream Unix ports (`sbase`, `dash`/`ash`, `sqlite`)

**Status:** Pending — Phase 3 milestone (needs discovery, research & verification)
**Depends on:** `37-linux-musl-abi-and-auxv`
**Scope:** `xmake.lua`, `third_party/`, `Makefile`

## Problem statement

Currently C third-party ports (`bc`/`dc` and `lua`) build against `picolibc`, which is designed for bare-metal microcontrollers and lacks full POSIX headers (`<sys/socket.h>`, `<dirent.h>`, `<sys/wait.h>`, `<termios.h>`, `<poll.h>`, `<sys/ioctl.h>`, `<sys/mman.h>`). Switching the userspace C library to **`musl`** (or using `musl` alongside `picolibc`) provides a complete, standards-conformant POSIX C library so standard Unix C software compiles without custom header shims.

## Discovery & research checklist

1. **`musl` cross-build for `riscv64` and `aarch64` in `xmake.lua`:**
   - Verify building `musl` (e.g., tag `v1.2.5`) from source in `xmake.lua` using `riscv64-elf-gcc` (for `riscv64`) and `clang --target=aarch64-linux-musl` + rustup `llvm-ar`/`ld.lld` (for `aarch64`), producing `<install>/include`, `libc.a`, and `crt1.o`/`crti.o`/`crtn.o`.
   - Check how existing `bc`/`dc` and `lua` builds simplify when linked against `musl` instead of `picolibc` + `picolibc_stubs.c` + `bc_xv6rs.h`.
2. **Porting upstream Unix C toolkits:**
   - **Suckless [`sbase`](https://git.suckless.org/sbase/):** Audit `sbase` utilities (`grep`, `sed`, `tar`, `find`, `xargs`, `sort`, `uniq`, `wc`, `head`, `tail`, `cut`, `tr`, `ed`, `cmp`, `diff`) against `musl` and verify any missing syscalls (`utimensat`, `fchmodat`, `fchownat`).
   - **POSIX shell (`dash` or BusyBox `ash`):** Identify the minimal terminal/signal/wait syscalls needed (`wait4`, `setpgid`, `tcsetpgrp`, `sigaction`, `sigsuspend`, `dup2`/`fcntl(F_DUPFD_CLOEXEC)`) so real shell scripts (`if`/`for`/`case`, functions, subshells `$(...)`, exit statuses) work.
   - **Stretch C ports:** Single-file SQLite (`sqlite3` shell + `sqlite3.c` amalgamation) or TinyCC (`tcc`) / `chibicc` to verify file locking (`fcntl(F_SETLK)` stub), `mmap`, and stdio edge cases.

## Verification

- Build `musl` for both `riscv64` and `aarch64` via `xmake`.
- Run automated guest test scripts exercising `sbase` pipelines (`find / | sort | grep ... | wc -l`, `tar` create/extract, `sed` transforms) and shell scripts on both architectures.
