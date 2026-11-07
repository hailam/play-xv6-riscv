# 34: Standard Unix pseudo-devices (`/dev/{null,zero,urandom,tty}`) & process groups

**Status:** Pending — Phase 3 foundation (needs discovery, research & verification)
**Scope:** `crates/kernel/src/file.rs`, `crates/kernel/src/syscall.rs`, `crates/kernel/src/proc.rs`, `crates/kernel/src/uapi.rs`, `crates/kernel/user/init.c`

## Problem statement

Open-source Unix tools, shells (`dash`, `ash`), and `uutils/coreutils` routinely open `/dev/null`, `/dev/zero`, `/dev/urandom`, and `/dev/tty`, query `isatty(fd)` via `ioctl(fd, TCGETS, ...)`, and manage foreground/background job control via process groups (`setpgid`, `getpgid`, `TIOCGPGRP`, `TIOCSPGRP`). Currently only `/dev/fb0` (major 1) and `/dev/input/0` (major 2) exist as device nodes, and `picolibc_stubs.c` hardcodes `isatty() -> 0`.

## Discovery & research checklist

1. **Audit current `T_DEVICE` major/minor dispatch:**
   - Inspect `crates/kernel/src/file.rs` and `crates/kernel/src/syscall.rs` (`sys_open`, `sys_mknod`, `File::Device`) to see how device majors are registered and dispatched for `read`, `write`, `lseek`, `mmap`, `ioctl`, and `poll`.
   - Check how `crates/kernel/user/init.c` (or `mkfs`) creates `/dev/fb0` and `/dev/input/0` at boot.
2. **Design pseudo-device majors/minors:**
   - `/dev/null`: `read` returns `0` (EOF); `write` discards and returns `len`; `poll` reports `POLLIN | POLLOUT`.
   - `/dev/zero`: `read` zeroes user buffer and returns `len`; `write` discards and returns `len`; `mmap` maps anonymous zeroed pages.
   - `/dev/urandom` (and `/dev/random`): `read` fills user buffer from a fast kernel PRNG seeded from `Arch::now_ticks()` (`rdtime` / `cntvct_el0`) + hart/interrupt jitter (needed by `musl` `AT_RANDOM` and Rust `HashMap` `RandomState`).
   - `/dev/tty` / `/dev/console`: routes to console read/write + `TCGETS`/`TCSETS`/`TIOCGWINSZ`/`TIOCGPGRP`/`TIOCSPGRP` `ioctl`s.
3. **Process group & session state on `Proc`:**
   - Check what minimum `pgid` / `sid` fields on `Proc` (and foreground `pgid` in `console_in.rs`) are needed so interactive shells and Ctrl-C (`SIGINT` to foreground process group) work cleanly.

## Verification

- Remove the `isatty() -> 0` stub in `third_party/libc-glue/picolibc_stubs.c` and route `isatty` through `ioctl(fd, TCGETS, ...)`.
- Add regression tests verifying `/dev/null` (EOF on read, discard on write), `/dev/zero` (zero-fill read), `/dev/urandom` (non-zero entropy), and `isatty` (`1` on console/tty, `0` on regular file/pipe).
