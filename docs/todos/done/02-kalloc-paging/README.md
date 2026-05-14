# 02: kalloc + Sv39 paging

**Done.** Physical frame allocator (linked-list freelist over `[_end, PHYSTOP)`,
spinlock-protected), Sv39 page tables, M→S transition in `mstart`,
identity-mapped kernel pagetable installed on every hart.

## Notes
- ~127 MiB free after kernel + boot data.
- Sv39 satp encoding: `mode(4=8) | ASID(16=0) | PPN(44)`.
- `mstart` delegates everything to S-mode and `mret`s to `kmain`.

## Files
- `crates/kernel/src/kalloc.rs`
- `crates/kernel/src/vm.rs`
- `crates/hal-riscv64/src/pagetable.rs`
- `crates/hal-riscv64/src/start.rs`
