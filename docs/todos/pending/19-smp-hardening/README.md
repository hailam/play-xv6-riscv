# 19: SMP hardening

**Status:** Pending
**Estimated:** 2-3 sessions
**Depends on:** [17-correctness-audit](../../done/17-correctness-audit/) (landed)
**Gate:** full `usertests` passes at `-smp 3` (riscv64) and `-smp 4`
(aarch64).

## Starting point (recorded 2026-06-10)

With the audit fixes in, `-smp 1` passes the FULL suite on both
arches, but `usertests` at `-smp 3` hangs early (≈`copyin`..`truncate3`,
output stalls, idle >60 s). Reproduce:
`make fs.img && qemu-system-riscv64 ... -smp 3` then `usertests`.

## Known gaps from the audit (each with a concrete mechanism)

1. **No cross-hart wake IPI** — `executor::wake` pushes to the home
   hart's queue; a hart parked in `wfi` only notices on its next timer
   tick (~10 ms). Latency at best; combined with anything else, a
   stall amplifier. ([[ipi-plumbing]] in executor.rs comments.)
2. **Mid-poll invisibility** — `run()` `take()`s the task while
   polling, so `find_proc_by_pid` can't see it: cross-hart `kill` of a
   mid-poll proc returns -1, and `time.rs` alarm delivery pops the
   wheel entry first and DROPS the SIGALRM forever when lookup misses.
   Fix: poll in place behind a flag, or a pid→Arc side table; requeue
   alarm entries on miss.
3. **Shared-buffer `data_mut` discipline** — bitmap RMW is locked now
   (`BITMAP_LOCK`), but two harts writing two different inodes in the
   SAME inode block via `iupdate` race byte-disjoint writes through
   `UnsafeCell` (UB-by-the-book, works on hw); needs per-buffer or
   per-inode-block serialization to be rigorous.
4. **pid u32 wrap** (`next_pid` fetch_add) — duplicate pids after 2^32
   forks; comparisons by value in waitpid/kill.
5. Anything the `-smp 3` hang itself turns out to be (instrument the
   park points like 17 did — the debug-print pattern in
   `done/17-correctness-audit` found the begin_op livelock in one run).

## Verification

- Full suite × both arches × `-smp 3/4`, plus a long `forkforkfork` +
  `manywrites` soak (10 min) for livelock confidence.
