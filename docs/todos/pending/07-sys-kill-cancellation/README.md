# 07: sys_kill + cancellation

**Status:** Pending
**Estimated:** ~150 LoC
**Depends on:** —
**Unblocks:** `usertests` parity, robust shell (Ctrl-C)

## Why

xv6's `usertests` exercises `kill`-during-syscall hard: kill a process
while it's reading a pipe, sleeping, doing I/O. Each `.await` in our
syscall handlers needs to check `proc.killed` and exit early.

Without this, a killed proc that's blocked in (say) `sleep(1000)` keeps
its task alive for the full duration. The Plan agent flagged this as a
2-week-budget item back in the original design.

## Approach

### State

`Proc` already has `pub killed: AtomicBool` (added when sketching;
verify; if missing, add).

### `sys_kill(pid)`

```rust
async fn sys_kill(_proc, pid: i32) -> i64 {
    let Some(target) = find_proc_by_pid(pid as usize) else { return -1; };
    target.killed.store(true, Ordering::Release);
    // Wake any waker the target is parked on.
    // Easiest: enqueue the target's task; on next poll it sees killed and exits.
    executor::wake(target.task_id.load(Ordering::Relaxed));
    0
}
```

`find_proc_by_pid` scans `EXECUTOR.tasks` for a `Task` whose `proc`'s
`pid` matches. Linear scan is fine; N is small.

### Cancellation at every `.await`

Pattern: a `check_killed!()` macro that returns -1 from the enclosing
syscall if `proc.killed` is set.

```rust
macro_rules! check_killed {
    ($proc:expr) => {
        if $proc.killed.load(Ordering::Acquire) {
            return -1;
        }
    };
}
```

Audit every `.await` in `syscall.rs`:
- `sys_wait`'s `Wait { proc }.await` — check_killed before the await, AND make `Wait::poll` check killed → Ready(-1)
- `sys_sleep`'s `Sleep { deadline }.await` — same
- `sys_read`'s `ConsoleByte.await` / `PipeReadByte.await` — same
- `sys_write` pipe path — same

Cleanest: bake the check into each `Future::poll`. `Wait`, `Sleep`,
`PipeReadByte`, `PipeWriteByte`, `ConsoleByte` all already get
`&proc` somehow; pass the killed flag in too (or read via `cpu::current_proc()`).

### Exit semantics

In `proc_main`, after every event:

```rust
match event {
    TrapEvent::Syscall { nr } => {
        let ret = syscall::dispatch(&proc, nr).await;
        proc.trapframe().a0 = ret as u64;
        if proc.killed.load(Acquire) || proc.is_zombie() {
            // sys_exit if not already
            if !proc.is_zombie() {
                syscall::sys_exit(&proc, -1).await;
            }
            core::future::pending::<()>().await;
        }
    }
    ...
}
```

## Verification

- `/sleeptest` user binary: sleeps 1000 ticks.
- `/killer` user binary: kills the sleeper by pid.
- Shell runs them in sequence; the sleeper exits within ~100ms of kill.

Or — interactive: shell forks a sleeper, after 1s kills it (could be
shell-built-in syntax like `kill <pid>`).

## Risks

- The `check_killed` in every Future's poll requires the future to know
  the proc. `Sleep` currently doesn't — it just has a `deadline`. Need
  to add `proc: Arc<Proc>` or get it from `cpu::current_proc()`.
- `cpu::current_proc()` may be cleaner: the executor sets it when
  polling a proc task; futures read it. No extra parameters needed.

## Code touch points

- `crates/kernel/src/syscall.rs` — add `sys_kill`, modify every blocking future's `poll` to check killed
- `crates/kernel/src/proc.rs::Proc` — confirm `killed: AtomicBool` exists
- `crates/kernel/src/proc.rs::proc_main` — check killed after each dispatch
- `crates/kernel/src/uapi.rs` — `SYS_KILL = 6` already exists; just wire it up
- `crates/kernel/user/ulib.S` — `DEFINE_SYSCALL kill, 6`
- New: `crates/kernel/user/sleeptest.c` or similar for the demo
