# 29: AArch64 HAL comment audit & TrapFrame layout assertions

**Status:** Pending — needs discovery & verification
**Scope:** `crates/hal-aarch64/`, `crates/hal/`, `crates/kernel/src/main.rs`, `crates/kernel/build.rs`, `Makefile`

## Problem statement

Several comments and build targets still describe the pre-bringup state where `hal-aarch64` was an unimplemented skeleton and only RISC-V built user programs. In addition, `hal_aarch64::trapframe::TrapFrame` lacks compile-time layout assertions (`size_of` / `offset_of!`) matching `crates/hal-aarch64/asm/trampoline.S`, whereas `hal_riscv64::trapframe::TrapFrame` enforces them at compile time.

## Discovery & research checklist

1. **Comment and docstring audit across `hal-aarch64` and `kernel`:**
   - Inspect `crates/hal-aarch64/src/lib.rs` (module header at lines 3–10, `TIMER_INTERVAL` comment at lines 82–83, and `trap surface — skeleton` banner at lines 193–198).
   - Inspect `crates/hal-aarch64/src/trapframe.rs` (lines 1–3) which still describes the struct as a skeleton for a future trampoline.
   - Check `crates/kernel/src/main.rs` (`kernel_free_frame` doc comment at lines 153–155) and `crates/kernel/build.rs` (`initcode` comment at lines 145–147) for stale `riscv64`-only wording.
   - Check whether `TIMER_INTERVAL` in `crates/hal-aarch64/src/lib.rs` is actually used by `trap.rs` (or if `trap.rs` reads `CNTFRQ_EL0` directly) and document or reconcile the constant with `crates/kernel/src/net.rs`.
2. **Verify `TrapFrame` offsets against `crates/hal-aarch64/asm/trampoline.S`:**
   - Trace every `ldr`/`str`/`ldp`/`stp` offset in `uservec_sync`, `uservec_irq`, and `trampoline_userret` (`#0`, `#8`, `#16`, `#24`, `#32`, `#40`, `#48`, `#56`..`#296`, `#304`, `#312`).
   - Confirm exact byte offsets and total struct size (`320` bytes) of `hal_aarch64::trapframe::TrapFrame`.
3. **Check `Makefile` cleanup targets:**
   - Verify what artifacts `make build-aarch64` and `make fs-aarch64.img` produce (`fs-aarch64.img`, `target/user-aarch64`, etc.) and ensure `make clean` removes them.

## Verification

- Add a `const _: () = { assert!(...); };` block in `crates/hal-aarch64/src/trapframe.rs` checking `size_of::<TrapFrame>()` and `offset_of!` for `kernel_satp`, `kernel_sp`, `kernel_trap`, `kernel_hartid`, `elr_el1`, `sp_el0`, `spsr_el1`, `x`, `esr_el1`, and `trap_kind`.
- Build both `riscv64gc-unknown-none-elf` and `aarch64-unknown-none-softfloat` targets cleanly and run the test suite on both architectures.
