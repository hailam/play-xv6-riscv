# 16: POSIX-ish compatibility

**Status (re-scoped 2026-06-10):** Tiers 1–5 **done**; Tier 8 is
**further along than this doc originally claimed** — a picolibc-based
toolchain is already integrated in-tree (`third_party/`, `picohello`/
`picotest` user programs) and real ported software runs on it: `bc`,
`dc`, and **lua** are built into fs.img on both arches. The remaining
scope of this todo is effectively **sockets**:
  * Tier 6 remainder — AF_UNIX domain sockets: **DONE (2026-06-11)**.
    `socket/bind/listen/accept/connect/socketpair` (syscalls 63-68),
    path-based API (sockaddr_un translation is the libc glue's job).
    Design: a connected socket is two `PipeInner`s (one per
    direction) so all blocking/waker/EOF machinery is the pipe code
    verbatim; endpoints decrement their direction counts in Drop.
    Bindings are real fs nodes (`T_SOCK = 5` — ls-visible,
    unlink-able) with an inum→`Weak<Listener>` registry; connect
    completes immediately (data buffered until accept, backlog cap
    16). poll() integrates (POLLIN/POLLOUT/POLLHUP/POLLERR +
    listener-readable). Found+fixed along the way: writes to a
    fully-closed read side now fail immediately (EPIPE-first in
    PipeWriteByte — previously they "succeeded" until the buffer
    filled; affected plain pipes too). No SIGPIPE raise (return -1
    only — documented divergence). `unixsock` usertest covers
    socketpair both directions, EOF, EPIPE, double-bind EADDRINUSE,
    fork'd echo server, unlink + stale-connect. Suite (75 tests)
    green at smp1 both arches + riscv smp3.
    (pthread_* stays out-of-scope by design: cooperative
    one-task-per-proc model.)
  * Tier 7 — TCP/IP + AF_INET. **DONE (2026-06-11): loopback M1 +
    virtio-net M2 both working.**
    The "no external crates" rule is now lifted: **smoltcp 0.12**
    (no_std, features medium-ethernet/medium-ip/proto-ipv4/socket-tcp/
    alloc/async) is the stack. Architecture: one global `NetStack`
    (loopback iface + optional eth iface + `SocketSet`) behind a
    SpinLock; a kernel `net_task` owns the poll loop (re-polls on a
    `kick()` waker or smoltcp's `poll_delay` deadline); blocking
    syscalls park on smoltcp's per-socket send/recv `Waker`s — which
    are exactly the executor's wakers (this is why an async kernel
    pays off here). AF_INET reuses the Tier-6 socket syscalls
    (`socket(2,1,0)` etc.) with `"a.b.c.d:port"` string addresses
    (sockaddr_in translation deferred to libc glue). `File::Socket`
    gained `Tcp`/`TcpListening` states; smoltcp has no backlog so a
    listener socket *becomes* the connection on SYN and accept()
    re-arms a fresh one.
      * **M1 (loopback, 127.0.0.1): DONE + gated.** Full TCP
        handshake / bidirectional data / FIN-EOF / connection-refused,
        no NIC needed. The `tcploop` usertest covers it and runs in
        every suite pass (74 tests green, both arches, smp1 + smp3).
        Found+fixed: writes to a fully-closed peer now fail
        immediately (EPIPE-first in PipeWriteByte — also fixed plain
        pipes/unix sockets).
      * **M2 (virtio-net, real host↔guest): DONE (2026-06-11).**
        Driver `crates/kernel/src/driver/virtio_net.rs` (second
        virtio-mmio slot, modern/v2, negotiates VIRTIO_F_VERSION_1 +
        VIRTIO_NET_F_MAC, 8 RX/8 TX bufs, 12-byte zero net-hdr) +
        smoltcp `Device` impl; eth iface 10.0.2.15/24 gw 10.0.2.2
        (qemu SLIRP); Makefile wires `-netdev user,hostfwd` +
        `virtio-net-device` on bus .1, both arches. Verified by
        `make test-net` (`scripts/test-net.py`): the guest runs
        `/tcpecho` on :7878 and the host connects through the
        forwarded port and gets a real round-trip echo (pcap shows
        the guest's `[P.] seq 1:13 length 12` data segment + clean
        FIN).
          THE BUG (root-caused via smoltcp source): a SINGLE
        `SocketSet` was shared across the loopback + eth interfaces.
        smoltcp's `socket_egress` walks every socket in the set it's
        given with no route filter, and the loopback (`Medium::Ip`)
        does NO route lookup in `dispatch_ip` — so the loopback poll
        "successfully" emitted the eth socket's data segment into the
        loopback void, and because `tcp::dispatch` commits
        `remote_last_seq` only *after* a successful emit, the seq
        advanced and the eth interface then sent only a bare ACK.
        Handshake survived (SYN/ACK retransmit). FIX: one `SocketSet`
        per interface + a `NetHandle { eth: bool, handle }` so a
        socket is only ever dispatched by the interface that routes
        it. `net.rs` `NetStack` now owns `lo_sockets` +
        `eth: Option<(dev, iface, SocketSet)>` and exposes
        `listen`/`relisten`/`connect`/`sock`/`remove` over handles.
      * M3 (deferred): aarch64 NIC (PCIe vs mmio on virt), DNS/UDP,
        sockaddr structs in the kernel, UDP sockets.
Also note: fd semantics were corrected by
[17-correctness-audit](../../done/17-correctness-audit/) — fork/dup/dup2
now share one open file description (shared offset) per POSIX.
**Estimated:** sockets are a multi-session project; everything else
here is done.

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
