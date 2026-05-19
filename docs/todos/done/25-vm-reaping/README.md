# 25: VM reaping — `Drop for PageTable` + Proc cleanup  [DONE]

User pages, intermediate page-table pages, and trapframe pages are
returned to the global pool on `exec` and `exit`. The free-frame
count stays constant across long stretches of fork/exec/exit
cycles.

## What landed

```
kalloc: 32450 free frames (126 MiB)
...
$ malloctest          # heap grows to 135168 bytes (~33 pages)
malloctest: post-bulk sbrk(0) = 135168
malloctest: done
pid 2 exit(0) kalloc.free=32368
$ malloctest
pid 3 exit(0) kalloc.free=32368   # same — every heap page came back
$ malloctest
pid 4 exit(0) kalloc.free=32368
$
```

Same for 20× `echo`: 32368 → 32368 → … → 32368. Zero drift.

Before this todo: ~2 frames leaked per fork/exec/exit, plus all of
the heap on every `malloctest`.

## Files

- `crates/hal-riscv64/src/pagetable.rs`:
  - `static FREE_FRAME: AtomicPtr<()>` — function-pointer slot for
    the global frame-free callback. Avoids the fat-pointer-in-
    AtomicPtr problem.
  - `pub fn install_free_frame(unsafe fn(usize))` — kernel calls
    this once at boot.
  - `impl Drop for PageTable` → recursive `free_subtree`:
    - frees leaves with `PTE_U` set (user data / heap / stack)
    - skips non-user leaves (TRAMPOLINE = kernel-owned RX,
      TRAPFRAME = owned by `Proc` directly)
    - frees every intermediate L1/L0 table, then the root
- `crates/hal-riscv64/src/lib.rs` — re-export `install_free_frame`.
- `crates/kernel/src/main.rs`:
  - `kernel_free_frame(pa)` shim that calls
    `KFRAMES.free(pa)` (the kernel can't directly hand
    hal-riscv64 a `&KFRAMES` because the crate boundary doesn't
    know that type).
  - Wires the shim immediately after `kalloc::init`, before any
    user pagetable could possibly be dropped.
- `crates/kernel/src/proc.rs`:
  - `impl Drop for Proc` — frees `trapframe_pa` (mapped without
    `PTE_U` so the page-table reaper leaves it alone).
  - `proc_main` now **returns** when the proc becomes a zombie
    (was previously parked on `pending().await`, which held the
    proc Arc forever — that's why the trapframe wasn't being
    freed). With the return, the executor sees `Poll::Ready(())`
    and clears the task slot; the captured `Arc<Proc>` drops, and
    once the parent's `wait` reaps the child from `parent.children`
    the refcount hits zero and `Drop for Proc` runs.
- `crates/kernel/src/syscall.rs::sys_exit_inner`:
  - Tears down the user pagetable eagerly (replaces with a fresh
    empty one). The old `PageTable` value is dropped right there,
    triggering the recursive reap. This is what lets a child's
    heap come back to the parent's free pool *before* `wait`
    runs.
  - Logs `kalloc.free=N` on every exit so we can verify drift at
    a glance.

Total: ~70 LoC kernel + ~60 LoC hal. No new unsafe boundaries —
just the existing `free_frame` callsites.

## Design notes

- **Why a function pointer, not `&dyn FrameAllocator`?** Storing a
  trait-object reference in an atomic needs splitting the fat
  pointer into (data, vtable). Two-word atomics aren't portable;
  one-word atomics need ad-hoc reconstruction. A plain function
  pointer (`unsafe fn(usize)`) goes through `AtomicPtr<()>`
  cleanly and the kernel-side shim is one line.
- **Why empty replacement pt in `sys_exit`?** Just dropping the
  proc's pagetable would leave it in an indeterminate state if any
  later code touched `proc.pagetable`. Replacing with a fresh
  one-frame empty PT keeps the invariant that the field always
  holds a valid tree.
- **Why does dropping a zombie's pagetable not race with the user
  return path?** When `sys_exit` runs, the proc is in async-syscall
  context — it's not about to return to user mode. After `sys_exit`,
  `proc_main` returns immediately (zombie check), so `UserMode::poll`
  is never re-entered.
- **The kernel's own pagetable is safe.** `vm::init_and_install`
  calls `core::mem::forget(pt)`, so `Drop for PageTable` never
  runs on it.

## Verified at

- 20× sequential `echo run-N` → free count = 32368 every time.
- 3× sequential `malloctest` (~33 pages of heap per run) → free
  count = 32368 every time.
