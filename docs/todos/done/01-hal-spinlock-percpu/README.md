# 01: HAL trait + SpinLock + per-CPU state

**Done.** Three crates: `hal` (trait only), `hal-riscv64` (impl), `kernel`.
Added interrupt-aware `SpinLock<T>` with `push_off`/`pop_off`, and the
`Cpu` struct indexed by `tp`.

## Notes
- `Hal` trait surface intentionally small at this point; grew with each phase.
- `push_off`/`pop_off` is xv6's pattern for "disable interrupts across
  the critical section, restore on outermost release."
- Per-hart printing of `hart N up` verified by running with `CPUS=3`.

## Files
- `crates/hal/src/lib.rs`
- `crates/kernel/src/sync.rs`
- `crates/kernel/src/cpu.rs`
