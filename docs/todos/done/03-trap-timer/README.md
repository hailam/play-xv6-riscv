# 03: trap plumbing + Sstc timer

**Done.** `kernelvec.S` (S-mode trap entry), `rust_kerneltrap`,
supervisor timer via Sstc (`stimecmp` written from S-mode after M-mode
enables `menvcfg.STCE`). Ticking dots verify.

## Notes
- Sstc avoids the xv6 M-mode timer trap relay. Cleaner if available.
- `mstart` enables `menvcfg.STCE` and grants `mcounteren.TM`.
- Each timer fire re-arms `stimecmp` for the next interval.

## Files
- `crates/hal-riscv64/asm/kernelvec.S`
- `crates/hal-riscv64/src/trap.rs`
- `crates/kernel/src/trap.rs`
