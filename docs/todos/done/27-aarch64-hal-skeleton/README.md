# 27: aarch64 HAL — skeleton  [DONE]

The HAL trait surface fits a second arch. `hal-aarch64` builds
cleanly against `aarch64-unknown-none-softfloat`. **This is scope-1
only**: the kernel does *not* yet boot on aarch64 — see
[[14-aarch64-completion]].

## What landed

- New crate `crates/hal-aarch64/` with:
  - `Cargo.toml` — `hal = { path = "../hal" }`.
  - `src/lib.rs` — `pub struct AArch64`, `impl Hal for AArch64`.
    Every trait method has a real implementation (DAIF for
    interrupts, MPIDR_EL1 for hartid, CNTVCT_EL0 for ticks, WFI,
    TTBR0_EL1 for the pagetable install) or a clearly-marked
    stub (`send_ipi`, the pagetable populate path).
  - `src/csr.rs` — minimal `mrs`/`msr` helpers for the system
    registers above.
  - `src/memlayout.rs` — QEMU virt aarch64 layout:
    KERNBASE=0x4000_0000, PL011 UART at 0x0900_0000, GIC at
    0x0800_0000, virtio-mmio at 0x0a00_0000, 48-bit MAXVA.
  - `src/pagetable.rs` — `pub struct PageTable { root_pa }` +
    `impl PageTableOps` skeleton (alloc + empty map/translate).
  - `src/uart.rs` — PL011 `putc` (real, spin on TXFF).
- `Cargo.toml` workspace `members` now includes `crates/hal-aarch64`.

Verification:
- `cargo build --release -p kernel` — still clean, 11 warnings (no
  new ones).
- `cargo build --release -p hal-aarch64 --target aarch64-unknown-none-softfloat`
  — clean, after `rustup target add aarch64-unknown-none-softfloat`.

## What this proves

The original `hal::Hal` trait surface is genuinely arch-independent
enough to be implemented twice. The methods that matter cleanly
mapped:

| `Hal` method        | riscv64                | aarch64               |
|---------------------|------------------------|-----------------------|
| `hartid`            | `tp`                   | `mpidr_el1 & 0xff`    |
| `intr_off/on/get`   | `sstatus.SIE`          | `daif.I`              |
| `wfi`               | `wfi`                  | `wfi`                 |
| `now_ticks`         | `time` csr             | `cntvct_el0`          |
| `install_pagetable` | `satp` + `sfence.vma`  | `ttbr0_el1` + ISB     |
| `console_putc`      | NS16550 THR            | PL011 DR              |
| `pagetable_satp`    | Sv39 mode8 + PPN       | `ttbr0` encoding      |

One quirk: `Hal::write_satp` is the trait method name. On aarch64
that writes TTBR0_EL1. Rename is in the follow-up.

## What's NOT here (and why)

The full "boots a shell" port needs substantial extra work the
skeleton intentionally skips:

- **Kernel `#[cfg(target_arch = "riscv64")]` scrubbing.** `main.rs`,
  `syscall.rs`, `proc.rs`, `vm.rs`, `user_vm.rs`, `usertrap.rs`,
  `trap.rs` all directly `use hal_riscv64::{TRAPFRAME, TRAMPOLINE,
  PGSIZE, TrapFrame, ...}`. Many of these need to move behind the
  `Hal` trait surface (or behind a new `arch::layout` re-export
  that picks the right module).
- **Trap vector table.** ARMv8 has a 2 KiB vector with 16
  entries; needs its own `kernelvec.S` analogue + an Rust trap
  dispatch that decodes ESR_EL1.
- **EL2 → EL1 drop in start.S.** QEMU boots aarch64 at EL2 by
  default; need a CPU bring-up shim like riscv's `start.S` that
  configures HCR_EL2 and ERETs to EL1.
- **Real long-descriptor populate** — the L0..L3 walk + AP/AttrIndx
  encoding + TLBI broadcast.
- **GIC v2 driver** (replaces PLIC).
- **PL011 init + RX interrupt** (current uart.rs has only `putc`
  spin-wait).
- **Cross-toolchain**: `aarch64-elf-gcc` (or `aarch64-none-elf-gcc`)
  for the user binaries. None of these are installed in this
  environment.

These are tracked in [[14-aarch64-completion]].

## Files

- New: `crates/hal-aarch64/Cargo.toml`
- New: `crates/hal-aarch64/src/{lib.rs, csr.rs, memlayout.rs,
  pagetable.rs, uart.rs}` (~210 LoC total)
- `Cargo.toml` workspace members list

## Verified at

`cargo build --release -p hal-aarch64 --target aarch64-unknown-none-softfloat`
