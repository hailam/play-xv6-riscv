# 18: Process & file lifecycle completeness

**Status:** DONE (2026-06-10) — full suite (now 70 tests incl. two new
lifecycle regressions) prints **ALL TESTS PASSED on riscv64 AND
aarch64** (`-smp 1`).

## What landed

1. **A real init (pid 1) + orphan reparenting.** Previously initcode
   exec'd `/sh` directly, so pid 1 *was* the shell — there was nobody
   safe to reparent orphans to (sh's foreground `wait` would mis-reap
   them). Now:
   * `user/init.c` (new): forks `/sh`, loops in `wait()` discarding
     reaped orphans, restarts sh if it exits — classic xv6 init. fds
     0/1/2 come pre-wired from the kernel, so no `open("console")`.
   * `initcode.S` / `initcode-aarch64.S` exec `/init`;
     `build.rs` + `Makefile` build/install `init.elf` as `/init` on
     both arches.
   * Kernel: `proc::set_init_proc/init_proc` global handle (set at
     spawn — initcode execs in place, so that Proc IS init);
     `sys_exit_inner` splices `proc.children` onto init's children,
     re-points their parent weaks, and wakes init's `wait_waker`
     unconditionally (a child can turn zombie mid-splice). Zombie
     subtrees no longer pin `LIVE_PROCS`/the NPROC cap; orphans'
     `getppid()` returns 1.
2. **`iput`-on-last-close (deferred reap).** `Drop` impls can't run
   log transactions, so `fs/inode.rs` gained a reap queue +
   `iput_deferred(Arc<Inode>)` + an `inode_reaper()` kernel task
   (spawned in `bringup_then_init`). Enqueue points: `Drop for
   File::Inode`, `Drop for Vma` (file-backed mmap), `sys_chdir`'s old
   cwd, and exit's cwd release. The reaper re-checks under the inode
   lock (`strong_count <= 2` = cache + its own Arc, `nlink == 0`,
   `typ != 0`) and then mirrors `unlink_inside_op`'s free path
   (`itrunc; typ=0; iupdate`) in its own transaction. False-positive
   enqueues are harmless by design; unlinked files can't be re-opened
   (no dir entry), so there's no revival race.
3. **Executor slot reuse.** Completed tasks push their slot onto a
   per-CPU `free_slots` list that `insert_task` pops before growing
   `next_slot`; the 24-bit overflow `debug_assert` is now a hard
   `assert`. Stale tids for reused slots only cause spurious polls,
   which every future tolerates.

## New regression tests (in `usertests` quicktests)

* `orphanppid` — grandchild outlives its parent and reports its
  observed `getppid()` through a pipe; requires 1.
* `unlinkedfree` — 250× open/unlink-while-open/write/close
  (> NINODE=200): fails partway if the last close leaks the inode.

## Notes / follow-ups

* If init itself ever exited, children fall back to Arc-cascade
  freeing (commented in `sys_exit_inner`); init never exits.
* The reaper is single-task; under SMP its queue/waker pattern is the
  single-waiter `WakerCell` (correct by construction). The broader
  SMP gaps stay in [19-smp-hardening](../19-smp-hardening/).

## Key files
`user/init.c` (new), `user/initcode*.S`, `crates/kernel/build.rs`,
`Makefile`, `src/main.rs`, `src/proc.rs`, `src/syscall.rs`,
`src/fs/inode.rs` (reaper), `src/file.rs`, `src/executor.rs`,
`user/usertests.c`.
