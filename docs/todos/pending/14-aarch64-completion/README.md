# 14: aarch64 — boot to a shell

**Status:** Pending — **after [[15-xv6-compat]]**
**Estimated:** 3–4 sessions + cross-toolchain install
**Depends on:** [[27-aarch64-hal-skeleton]] (DONE),
[[15-xv6-compat]] (so we share verification with the riscv64 path
via `usertests`)
**Unblocks:** `qemu-system-aarch64 -M virt` actually running the
shell.

## Why

The skeleton crate compiles. The kernel now uses the `Hal` trait
surface for **all** the cross-arch consts, helpers, and trap-frame
access — so the kernel itself is closer to building for aarch64
than the README originally implied. What's left is the arch-
specific *implementation* — start path, trap vectors, pagetable
populate, interrupt controller, timer, user-mode trampoline.

## What's already done (since this todo was first written)

- ✅ Trait widening: `PGSIZE/KERNBASE/PHYSTOP/TRAMPOLINE/`
  `TRAPFRAME/TIMER_INTERVAL/UART0/VIRTIO0/INTC_BASE/UART0_IRQ/`
  `VIRTIO0_IRQ` are all `Hal` consts. AArch64 impl provides real
  values.
- ✅ `TrapFrameAccess` trait: `epc/sp/arg/syscall_nr/set_*`. AArch64
  impl uses `ELR_EL1/SP_EL0/x0..x7/x8`.
- ✅ **Phase A done**: `cargo build --target
  aarch64-unknown-none-softfloat -p kernel` succeeds (1 expected
  linker warning — `_start` not defined yet).
- ✅ `Hal` trait now exposes:
  - `decode_user_trap(tf) -> UserTrapCause`
  - `arm_timer`, `handle_external_irq`, `init_kernel_trap_vec`,
    `on_user_trap_entry`
  - `return_to_user(tf, user_satp) -> !` (noreturn; handles all
    arch-specific CSR setup + trampoline jump)
  - `init_console`, `init_intc_global`, `init_intc_per_hart`
  - `console_try_getc`
  - `install_free_frame`
- ✅ Kernel scrubbed of ALL arch-specific imports except in
  `arch.rs` (the selector, correct).
- ✅ AArch64 impls provide real values for the trait consts +
  unimplemented!()-style stubs for the trap path methods.

## Concrete remaining checklist

Each item has an LoC estimate (rough) and a verification gate.

### Phase A: kernel compiles for aarch64 — **DONE**

### Phase B: first PL011 print  (~150 LoC, gate: "rust kmain" appears under qemu-system-aarch64)

- **`hal-aarch64/asm/entry.S`** (~80 LoC) — entry from EL2 (QEMU
  virt default), set up SP per hart (uses MPIDR_EL1.aff0), drop
  to EL1 via HCR_EL2 = HCR_RW + ERET to `kmain`. Stack base
  laid out like the riscv64 `_stack0`.
- **`hal-aarch64::install_free_frame`** — already present, but
  PL011 init needs to happen before we print, so register the
  free-frame callback after `uart::init()`.
- **PL011 real init** (~40 LoC) — CR=0, IBRD/FBRD = baud,
  LCRH = 8N1 | FIFO, CR = TXE | RXE | EN. RX interrupt enable
  comes later with GIC.

### Phase C: kvm + paging on  (~250 LoC, gate: kernel continues running after install_pagetable)

- **4-level long-descriptor populate** (~200 LoC) — walk L0..L3,
  4 KiB granule, 48-bit VA. Encode AP (data perm), AttrIndx
  (cacheability), AF (accessed-flag), nG (non-global). Convert
  from our `PtePerm` bits.
- **`install_pagetable` real impl** (~50 LoC) — write TTBR0_EL1,
  set MAIR_EL1 attrs, TCR_EL1 layout fields, SCTLR_EL1.M to
  enable MMU, DSB ISH + ISB + TLBI VMALLE1IS.

After: "kvm: installed (ttbr0=...)" appears, kernel keeps
running.

### Phase D: vector table + GIC + timer  (~270 LoC, gate: timer ticks visible for 30s)

- **`hal-aarch64/asm/kernelvec.S`** (~150 LoC) — VBAR_EL1 vector
  table (16 entries × 128 bytes). For "current EL with SP_EL1
  → IRQ/sync/serror", save GP regs, jump to Rust dispatch.
