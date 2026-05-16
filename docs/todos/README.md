# todos

Tracked work for the Rust xv6-riscv rewrite.

Each todo is a directory under `pending/`, `done/`, or `revisit/`. The
directory contains a `README.md` with the plan/summary, plus optional
`design.md` or `notes.md` for deeper context.

## Status snapshot

| Bucket | Count | What's in it |
|---|---|---|
| `done/` | 23 | Through fs writes — `mkdir` / `O_CREATE` / `unlink`, durable across reboot |
| `pending/` | 6 | Polish + portability |
| `revisit/` | 3 | Decisions to potentially revisit later |

## Pending — priority order

Userland-visible features are done for the in-tree binaries; the
remaining queue is polish + portability work.

1. [07-sys-kill-cancellation](pending/07-sys-kill-cancellation/) — `kill` + every `.await` checks `proc.killed`
2. [08-sbrk-and-malloc](pending/08-sbrk-and-malloc/) — user heap, tiny `malloc` in ulib
3. [09-vm-reaping](pending/09-vm-reaping/) — `Drop` for `PageTable`; stop leaking on `exec`/`exit`
4. [10-smp-user-procs](pending/10-smp-user-procs/) — per-CPU executors with sticky `home_cpu`
5. [11-aarch64-hal](pending/11-aarch64-hal/) — second HAL impl; prove the trait surface holds
6. [12-phase2-gui](pending/12-phase2-gui/) — minimal framebuffer-backed display

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

Current kernel totals: **~5,895 LoC, ~153 unsafe-ish lines** (~2.6%,
well inside the 700-line budget). Plus ~270 LoC host code in `mkfs/`,
~40 LoC in the shared `xv6-fs-layout` crate, and ~240 LoC user code
(`ls/mkdir/rm/wr` C sources).

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
