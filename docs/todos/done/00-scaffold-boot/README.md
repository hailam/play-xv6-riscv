# 00: scaffold + boot

**Done.** Cargo workspace, `rust-toolchain.toml`, custom `.cargo/config.toml`,
`kernel.ld` linker script, `entry.S`. M-mode kernel that pokes the
NS16550A UART and prints "rust kmain".

## Notes
- Booted on QEMU `-machine virt -bios none -kernel ...`.
- Initial UART code is raw MMIO; HAL came in the next phase.
- Linker script puts `.text.entry` first so `_entry` sits at `0x80000000`.

## Files
- `crates/kernel/src/main.rs`
- `crates/hal-riscv64/asm/entry.S` (was kernel/asm/entry.S; moved in 01)
- `crates/kernel/kernel.ld`
