# 10: SMP user procs — per-CPU executor + sticky home_cpu

**Status:** Pending
**Estimated:** ~200 LoC
**Depends on:** —
**Unblocks:** scaling beyond one user-runnable CPU

## Why

`executor::run` is currently a single global executor. Multi-hart users
in QEMU (`make qemu CPUS=3`) work for boot but only hart 0 runs user
procs; other harts sit in `wfi`. This caps user-mode throughput.

## Approach

The plan agent recommended **sticky `home_cpu`**: each task gets a CPU
at `spawn` time, never migrates. Avoids cross-CPU TLB shootdown for
user pagetables.

### Per-CPU executor

```rust
struct PerCpuExec {
    ready: SpinLock<VecDeque<TaskId>>,
    tasks: SpinLock<Vec<Option<Task>>>,
    next_id: AtomicU32,
}

static EXECUTORS: [PerCpuExec; MAX_CPUS] = ...;
```

The current `EXECUTOR` becomes `EXECUTORS[hartid]`. `run` reads
`EXECUTORS[Arch::hartid()]`.

### Cross-CPU wakers

A task running on CPU 0 may be waited on by another CPU's IRQ (e.g.,
virtio IRQ landed on CPU 1). The IRQ's wake call must enqueue to CPU
0's ready queue + IPI CPU 0.

```rust
fn wake(task_id: TaskId) {
    let home = TASKS[task_id].home_cpu;
    EXECUTORS[home].ready.lock().push_back(task_id);
    if home != Arch::hartid() {
        unsafe { Arch::send_ipi(1 << home) };
    }
}
```

`TASKS` becomes a global `Vec<Option<TaskMeta>>` (just `home_cpu`,
the per-CPU `tasks` array stores the future).

### Load balancing

At `sys_fork`, pick the CPU with the shortest ready queue. Rare event
so the global comparison is cheap.

### IPI plumbing

`Hal::send_ipi` is currently a stub. Implement via:
- RISC-V: CLINT MSIP bit set → translated to S-mode software interrupt via xv6-style M-mode timer trap, OR SBI `send_ipi`.
- Our M-mode trap path doesn't have an SBI; xv6's `timervec` is the
  classic approach. Adds complexity.

Alternatively: use `mip.SSIP` directly from M-mode helper? No, S-mode
can't set its own SSIP. The standard path is to relay via M-mode.

For now, simpler: use `SBI ext 0x735049 (sPI) call` or just `wfi` poll.

### Trampoline `tp`

Each hart still has `tp = hartid`. The IPI handler reads `tp` to know
which CPU it is. Already done.

## Verification

- `make qemu CPUS=3` runs the shell.
- Background `/echo` jobs split across CPUs.
- Print per-CPU task counts; observe non-zero on each.

## Risks

- IPI plumbing is the biggest unknown. May require a small M-mode trap
  shim (xv6-style `timervec`).
- Once SMP, certain `static`s become contention points. Locks have to
  be fine-grained enough that we don't hit a "big kernel lock" bottleneck.
  Most current locks are per-resource (per-proc, per-buffer) so should
  scale.
- Sticky `home_cpu` is great for TLB locality but skews load if one CPU
  gets a heavy fork chain. Acceptable for first cut.

## Code touch points

- `crates/kernel/src/executor.rs` — split into per-CPU state
- `crates/hal-riscv64/src/lib.rs::send_ipi` — implement
- Possibly: new `crates/hal-riscv64/asm/timervec.S` for M-mode IPI relay
- `crates/kernel/src/main.rs` — each hart calls `executor::run()`
