# 30: Reclaim `munmap` virtual address holes in `Proc` VMA allocator

**Status:** Pending — needs discovery, research & verification
**Scope:** `crates/kernel/src/proc.rs`, `crates/kernel/src/syscall.rs`, `crates/kernel/src/user_vm.rs`, `crates/kernel/user/mmaptest.c`

## Problem statement

`Proc::mmap_base` (`crates/kernel/src/proc.rs`, lines 143–146) is currently a monotonic top-down bump pointer initialized to `user_vm::MMAP_TOP`. When `sys_munmap` unmaps a region, physical frames are freed and `Proc::vmas` entries are trimmed or removed, but `mmap_base` is never moved back up and freed VA intervals are never reused. A long-lived process performing repeated `mmap`/`munmap` cycles will eventually exhaust the virtual address window between the heap/stack ceiling and `MMAP_TOP`.

## Discovery & research checklist

1. **Trace all VMA lifecycle paths:**
   - Audit `sys_mmap` and `sys_munmap` in `crates/kernel/src/syscall.rs`: how `vmas: SpinLock<Vec<Vma>>` is ordered, how prefix/suffix unmaps mutate an existing `Vma`, and whether unmapping the middle of a single `Vma` splits it into two `Vma` descriptors (and adjusts `file_offset` for file-backed mappings).
   - Audit `fork`, `exec`, and `exit` in `crates/kernel/src/proc.rs` and `crates/kernel/src/syscall.rs` to verify how `vmas` and `mmap_base` are cloned or reset.
   - Check lazy page-fault resolution (`handle_page_fault`) and `translate_user_perm` to ensure VMA lookup assumptions (such as sorted order or non-overlapping intervals) are preserved.
2. **Research VA allocation strategy:**
   - Compare scanning the sorted `vmas` slice top-down from `MMAP_TOP` for the first gap of size $\ge \text{len}$ (with a lower-bound guard above `proc.size` / user stack) against maintaining an explicit free-list.
   - Since `vmas` per process is small in practice, evaluate whether a linear gap scan over sorted `vmas` completely eliminates `Proc::mmap_base` state while keeping allocation $O(|vmas|)$.

## Verification

- Extend `crates/kernel/user/mmaptest.c` with:
  1. A hole-reuse test that maps regions A, B, C, unmaps B, maps D of the same size, and verifies D reuses B's virtual address range.
  2. A middle-split `munmap` test (unmapping the middle page of a 3-page mapping and verifying both outer pages remain accessible while the middle page faults).
  3. A churn loop that maps and unmaps regions across thousands of iterations (exceeding total VA space if bump-allocated) and verifies no OOM or VA exhaustion occurs.
- Run `mmaptest` and full `usertests` on both `riscv64` and `aarch64`.
