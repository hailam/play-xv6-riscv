# 06: sys_sleep + sys_wait + async wakers

**Done.** `WakerCell` (one-shot waker slot), timer wheel, `sys_sleep`/
`sys_wait` futures, parent-child linkage. Verification: parent forks,
child sleeps 1s, child prints+exits, parent's wait_waker fires.

## Notes
- The SIE-after-trap bug: hardware clears `sstatus.SIE` on trap entry;
  we never re-enabled before `wfi`. Fix: `Arch::intr_on()` at the top
  of `executor::run`.
- This was the first phase where the kernel genuinely `wfi`'d while
  procs were parked.

## Files
- `crates/kernel/src/wait.rs`
- `crates/kernel/src/time.rs`
- `crates/kernel/src/syscall.rs::{Wait, Sleep}`
