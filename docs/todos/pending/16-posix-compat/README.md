# 16: POSIX-ish compatibility

**Status:** Pending — **after [[15-xv6-compat]] and [[14-aarch64-completion]]**
**Estimated:** Many sessions across multiple sub-todos; this is a
sustained track, not a single feature.
**Depends on:** xv6 compat lands first (gives us a real test
harness via `usertests`); aarch64 lands second (proves the trait
surface holds when we add new abstractions). POSIX work spans
both arches once they're in.

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

### Tier 1 — file API parity  (~3 sub-todos)

Closest to what xv6 already has:

- **fcntl + extended O_ flags** — `O_APPEND`, `O_NONBLOCK`,
  `O_CLOEXEC`. Each needs honest semantics in our sys_write,
  sys_read, sys_exec.
- **Permission bits + chmod/chown** — adds `st_mode`, uid/gid
  tracking per inode. `mode_t`, `chmod(2)`, `chown(2)`,
  `umask(2)`. Per-proc uid/gid in `Proc`.
- **`stat`/`lstat`** — separate from `fstat`. Adds `sys_stat`,
  symlink handling.
- **`lseek`** — random-access read/write on a file descriptor.
  Routes through `File::Inode { off }` (already there as
  `AtomicU32`).
- **`pread`/`pwrite`** — explicit-offset variants.
- **POSIX `struct stat`** — adds `st_atime/mtime/ctime/blksize/
  blocks/mode/uid/gid`. Layout diverges from xv6 — gated behind
  `--feature posix-stat` or a separate `posix_stat` syscall (it's
  a bigger struct).

### Tier 2 — signals  (~2 sub-todos)

Signals are the second-biggest divergence from xv6. They need:

- **`sigaction`/`sigprocmask`/`sigreturn`** — kernel-side
  signal-state in `Proc`. Per-proc pending bitmask. Deliver on
  return-to-user by setting up a signal-frame on the user stack.
- **`SIGINT`** from Ctrl-C in `console_in`, **`SIGCHLD`** when a
  child exits, **`SIGALRM`** from `setitimer`.
- Kill-with-signal-number — extend our `sys_kill` to take a
  signo arg and store in target's pending bitmask.

The async kernel makes signal delivery cleaner than xv6's — the
"check signal" happens in `proc_main` right before
`UserMode::run`.

### Tier 3 — environment + argv  (~1 sub-todo)

- **`envp` in exec** — third arg to `execve(path, argv, envp)`.
  Store on user stack alongside argv; `__libc_init` reads it.
- **`getenv`/`setenv`** in user libc.

### Tier 4 — directory iteration  (~1 sub-todo)

- **`opendir`/`readdir`/`closedir`** — provided by user libc
  on top of our existing `open` + `read` on a directory.

### Tier 5 — process info  (~2 sub-todos)

- **`getppid`, `getuid`, `getgid`, `geteuid`, `getegid`,
  `setuid`, `setgid`**.
- **`getcwd(2)`** — currently no way to read cwd; needs to walk
  back up `..` from `proc.cwd`.

### Tier 6 — IPC + concurrency  (~bigger; can defer)

- **Unix-domain sockets** as named pipes through fs.
- **`waitpid(pid, &status, options)`** — wait for a specific
  child, optionally non-blocking. Generalises our `wait`.
- Real **`select`/`poll`** on fds. Routes through the async
  executor — fits naturally.

POSIX **`pthread_*`** is fundamentally at odds with our async-
single-task-per-proc model. Either we don't do it (recommend),
or we map pthread create to spawning sibling async tasks that
share the user pagetable. The latter is research, not engineering.

### Tier 7 — sockets  (~biggest)

TCP/IP stack + AF_INET + bind/listen/accept/connect. This is a
separate project. We could plug in `smoltcp` (Rust no_std TCP
stack) as a kernel module behind the HAL.

### Tier 8 — POSIX-ish libc  (~external)

Port **newlib** or **musl** in user space. They use a small set
of "OS-glue" syscalls; if we provide that set, off-the-shelf C
programs (`busybox`, `coreutils`, anything `./configure
--host=riscv64-unknown-posix && make`) link cleanly.

The minimal glue set is roughly: open, close, read, write, lseek,
stat, fstat, mmap, munmap, brk, fork, execve, wait4, kill,
sigaction, sigprocmask, getpid, getppid, gettimeofday, nanosleep,
ioctl, fcntl, pipe, dup, dup2, mkdir, rmdir, unlink, link,
symlink, readlink, chdir, getcwd, chmod, chown, getuid, geteuid,
getgid, getegid, setuid, setgid. That's ~40 syscalls.

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
