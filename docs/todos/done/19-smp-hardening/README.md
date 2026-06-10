# 19: SMP hardening

**Status:** DONE (2026-06-10) — gate met: full `usertests` suite
prints **ALL TESTS PASSED at `-smp 3` (riscv64, 222 s) and `-smp 4`
(aarch64, 67 s)**, with `-smp 1` regressions clean on both arches.

## What the hang actually was

Not a deadlock — **cross-hart wake latency**. `executor::wake` to a
remote hart just pushed onto that hart's ready queue; a hart parked in
`wfi` (or running user code) only noticed on its next timer tick, and
the tick is **100 ms** (`TIMER_INTERVAL = 1_000_000` @ 10 MHz).
`truncate3` is the first test whose two procs land on different harts
and ping-pong through `begin_op`/`ilock` parks — thousands of parks ×
100 ms ≈ forever. (Instrumented evidence: both pids alive, looping
open/close + `beginop-park`, zero deadlock.) With IPIs, truncate3 went
from unbounded to **3.2 s**.

## What landed

1. **riscv cross-hart IPIs without SBI** (`-bios none` boots leave us
   M-mode): the classic CLINT dance —
   * `start.rs`: per-hart `MSCRATCH` (slot 2 = this hart's CLINT MSIP
     address), `mtvec` → a tiny M-mode `m_ipivec` trampoline,
     `mie.MSIE` enabled. The trampoline clears MSIP and sets `SSIP`
     (supervisor-software, delegated), then `mret`s.
   * `Hal::send_ipi(hart_mask)` riscv impl rings `CLINT+4*hart`
     doorbells; new `Hal::EXTRA_MMIO` const gets the CLINT page
     identity-mapped into the kernel pagetable (vm.rs loops it).
   * S-mode handling: kernel trap (`scause=1`) clears SSIP and
     returns (the wfi already broke); user-mode decode maps SSIP to
     `Devintr` (riscv `handle_external` tolerates a zero claim) so a
     kick mid-user-mode just bounces through the executor.
2. **aarch64**: `send_ipi` (GIC SGI 0 broadcast) existed but had **no
   caller**; reception was kernel-path-only. Added the SGI arm to the
   user-path `handle_external_irq` (claim+complete = the whole job;
   SGI 0 was already per-hart enabled in `init_for_hart`).
3. **`executor::wake` sends the IPI** whenever the home hart isn't the
   current one.
4. **Pid registry** (`proc::PROC_REGISTRY`, `Vec<Weak<Proc>>`,
   registered in `spawn_proc_main`, dead weaks purged on lookup):
   `find_proc_by_pid` no longer scans the per-CPU task tables, which
   are blind to a task **mid-poll** (`run()` `take()`s it) — that
   blindness made cross-hart `kill` intermittently return -1 and made
   `time.rs` alarms get dropped on the floor.

## Residual SMP notes (documented, not blocking the gate)

* `iupdate` for two different inodes in the same disk block can write
  byte-disjoint ranges of one `Buffer` from two harts concurrently —
  benign on real hardware, UB-by-the-letter through `UnsafeCell`;
  a per-buffer write lock would make it rigorous.
* pids are `AtomicU32` `fetch_add` — wrap at 2^32 forks.
* IPIs are broadcast on aarch64 (`sgi_all_except_self`) — a precise
  target mask would shave spurious wakeups; harmless today.

## Key files
`hal/src/lib.rs` (EXTRA_MMIO), `hal-riscv64/src/{start.rs,lib.rs,
trap.rs,csr.rs}`, `hal-aarch64/src/trap.rs`, `kernel/src/{vm.rs,
executor.rs,proc.rs}`.
