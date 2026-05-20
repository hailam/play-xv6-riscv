# 26: SMP user procs — per-CPU executor + sticky home_cpu  [DONE]

Multi-hart execution. With `-smp 3`, user procs run on hart 0, 1,
and 2.

## What landed

`smptest` forks 6 concurrent children that each sleep briefly and
then `exit(0)`. The kernel's exit log shows the hart that reaped
each one:

```
$ smptest
smptest child done
pid 3 exit(0) on hart 0 kalloc.free=32319
smptest child done
smptest child done
pid 5 exit(0) on hart 0 kalloc.free=32333
pid 4 exit(0) on hart 1 kalloc.free=32333
smptest child done
smptest child done
pid 6 exit(0) on hart 2 kalloc.free=32349
pid 7 exit(0) on hart 0 kalloc.free=32349
...
```

Pids landed on harts 0, 1, and 2 — exactly what we want.

## Files

- `crates/kernel/src/executor.rs` — full rewrite:
  - `TaskId` encodes `(cpu, slot)` as one `u32` (8 + 24 bits).
    `tid_cpu`, `tid_slot`, `make_tid` helpers.
  - `static EXECUTORS: [PerCpuExec; MAX_CPUS]` — each CPU owns
    its `tasks` Vec + `ready` VecDeque + `next_slot` counter.
  - `spawn` picks the least-loaded CPU at spawn time. The pick is
    sticky for the task's lifetime (no migration). Cheap because
    forks are infrequent and `MAX_CPUS == 8`.
  - `spawn_kernel_on(cpu, ...)` — explicit pin, used for the
    boot-time `bringup_then_init` task (anchored to hart 0 so the
    fs init can't race with hart 1/2 plumbing).
  - `wake(tid)` decodes the home CPU and enqueues on that CPU's
    ready queue. Remote-CPU wakes are picked up at the remote
    hart's next timer tick (no IPI plumbing yet — see
    [[ipi-plumbing]] in the deferred list).
  - `run()` reads `EXECUTORS[Arch::hartid()]`. If a stale tid
    addressed to another CPU shows up locally, the loop forwards
    it.
  - `find_proc_by_pid` scans every CPU's task vec.
- `crates/kernel/src/main.rs` — every hart calls `executor::run`
  after its per-hart init; previously only hart 0 did.
- `crates/kernel/src/syscall.rs::sys_exit_inner` — exit log now
  reports `on hart N` so SMP distribution is visible at a glance.
- New: `crates/kernel/user/smptest.c` — the demo above. Forks 6
  children that sleep different amounts so their `exit` events
  don't all serialise on the same hart.

Total: ~150 LoC kernel + ~25 LoC user. No new unsafe.

## Design notes

- **No IPI**: a cross-CPU `wake` enqueues to the remote queue but
  doesn't notify the remote hart. The remote hart drains its queue
  on the next timer-tick wake (every TIMER_INTERVAL = 100ms in
  QEMU). For shell-paced workloads this is plenty; usertests-style
  pressure would benefit from real IPIs. Tracked as a follow-up.
- **Stickiness**: tasks never migrate. That keeps the user
  pagetable's TLB working-set on one hart and removes any need
  for cross-CPU TLB shootdowns for user mappings. The kernel
  pagetable's mappings are identical on every hart, so its TLB
  state is unaffected.
- **Tie-break in `pick_home_cpu`**: ties go to the lowest hart
  ID. With a synchronous shell (fork+wait), each fork sees
  `[0, 0, 0]` and lands on hart 0 — that's why `echo run-N`
  produces all-hart-0 output. Concurrent fork chains (smptest)
  break the tie naturally as the queues diverge.
- **No locking on `TaskId`**: encoding `home_cpu` into the tid
  means the RawWaker is just an integer; no global table lookup
  is needed on wake.

## Stuff deferred / written down

- **IPI plumbing**: `Hal::send_ipi` is still a stub. The classical
  RISC-V path is M-mode CLINT MSIP. We boot `-bios none` and never
  re-enter M-mode after `start.S`, so adding a CLINT shim is its
  own little project. For now, timer-tick polling is enough.
- **Smarter load balancing**: round-robin across ties, or watching
  task histograms. Not needed yet.

## Verified at

`make fs.img && qemu-system-riscv64 ... -smp 3 ... < <(echo smptest)`
