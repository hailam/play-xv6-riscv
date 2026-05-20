# 14: aarch64 — boot to a shell

**Status:** Pending
**Estimated:** ~700 LoC + cross-toolchain install
**Depends on:** [[27-aarch64-hal-skeleton]] (DONE)
**Unblocks:** `qemu-system-aarch64 -M virt` actually running the
shell

## Why

The skeleton crate compiles, but the kernel doesn't yet build for
aarch64. The remaining work is the *real* port: scrubbing the
kernel's RISC-V-specific imports, writing the trap path, and
filling in the pagetable / GIC / PL011 stubs.

## Concrete checklist

### 1. `Hal` trait surface widen

Move the riscv64-specific re-exports off direct `use hal_riscv64`
calls and behind the trait:

```rust
pub trait Hal: 'static {
    type PageTable: PageTableOps;
    type TrapFrame: Default + Copy + 'static;

    const PGSIZE: usize;
    const TRAMPOLINE: usize;
    const TRAPFRAME: usize;
    const TIMER_INTERVAL: u64;

    fn trampoline_pa() -> usize;
    fn uservec_offset() -> usize;
    fn userret_offset() -> usize;
    fn write_pagetable_root(satp_or_ttbr: usize);  // rename `write_satp`
    // ... existing methods
}
```

Then everything that currently says `use hal_riscv64::{PGSIZE, TRAPFRAME, ...}`
becomes `use crate::arch::{Arch, Hal}; let pgsize = <Arch as Hal>::PGSIZE;`.

### 2. Scrub kernel `#[cfg(target_arch = "riscv64")]`

Files with direct `hal_riscv64` use:

- `crates/kernel/src/syscall.rs`
- `crates/kernel/src/proc.rs`
- `crates/kernel/src/vm.rs`
- `crates/kernel/src/user_vm.rs`
- `crates/kernel/src/usertrap.rs`
- `crates/kernel/src/trap.rs`

For each: either go through `Arch::` or behind a per-arch
re-export module.

### 3. aarch64 boot

- `hal-aarch64/asm/entry.S` — set up stack, drop EL2→EL1, jump to
  `kmain`.
- `hal-aarch64/asm/kernelvec.S` — 16-entry VBAR_EL1 vector. Save
  regs, dispatch to Rust.
- `hal-aarch64/asm/trampoline.S` — EL0↔EL1 trampoline at fixed VA.

### 4. Pagetable populate

- 4-level walk, 4K granule, 48-bit VA.
- Encode AP/AttrIndx/nG; convert from `PtePerm` bits.
- TLBI VMALLE1IS + DSB ISH + ISB on install.

### 5. GIC v2 driver

`crates/hal-aarch64/src/gic.rs`. Same API shape as
`hal_riscv64::plic`: `init`, `init_for_hart`, `enable_irq`,
`claim`, `complete`.

### 6. PL011 init

- Disable UART (CR = 0).
- Set baud (IBRD / FBRD).
- Set LCRH = 8N1 + FIFO enable.
- Enable UART (CR = TXE | RXE | EN).
- Enable RX interrupt (IMSC.RXIM).

### 7. User toolchain

`brew install aarch64-elf-gcc` (or `aarch64-none-elf` via
`gcc-arm-embedded`). Update `build.rs` to pick the right
toolchain per `cfg(target_arch)`. User binaries become arch-
specific.

### 8. Makefile / xtask

```makefile
qemu-aarch64: build-aarch64 fs-aarch64.img
    qemu-system-aarch64 -M virt -cpu cortex-a72 \
        -bios none -kernel target/aarch64-unknown-none-softfloat/release/kernel \
        ...
```

## Verification

Same gate the original `11-aarch64-hal` listed: shell prompt
appears under `qemu-system-aarch64 -M virt`, and `/echo hello`
runs.

## Risks

- ARMv8 weak memory model: existing `Ordering::SeqCst` should
  cover most cases, but worth a fence audit.
- aarch64 cache invalidation conventions are stricter; trampoline
  page may need `dc cvac` + `ic ivau` after install.
- GIC v2 vs v3 split: QEMU virt defaults to v2; pin that for now.

## Code touch points

- `crates/hal/src/lib.rs` — trait widening
- `crates/hal-aarch64/` — fill in every TODO from the skeleton
- `crates/hal-aarch64/asm/` — new
- Every kernel file currently using `hal_riscv64::*` directly
- `crates/kernel/build.rs` — multi-arch user toolchain dispatch
- `Makefile` — `qemu-aarch64` target
