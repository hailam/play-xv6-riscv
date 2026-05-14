# 11: aarch64 HAL

**Status:** Pending
**Estimated:** ~500 LoC
**Depends on:** —
**Unblocks:** running on Cortex-A class hardware (Raspberry Pi 3+, Apple Silicon dev boards under QEMU)

## Why

The original plan committed to "MMU-class only initially, with future
Cortex-A / aarch64 reachable as a HAL impl." Adding the second arch
proves the HAL trait surface is right; if it's not, we discover that
*now*, before more code accumulates that quietly depends on RISC-V
specifics.

## Approach

### Mirror the riscv64 HAL structure

```
crates/
  hal-aarch64/
    Cargo.toml
    asm/
      entry.S         # CPU bring-up, set up stack, jump to start_rust
      kernelvec.S     # EL1 kernel trap entry
      trampoline.S    # EL0↔EL1 trampoline page (mapped at fixed VA)
    src/
      lib.rs          # `impl Hal for AArch64`
      csr.rs          # CurrentEL, SPSR_EL1, ELR_EL1, TTBR0/1_EL1, SCTLR_EL1...
      pagetable.rs    # 4-level translation tables (granule 4K, 48-bit VA)
      start.rs        # EL2/EL3 → EL1 transition (drop to EL1 from EL2 typically)
      trap.rs         # Vector table, decode ESR_EL1, dispatch
      uart.rs         # PL011 driver
      memlayout.rs    # QEMU `-M virt` aarch64 layout
```

### Differences vs riscv64

- **Privilege levels:** EL3 (firmware) / EL2 (hypervisor) / EL1 (kernel) / EL0 (user). QEMU starts at EL2 by default; drop to EL1.
- **Page tables:** ARMv8 long-descriptor format. 4-level, 4K granule, 48-bit VA. Different attribute encoding from Sv39 (AP, AttrIndx, nG, etc.).
- **Trap entry:** vector table at VBAR_EL1, 16 entries (4 sources × 4 types). Need a single `kernelvec` covering current EL with SP_EL1 + lower EL with AArch64.
- **Timer:** ARM generic timer; CNTV_CTL_EL0 + CNTV_TVAL_EL0. Simpler than Sstc but with different IRQ source (GIC virtual timer IRQ).
- **PIC:** GIC (Generic Interrupt Controller) v2/v3 instead of PLIC. Different MMIO layout. `gic_init_for_hart` replaces `plic_init_for_hart`.
- **UART:** PL011 at `0x09000000` (QEMU virt aarch64) instead of NS16550 at `0x10000000`.

### `Hal` trait — does the surface fit?

Walk through each method:

- `hartid()` — read MPIDR_EL1 → aff0. ✓
- `intr_off/on/get` — toggle DAIF / read it. ✓
- `wfi/send_ipi` — `wfi` instruction; IPI via GIC SGI. ✓
- `now_ticks` — CNTVCT_EL0. ✓
- `install_pagetable/write_satp` — write TTBR0_EL1 + ISB + TLBI. The
  current trait uses `satp` naming; consider renaming to `set_page_table_root`
  or keep `write_satp` as a quirky alias (with a comment). The latter
  is more surgical.
- `PageTable` associated type — ARM impl provides its own. ✓
- `console_putc` — PL011 putc. ✓

The surface fits. One rename (or comment) on `write_satp` and we're done.

### Hal trait method consts

`MAX_CPUS` and other constants are currently `pub const` in
`hal_riscv64`; consider moving them to `Hal::MAX_CPUS` so the kernel
isn't `#[cfg]`-imported. Already partially done (`MAX_CPUS` is in
`crate::arch`).

### Target spec

QEMU aarch64-virt boots a `aarch64-unknown-none` target. Add to
`rust-toolchain.toml` `targets`.

Building:

```
cargo build --target=aarch64-unknown-none-softfloat -p kernel
```

For multi-arch kernel build, `.cargo/config.toml` has the default
target; explicit `--target` for aarch64.

## Verification

- `cargo build --target=aarch64-unknown-none-softfloat -p kernel` succeeds.
- `qemu-system-aarch64 -M virt -nographic -kernel ...` boots; same shell
  prompt appears.
- `/echo hello | /cat` works.

## Risks

- Memory model differences: ARMv8 is weaker than RISC-V WMO. Existing
  `fence(Ordering::SeqCst)` calls should suffice; audit any
  fences-as-comments and add real fences if needed.
- ARM uses different cache invalidation conventions (point-of-unification, etc.).
  Trampoline page must be in I-cache *and* D-cache coherently after
  install; may need explicit `dc cvac` + `ic ivau`. Verify.
- GIC v2 vs v3 selection: QEMU virt defaults to v2; pick one and document.

## Code touch points

- New crate: `crates/hal-aarch64/`
- `crates/kernel/Cargo.toml` — `[target.'cfg(target_arch = "aarch64")'.dependencies] hal-aarch64 = { path = "../hal-aarch64" }`
- `crates/kernel/src/arch.rs` — add `#[cfg(target_arch = "aarch64")] pub use hal_aarch64::AArch64 as Arch;`
- `Makefile` — add `make qemu-aarch64` variant
