# 16: POSIX-ish compatibility

**Status:** Tiers 1–5 + 8 **done** (kernel-side syscall surface
for newlib/musl bring-up is in place); Tier 6 (signals/concurrency
bits beyond what landed) and Tier 7 (sockets) remain.
**Estimated:** Originally "many sessions"; ~60 sub-features
across 30+ syscalls landed in chunks over the past few weeks.

## Done summary

The kernel exposes **62 POSIX syscalls** across the libc-glue
surface. A typical newlib/musl OS-glue layer can be wired against
SYS_* directly — see the test programs under
`crates/kernel/user/` for the patterns:

- File I/O — `open/close/read/write/lseek/pread/pwrite/dup/dup2/`
  `stat/fstat/lstat/chdir/getcwd/rename/unlink/link/symlink/`
  `readlink/mkdir/rmdir/mknod/chmod/chown/umask/ftruncate/truncate/`
  `getdents/fcntl(F_GETFD/SETFD/DUPFD/GETFL/SETFL)/ioctl(TCGETS/`
  `TIOCGWINSZ/FIONREAD)`
- Open-flag suite — `O_RDONLY/WRONLY/RDWR/CREATE/TRUNC/APPEND/`
  `CLOEXEC/NONBLOCK`
- Process — `fork/exec/execve/wait/waitpid/wait4/exit/sbrk/brk/`
  `sleep/nanosleep/uptime/clock_gettime/gettimeofday/getpid/`
  `getppid/kill/pause/alarm/sigaction/sigprocmask/sigreturn`
- Credentials — `getuid/geteuid/getgid/getegid/setuid/setgid/umask`
- Memory — `mmap` (anonymous **and** file-backed `MAP_PRIVATE`)
  with lazy page-fault loading; `munmap`
- I/O multiplex — `poll` (cooperative 10ms polling loop;
  multi-waker `select` is a follow-on if perf matters)
- Pipes — `pipe`

Verified end-to-end on both **riscv64** and **aarch64** under
QEMU virt machines. ~25 user-space test programs exercise the
surface; all pass identically on both arches.

## Why

xv6 compat is the *minimum* bar for "runs unmodified xv6 binaries."
POSIX is the bigger ask: "compiles and runs a non-trivial subset
of mainline Unix software." The original plan called this out as
the long-horizon goal but never scoped it.

Full POSIX is a moving target (1000+ syscalls, libc, threads,
signals, IPC, sockets). We'll never be a POSIX-compliant OS. The
realistic goal is a useful subset — enough that a port of
`busybox` or similar runs, and a port of `newlib` or `musl`
provides the C-standard surface around what we expose.

## Scope brackets

This todo isn't doing all of this. It's the *index* of what
POSIX-ish work would look like, broken into sub-todos that can
be picked off independently.

### Tier 1 — file API parity — **DONE**

Landed:
fcntl (F_GETFD/F_SETFD/F_DUPFD/F_DUPFD_CLOEXEC/F_GETFL/F_SETFL),
O_APPEND/O_NONBLOCK/O_CLOEXEC, chmod, chown, umask, per-proc
uid/gid + open-permission enforcement, stat/lstat/fstat,
lseek/pread/pwrite, Stat struct extended to 48 bytes
(mode + uid + gid + atime + mtime + ctime), ftruncate/truncate.

### Tier 2 — signals — **DONE**

Landed: sigaction (with restorer stub in ulib), sigprocmask
(SIG_BLOCK/UNBLOCK/SETMASK), sigreturn (snapshots trapframe,
restores blocked mask), pending-bit dispatch in usertrap's
return-to-user, handler-blocks-self semantics (sa_mask | sig
ORed into blocked during handler). SIGKILL/SIGSTOP uncatchable.
alarm + SIGALRM via the timer wheel. pause() blocks until
deliverable signal. kill(pid, sig) replaces the old 1-arg kill.

### Tier 3 — environment + argv — **DONE**

Landed: execve(path, argv, envp). User stack layout extended
with envp[] array + strings. ulib's _start stores envp from
x2/a2 into the global `environ`. ulib.c provides
getenv/setenv/unsetenv (sbrk-backed pool for new strings).

