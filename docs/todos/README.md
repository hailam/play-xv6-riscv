# todos

Tracked work for the Rust xv6-riscv rewrite.

Each todo is a directory under `pending/`, `done/`, or `revisit/`. The
directory contains a `README.md` with the plan/summary, plus optional
`design.md` or `notes.md` for deeper context.

## Status snapshot

| Bucket | Count | What's in it |
|---|---|---|
| `done/` | 36 | Through POSIX sockets + TCP/IP — AF_UNIX + smoltcp (loopback + virtio-net, real host↔guest echo via `make test-net`); **suite (74 tests) green smp1 both arches + smp3** |
| `pending/` | 1 | phase-2 GUI |
| `revisit/` | 3 | Decisions to potentially revisit later |

## Pending — priority order

1. [12-phase2-gui](pending/12-phase2-gui/) — minimal
   framebuffer-backed display. Its window-protocol IPC prerequisite
   (a Unix-socket equivalent) now exists (AF_UNIX, todo 16).

## Done — chronological

| ID | Title | LoC |
|---|---|---|
| 00 | [scaffold-boot](done/00-scaffold-boot/) | +250 |
| 01 | [hal-spinlock-percpu](done/01-hal-spinlock-percpu/) | +250 |
| 02 | [kalloc-paging](done/02-kalloc-paging/) | +500 |
| 03 | [trap-timer](done/03-trap-timer/) | +200 |
| 04 | [async-executor-first-user](done/04-async-executor-first-user/) | +800 |
| 05 | [fork](done/05-fork/) | +200 |
| 06 | [sleep-wait-wakers](done/06-sleep-wait-wakers/) | +100 |
| 07 | [exec-multiple-bins](done/07-exec-multiple-bins/) | +140 |
| 08 | [plic-uart-shell](done/08-plic-uart-shell/) | +260 |
| 09 | [pipes](done/09-pipes/) | +340 |
| 10 | [shell-pipelines](done/10-shell-pipelines/) | +180 |
| 11 | [argv](done/11-argv/) | +90 |
| 12 | [elf-loader](done/12-elf-loader/) | +220 |
| 13 | [virtio-disk-sync](done/13-virtio-disk-sync/) | +425 |
| 14 | [async-virtio-kernel-tasks](done/14-async-virtio-kernel-tasks/) | +75 |
| 15 | [buffer-cache](done/15-buffer-cache/) | +190 |
| 16 | [bio-eviction](done/16-bio-eviction/) | +60 |
| 17 | [bio-write](done/17-bio-write/) | +50 |
| 18 | [log-wal](done/18-log-wal/) | +210 |
| 19 | [mkfs-host-tool](done/19-mkfs-host-tool/) | +270 (host) |
| 20 | [fs-inode-and-dir](done/20-fs-inode-and-dir/) | +400 |
| 21 | [file-syscalls-read-path](done/21-file-syscalls-read-path/) | +310 (+120 user) |
| 22 | [fs-writes](done/22-fs-writes/) | +570 (+120 user) |
| 23 | [sys-kill-cancellation](done/23-sys-kill-cancellation/) | +150 (+70 user) |
| 24 | [sbrk-and-malloc](done/24-sbrk-and-malloc/) | +50 (+160 user) |
| 25 | [vm-reaping](done/25-vm-reaping/) | +130 |
| 26 | [smp-user-procs](done/26-smp-user-procs/) | +150 (+25 user) |
| 27 | [aarch64-hal-skeleton](done/27-aarch64-hal-skeleton/) | +210 (hal-aarch64) |
| 28 | [fs-polish](done/28-fs-polish/) | +180 (+80 user) |
| 29 | [aarch64-completion](done/14-aarch64-completion/) | +1500 — boots interactive shell under qemu-system-aarch64 -smp 4 |
| 30 | [xv6-compat](done/15-xv6-compat/) | (spread) — **full usertests suite passes, both arches**; G1/G3/G9 ruled obsolete |
| 31 | [correctness-audit](done/17-correctness-audit/) | +750/−287 — kernel-wide audit; lost-wakeup waker class, begin_op livelock, free-list heap allocator, fd description sharing, fault-type checks, OOM leak fixes |
| 32 | [proc-lifecycle](done/18-proc-lifecycle/) | ~+330 — real init (pid 1) + orphan reparenting, deferred-iput inode reaper, executor slot reuse; +2 regression tests |
| 33 | [smp-hardening](done/19-smp-hardening/) | ~+150 — riscv M-mode CLINT-MSIP→SSIP IPI trampoline, aarch64 SGI wiring, wake-IPIs, pid registry; **full suite green at -smp 3/4** |
| 34 | [fs-console-robustness](done/20-fs-console-robustness/) | ~+300 — sparse-hole reads, ftruncate clamp, iget-wait, lazy demand-map in copyin/copyout, console line discipline (^D/erase/echo), fair console_write, waker-driven poll; +2 tests |
| 35 | [posix-compat (sockets+TCP/IP)](done/16-posix-compat/) | AF_UNIX (syscalls 63-68, fs-visible bindings) + TCP/IP via smoltcp 0.12 (loopback + virtio-net, per-interface SocketSets); first external crate; `tcploop`+`unixsock` usertests + `make test-net` host↔guest echo |

(Rows 30/31 live in `done/15-xv6-compat` and `done/17-correctness-audit`
— their numeric directory prefixes collide with older done entries; the
table is chronological.)

## Revisit

- [futures-task-dep](revisit/futures-task-dep/) — hand-rolled waker
  plumbing; consider `futures-task` if it bites. *(2026-06-10 note:
  its "our WakerCell is fine as-is" claim aged poorly — the audit
  found five multi-waiter sites that needed a hand-rolled `WakerList`
  with wake_all. The hand-rolled path still held up; revisit only if
  waker complexity keeps growing.)*
- [strip-elf-size](revisit/strip-elf-size/) — `sh.elf` is 1.2 KB after
  `--strip-all`; could be smaller
- [sync-virtio-fallback](revisit/sync-virtio-fallback/) —
  `sync_read_block` exists but no longer used

## Workflow

When picking up a todo:

1. Read its `README.md` (and `design.md` if present)
2. Update task tracker in this session: `TaskCreate` with the todo name
3. Implement
4. Move the directory: `mv docs/todos/pending/<id-name> docs/todos/done/`
5. Update its `README.md` to note what landed + key files touched
6. Update this index's "Done" table
