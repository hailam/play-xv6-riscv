# 07: sys_exec + multiple embedded binaries

**Done.** `sys_exec` swaps user pagetable + resets trapframe.
`embed::find(path)` looks up `/echo`, `/hello`, `/pipetest`, etc.

## Notes
- `proc.pagetable` was wrapped in `SpinLock` so `exec` can replace it.
- `proc_main` overwrites `tf.a0 = ret`; sys_exec returns `argv.len()`
  so that overwrite correctly sets argc.

## Files
- `crates/kernel/src/syscall.rs::sys_exec`
- `crates/kernel/src/embed.rs`
- `crates/kernel/build.rs` (multiple user-program build)
