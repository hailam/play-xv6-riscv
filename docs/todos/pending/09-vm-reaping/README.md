# 09: VM reaping — `Drop for PageTable`

**Status:** Pending
**Estimated:** ~80 LoC
**Depends on:** —
**Unblocks:** long-running shells (no leak on exec/exit)

## Why

Currently `sys_exec`'s `proc.replace_image(new_pt, ...)` swaps the
pagetable but the old `PageTable` is dropped with a no-op Drop. The
intermediate page-table pages and user data pages leak. Same for
`sys_exit`.

For our 128 MiB demo this is invisible (we never run out). For a
long-running system, fork+exec eventually exhausts memory.

## Approach

Implement `Drop for hal_riscv64::PageTable` that walks the tree and
frees:

1. All leaf pages with `PTE_V && PTE_U` (user data pages).
2. All intermediate page-table pages (the root, L2, L1 nodes).
3. Do NOT free TRAMPOLINE or TRAPFRAME mappings (they're shared / owned
   by the proc, not the pagetable).

```rust
impl Drop for PageTable {
    fn drop(&mut self) {
        // Walk all entries in the root and recurse.
        unsafe { unmap_tree(self.root_pa, 2); }
        // Then free the root itself.
        unsafe { KFRAMES.free(self.root_pa); }
    }
}

unsafe fn unmap_tree(pt_pa: usize, level: u32) {
    let pt = pt_pa as *mut [Pte; 512];
    for i in 0..512 {
        let pte = (*pt)[i];
        if !pte.is_valid() { continue; }
        if pte.is_leaf() {
            // Skip "shared" mappings (TRAMPOLINE — recognized by va high address).
            // Otherwise free the leaf page.
            // ... (need a way to know which leaves to free)
        } else if level > 0 {
            // Recurse into next-level table; free it after.
            unmap_tree(pte.pa(), level - 1);
            KFRAMES.free(pte.pa());
        }
    }
}
```

### How do we know which leaves to free?

Two options:

A. **VA discrimination.** Walk only `[0, USER_MAX_VA)` (skip TRAPFRAME
   and TRAMPOLINE). Free everything else. Requires the walker to know
   VAs; recursive walk doesn't. Make it iterative with VA tracking.

B. **Track per-proc free-list.** `Proc` records which frames it owns;
   on drop, free them. Cleaner abstraction but more bookkeeping.

xv6 uses (A): `uvmunmap(pagetable, va, npages, do_free)` and only
unmaps user pages, then `freewalk` walks the tree and frees
intermediate pages (asserting no remaining leaves).

We do the same: in `Proc::drop` (or `replace_image`), call a helper
`free_user_pagetable(pt, code_end)` that:

1. `uvmunmap(pt, 0, code_end / PGSIZE, true)` — unmap + free code/data
2. `uvmunmap(pt, STACK_VA_BASE, 1, true)` — unmap + free stack
3. `uvmunmap(pt, TRAMPOLINE, 1, false)` — unmap but DON'T free (shared)
4. `uvmunmap(pt, TRAPFRAME, 1, false)` — unmap but DON'T free (owned by Proc separately)
5. `freewalk(pt)` — recursive walk freeing intermediate tables

## Verification

- Track `KFRAMES.free_count()`.
- Fork + exec + exit 100 times.
- Free count should return to the initial value (~32400).

Without the fix, free count decreases by ~3-5 frames per exec/exit.

## Risks

- The trapframe page is allocated by `kalloc` but tracked separately
  (in `Proc.trapframe_pa`). On `Proc::drop`, free it explicitly,
  separately from pagetable reaping.
- `replace_image` must call this BEFORE swapping. After swap, the old
  pagetable is in a local `let old = ...; drop(old);` — Drop runs there.
- Need to be careful with intermediate-table sharing: in xv6, none of
  the intermediate tables are shared, so freewalk just frees them all.
  TRAMPOLINE's intermediate path might share an L1 page with user
  mappings? Sv39 with TRAMPOLINE at MAXVA-PGSIZE: its L2/L1 are NOT
  shared with low user VAs. Confirm.

## Code touch points

- `crates/hal-riscv64/src/pagetable.rs` — add `Drop for PageTable` + `uvmunmap` + `freewalk`
- `crates/kernel/src/proc.rs::Proc::drop` — free `trapframe_pa`
- Possibly add a `proc.size` → `code_end` rename for clarity
