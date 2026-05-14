# 12: ELF loader + multi-page user images

**Done.** Hand-rolled ELF64 parser, separate stack page at fixed VA
below TRAPFRAME, build system keeps `.elf` (stripped) instead of
`.bin`. Stack and code can't collide.

## Notes
- Each PT_LOAD segment is allocated independently with proper perms.
- `STACK_VA_BASE = TRAPFRAME - PGSIZE`. argv goes near the top of this
  stack page.

## Files
- `crates/kernel/src/elf.rs`
- `crates/kernel/src/user_vm.rs`
- `crates/kernel/build.rs` (objcopy --strip-all instead of -O binary)
