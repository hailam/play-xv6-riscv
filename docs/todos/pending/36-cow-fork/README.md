# 36: Copy-on-Write (CoW) `fork` and physical frame reference counting

**Status:** Pending — Phase 3 foundation (needs discovery, research & verification)
**Scope:** `crates/kernel/src/kalloc.rs`, `crates/hal/src/lib.rs`, `crates/hal-riscv64/src/pagetable.rs`, `crates/hal-aarch64/src/pagetable.rs`, `crates/kernel/src/proc.rs`, `crates/kernel/src/syscall.rs`

## Problem statement

`Proc::fork` currently allocates a fresh physical frame and copies 4 KiB for every mapped user page in the parent's address space. While acceptable for tiny 4–16 KiB xv6 binaries, open-source Unix binaries (`uutils/coreutils`, `BusyBox`, `lua`, `bc`, shells) are 200 KiB to 4 MiB+. When a shell pipeline forks 5 times before `execve`, eager page copying wastes megabytes of RAM and CPU time copying pages that `execve` immediately frees.

## Discovery & research checklist

1. **Physical frame reference counts (`kalloc.rs`):**
   - Design a compact refcount array indexed by frame number `(pa - KERNBASE) / PGSIZE` (`32768` frames for 128 MiB RAM $\Rightarrow$ 32 KiB of `AtomicU8` or 64 KiB of `AtomicU16`).
   - Verify every allocation and free path (`KFRAMES.alloc_zeroed`, `KFRAMES.free`, `kernel_free_frame` in `Drop for PageTable`, `Drop for Proc` trapframe, `sys_munmap`) so `free` decrements the refcount and only returns the frame to the freelist when the count reaches `0`.
2. **CoW PTE encoding in `hal::PtePerm` (`hal-riscv64` & `hal-aarch64`):**
   - Identify a software-reserved PTE bit on both architectures:
     - RISC-V Sv39: bits `[9:8]` (`RSW` — reserved for supervisor software).
     - AArch64 4-level stage-1 descriptor: bits `[58:55]` (software-use bits).
   - Expose `PtePerm::COW` in `crates/hal/src/lib.rs` and translate it in both `hal-riscv64` and `hal-aarch64` pagetable implementations.
   - Verify TLB shootdown / flush requirements (`sfence.vma` on RISC-V, `tlbi vmalle1is` on AArch64) when downgrading writable parent PTEs to `READ | USER | COW` during `fork` and when promoting/replacing a CoW page on fault.
3. **Fault and kernel `copyout` resolution:**
   - Trace `handle_page_fault` (`write = true`) and `translate_user_perm(..., write = true)` (used by `copyout` when the kernel writes to a user buffer in a syscall before user code has touched the CoW page!).
   - If `refcount == 1`, promote the PTE to writable in place without allocating or copying; if `refcount > 1`, allocate a new frame, copy 4 KiB, map with `WRITE`, and decrement the old frame's refcount.

## Verification

- Verify `usertests`'s existing `forkfork`, `forkforkfork`, and `copyin`/`copyout` tests pass under SMP (`-smp 3` / `-smp 4`).
- Add a CoW stress test where a process allocates 32 MiB of heap, forks 8 children (which would OOM a 128 MiB system under eager copy), and verifies isolated writes in parent and children plus flat free-frame count after exit.