### Tier 4 — directory iteration — **DONE**

Landed: getdents(fd, buf, len) returns a packed UserDirent
record stream (ino/reclen/namelen/name). userspace can wrap
into opendir/readdir/closedir at will.

### Tier 5 — process info — **DONE**

Landed: getppid (via Proc.parent: Weak<Proc>), getuid/geteuid/
getgid/getegid/setuid/setgid (collapsed real/effective model —
no setuid binaries means no separate saved-set ID), umask,
getcwd (walks ..-chain from proc.cwd using a new
dirlookup_by_inum helper), gettimeofday, clock_gettime
(CLOCK_MONOTONIC), nanosleep.

### Tier 6 — IPC + concurrency — **partially done**

Landed:
- waitpid(pid, &status, options) with WNOHANG
- wait4(pid, status, options, rusage) — rusage is zeroed (we
  don't track resource accounting)
- poll(fds, nfds, timeout_ms) — cooperative 10ms-poll loop

Not landed (deferred):
- Unix-domain sockets — needs AF_UNIX socket type; not started
- pthread_* — fundamentally at odds with our async-single-
  task-per-proc model; recommended to skip rather than build

### Tier 7 — sockets — **not started**

TCP/IP stack + AF_INET. Likely smoltcp behind the HAL. A
separate project — not gated by anything we have, but big.

### Tier 8 — POSIX-ish libc — **kernel side done; libc port outstanding**

The minimal glue set listed below is complete in the kernel.
Actual newlib/musl bring-up is the outstanding work: pull in
the library, point its syscall stubs at our SYS_* numbers,
get a `hello world` and a small toolkit compiling against it.

The minimal glue set in our SYS_* (all 62 implemented):
open, close, read, write, lseek, pread, pwrite, stat, fstat,
lstat, mmap, munmap, brk, sbrk, fork, exec, execve, wait,
waitpid, wait4, kill, sigaction, sigprocmask, sigreturn,
alarm, pause, getpid, getppid, gettimeofday, clock_gettime,
nanosleep, ioctl (TCGETS/TIOCGWINSZ/FIONREAD), fcntl
(F_*FD + F_*FL + F_DUPFD*), pipe, dup, dup2, mkdir, rmdir,
unlink, link, symlink, readlink, chdir, getcwd, rename,
chmod, chown, getuid, geteuid, getgid, getegid, setuid,
setgid, umask, getdents, ftruncate, truncate, sleep, uptime,
poll.

## Architecture implications

- **Async-first kernel maps cleanly to POSIX async** (signals,
  poll, async I/O — they all are "register a waker, return
  pending"). No `swtch.S` means no preemption complications.
- **Trait-widening done in [[14-aarch64-completion]] preparation**
  means new POSIX surface added once auto-supports both arches.
- **`fcntl` overlap with `Hal::TrapPlumbing`** isn't a thing —
  fcntl is purely fs-layer.

## Verification

For each sub-todo, the gate is "real off-the-shelf program X
runs." Examples:

- After Tier 1: `coreutils-busybox-ish ls -la` works.
- After Tier 2: shell can interrupt a `sleep` with Ctrl-C.
- After Tier 5: `pwd` works.
- After Tier 8: `make` (a small ported build) runs.

## Recommended order

The order in the tiers above. The first 3 tiers (files +
signals + env) get us 80% of practical POSIX use. Sockets and
threads can be the last ~20% or never.

## Risks

- **Scope creep** — this is a multi-month roadmap; treat it as
  a backlog, not a single deliverable.
- **xv6 compat divergence** — POSIX `struct stat` differs from
  xv6's. Decide early: keep xv6 layout for `fstat`, add a
  separate `posix_stat` syscall? Or fork `struct stat` per
  feature flag? Probably the former.
- **Threading model.** POSIX assumes preemption; our cooperative
  async kernel doesn't. `pthread_create` may simply be
  unsupported; we'd document that and move on.
- **No external crates.** The original plan ruled out external
  crates for phase 1. POSIX work (esp. sockets) almost certainly
  needs `smoltcp` or similar. Revisit the "no external crates"
  rule at this point — likely time to lift it for phase 3.