- **GICv2 driver** (~120 LoC) — distributor init + per-cpu
  interface init, `enable_irq(int_id)`, `claim()` reads IAR,
  `complete(int_id)` writes EOIR. Same shape as our
  `hal_riscv64::plic`.
- **ARM generic timer** (~50 LoC) — program `CNTV_TVAL_EL0`,
  enable in `CNTV_CTL_EL0`. Handle GIC PPI 27 in the trap
  dispatcher.

### Phase E: user mode + cross-toolchain  (~200 LoC + toolchain)

- **`hal-aarch64/asm/trampoline.S`** (~120 LoC) — analogue of
  riscv's. Save user regs into `AArch64::TrapFrame` (matching
  field offsets the trapframe.rs already declares), switch
  TTBR0_EL1 via `kernel_satp` slot, ERET to EL0. Reverse path
  on entry.
- **`TrapPlumbing::decode_trap`** for aarch64 — read `ESR_EL1`,
  decode EC field (0x15 = SVC from AArch64 → syscall, 0x24/0x25
  = data abort from EL0, etc.), produce same `TrapCause` enum
  the riscv impl returns.
- **`brew install gcc-arm-embedded`** or equivalent
  (`aarch64-elf-gcc` / `aarch64-none-elf-gcc`).
- **`build.rs`** multi-arch dispatch: pick toolchain per
  `cfg(target_arch)`, write `target/user/<arch>/<name>.elf`.
- **`Makefile`** new `qemu-aarch64` target.

After: same shell session boots under
`qemu-system-aarch64 -M virt -bios none -kernel ...`.

### Phase F (optional): aarch64 SMP + IPI  (~100 LoC)

GIC SGIs for cross-CPU wakeups. (The riscv64 path also defers
real IPI to timer-tick polling — both arches can land this together
as a separate todo.)

## Total estimate

~990 LoC across phases A–E + toolchain install. Realistic
calendar: 4 focused sessions, with each phase having its own
visible milestone so we can stop and resume cleanly.

## Verification gate (whole-todo done)

Same shell session as our riscv64 build:

```
$ /ls /
$ /echo hello on aarch64
$ /mkdir /work
$ /cd /work
$ /wr greet hi
$ /cat greet
$ /killtest
```

Output identical (modulo the hartid in the exit log) under
`qemu-system-aarch64 -M virt -cpu cortex-a72 -nographic -kernel
target/aarch64-unknown-none-softfloat/release/kernel -drive
file=fs.img,...`.

Plus: with [[15-xv6-compat]] landed, run `usertests` and confirm
the same pass rate on both arches.

## Risks

- **GICv2 vs v3.** QEMU virt defaults to v2 (the cortex-a72
  default); pin that.
- **Cache invalidation conventions.** ARMv8 is stricter than
  RISC-V about I-cache vs D-cache coherency. After installing
  the trampoline page, may need `dc cvac` + `ic ivau` + `dsb` +
  `isb` to make sure the instruction stream sees the freshly-
  written code. This caught us once during the riscv path; expect
  it here too.
- **EL2 entry assumptions.** QEMU virt boots at EL2 by default,
  but `-cpu cortex-a72,el2=off` could land us at EL1 directly.
  `entry.S` should check `CurrentEL` and branch.
- **Trapframe field offsets and trampoline.S.** Just like the
  riscv side, the offsets the asm uses must exactly match the
  `repr(C)` struct. `const _: () = assert!(offset_of!(...))` in
  `trapframe.rs` is mandatory.

## Code touch points

- `crates/hal/src/lib.rs` — `TrapPlumbing` extension to Hal.
- `crates/hal-aarch64/src/{lib.rs, csr.rs, pagetable.rs,
  trapframe.rs, uart.rs}` — fill the stubs.
- `crates/hal-aarch64/src/{start.rs, trap.rs, gic.rs}` — new.
- `crates/hal-aarch64/asm/{entry.S, kernelvec.S, trampoline.S}` — new.
- `crates/kernel/src/{usertrap.rs, trap.rs}` — route through
  the new TrapPlumbing trait.
- `crates/kernel/build.rs` — multi-arch toolchain dispatch.
- `Makefile` — `qemu-aarch64` target.
