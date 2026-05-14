# 04: async executor + first user mode

**Done.** `trampoline.S`, `TrapFrame` struct, user pagetable construction,
`rust_usertrap`, `initcode` that does `SYS_write` and `SYS_exit`.

## Notes
- First time the kernel goes U → S → U.
- The address bug: TRAPFRAME = `0x3fffffe000` (not `0x3ffffff000`,
  which is TRAMPOLINE). Lost an hour to that.
- Sync handler initially (Phase 4a); async refactor was 4b+5a.

## Files
- `crates/hal-riscv64/asm/trampoline.S`
- `crates/hal-riscv64/src/trapframe.rs`
- `crates/kernel/src/usertrap.rs`
- `crates/kernel/src/syscall.rs`
- `crates/kernel/user/initcode.S`
