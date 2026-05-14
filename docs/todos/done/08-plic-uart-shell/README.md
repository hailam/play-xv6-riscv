# 08: PLIC + UART RX IRQ + tiny shell

**Done.** First device-driven async waker. UART RX IRQ routes through
PLIC (source 10) → `kernel_on_external` → `console_in::push` →
`READER.wake()`. `sys_read` parks on the reader waker.

`initcode` becomes a minimal asm shell.

## Notes
- `-nographic` muxes serial with monitor and eats input. Use
  `-display none -serial stdio -monitor none` for piped tests.
- QEMU eats the first byte if input arrives before the device is fully
  initialized. Test scripts use `(sleep 1; printf ...) | qemu...`.

## Files
- `crates/hal-riscv64/src/plic.rs`
- `crates/kernel/src/console_in.rs`
- `crates/kernel/src/syscall.rs::{sys_read, ConsoleByte}`
