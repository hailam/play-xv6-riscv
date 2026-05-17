# 23: sys_kill + cancellation  [DONE]

`kill(pid)` now actually kills, and every blocking future in the
syscall layer bails when the proc holding it is killed.

## What landed

```
$ killtest
killtest: sending kill to pid 3
pid 3 exit(-1)
killtest: child reaped, exit=-1
pid 2 exit(0)
```

The child issued `sleep(10000)` — would normally take ~10s — but
the parent's `kill(pid)` 20 ticks later got the child reaped within
a few ms.

## Files

- `crates/kernel/src/proc.rs` — added `killed: AtomicBool`;
  `proc_main` now routes a killed proc into `sys_exit(-1)` after the
  syscall returns. Also fixed the zombie path: post-zombie tasks now
  park on `core::future::pending::<()>().await` so the executor
  sees `Pending` and never re-polls them.
- `crates/kernel/src/syscall.rs`:
  - `sys_kill(pid)` — looks up via `executor::find_proc_by_pid`,
    sets `killed`, then wakes every waker the proc may be parked
    on (wait_waker, console reader, executor task slot).
  - `current_proc_killed()` helper — reads `cpu::current_proc()`
    and checks `killed`.
  - `Sleep::poll`, `Wait::poll`, `ConsoleByte::poll`,
    `PipeReadByte::poll`, `PipeWriteByte::poll` — all check
    `current_proc_killed()` (or `self.proc.killed` for `Wait`)
    and return a cancellation sentinel.
  - `ConsoleByte::Output` switched from `u8` → `Option<u8>` (None = killed).
  - `PipeWriteByte::Output` switched from `()` → `bool` (false = killed).
    `pipe_write` also now returns -1 if readers all closed (was
    previously infinite-loop blocking).
  - `pipe_write` loop checks `proc.killed` per iteration.
  - `sys_exit` exposed as `pub(crate)` (split into a public
    re-exit wrapper around `sys_exit_inner`) so `proc_main` can
    invoke it for killed procs.
- `crates/kernel/src/executor.rs` — `find_proc_by_pid` helper
  (linear scan; N is tiny).
- `crates/kernel/src/console_in.rs` — exposed `wake()` so
  `sys_kill` can boot a parked console reader.
- New: `crates/kernel/user/kill.c` (~20 LoC) — `kill <pid>`.
- New: `crates/kernel/user/killtest.c` (~50 LoC) — the demo above.

Total: ~150 LoC kernel, ~70 LoC user. No new unsafe.

## Design notes

- **Where the killed flag is checked**: every async syscall that
  can block — `sys_sleep`, `sys_wait`, `sys_read` on
  console/pipe, `sys_write` on pipe. fs reads / writes are
  bounded by disk I/O and don't (yet) check killed; the next
  iteration would add it to `bio::bread`'s waker future.
- **Waking on kill**: kill wakes the target's task slot directly
  via `executor::wake(task_id)`. We also poke wait_waker and
  the console reader's WakerCell defensively, because the future
  on top may have stored a different waker than the proc's
  task_id (e.g. pipe wakers).
- **`proc_main` exit hook**: after the dispatch returns, if
  `proc.killed && !is_zombie()` we invoke `sys_exit(-1)`. This
  catches both "syscall ran to completion despite kill" and
  "syscall returned a cancellation sentinel".
- **Read-only stuff is still cancellable on next syscall**: a
  proc doing tight `getpid()` calls won't see kill until it
  blocks. That matches xv6.

## Not in this iteration

- Cancellation inside `fs::log::begin_op` / `bio::bread` —
  followups when we land usertests pressure.
- `sys_chdir`/`sys_link`/per-proc cwd — folded into a small
  future fs-polish todo rather than this one (this kept the
  diff focused).

## Verified at

`make fs.img && qemu-system-riscv64 ... < <(echo killtest)`
