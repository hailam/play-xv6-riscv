# 05: async executor refactor + sys_fork

**Done.** `Task`/`Waker`/`MpscQueue` plumbing, `proc_main` as an `async fn`
that loops `UserMode::run.await`, `sys_fork` that spawns a child task.
Two procs print distinct banners.

## Notes
- `UserMode::poll` sets per-CPU `user_target` and returns Pending;
  executor noreturns into user mode AFTER putting the task back in its
  slot (closes the race).
- `proc_main`'s state machine survives across return-to-user boundaries
  because the Future is heap-pinned.

## Files
- `crates/kernel/src/executor.rs`
- `crates/kernel/src/proc.rs::proc_main`
- `crates/kernel/src/syscall.rs::sys_fork`
