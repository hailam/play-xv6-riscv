# todos

Tracked work for the Rust xv6-riscv rewrite.

Each todo is a directory under `pending/`, `done/`, or `revisit/`. The
directory contains a `README.md` with the plan/summary, plus optional
`design.md` or `notes.md` for deeper context.

## Status snapshot

| Bucket | Count | What's in it |
|---|---|---|
| `done/` | 29 | Through fs polish — cwd, chdir, hard links, sh `>` redirect |
| `pending/` | 4 | xv6 compat + aarch64 boot + POSIX track + phase-2 GUI |
| `revisit/` | 3 | Decisions to potentially revisit later |

## Pending — priority order

1. [15-xv6-compat](pending/15-xv6-compat/) — **NEXT** — close the
   real ABI gaps (BSIZE, stat layout, LOGSIZE, wait sig, user
   faults), port xv6's `ulib.c` + `printf.c`, then run xv6's
   `usertests` as a real stress harness.
2. [14-aarch64-completion](pending/14-aarch64-completion/) — fill
   the skeleton, scrub remaining `#[cfg]`s behind a
   `TrapPlumbing` trait, boot a shell under `qemu-system-aarch64`.
3. [16-posix-compat](pending/16-posix-compat/) — POSIX-ish track:
   permissions, signals, env/argv, libc port (newlib/musl). Index
   of multiple sub-todos; not a single deliverable.
4. [12-phase2-gui](pending/12-phase2-gui/) — minimal
   framebuffer-backed display.

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

Current kernel totals: **~6,555 LoC, ~158 unsafe-ish lines** (~2.4%,
well inside the 700-line budget). Plus ~270 LoC host code in `mkfs/`,
~40 LoC in the shared `xv6-fs-layout` crate, ~575 LoC user code, and
~210 LoC in `hal-aarch64` (skeleton).

## Revisit

- [futures-task-dep](revisit/futures-task-dep/) — hand-rolled waker plumbing; consider `futures-task` if it bites
- [strip-elf-size](revisit/strip-elf-size/) — `sh.elf` is 1.2 KB after `--strip-all`; could be smaller
- [sync-virtio-fallback](revisit/sync-virtio-fallback/) — `sync_read_block` exists but no longer used

## Workflow

When picking up a todo:

1. Read its `README.md` (and `design.md` if present)
2. Update task tracker in this session: `TaskCreate` with the todo name
3. Implement
4. Move the directory: `mv docs/todos/pending/<id-name> docs/todos/done/`
5. Update its `README.md` to note what landed + key files touched
6. Update this index's "Done" table
