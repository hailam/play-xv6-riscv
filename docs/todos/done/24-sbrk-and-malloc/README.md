# 24: sbrk + user malloc  [DONE]

User programs have a heap.

## What landed

```
$ malloctest
malloctest: small alloc — ABCDEFGHIJKLMNOPQRSTUVWXYZ...UVWXYZABCDEFGHIJKLMNOPQRSTU
malloctest: pre-bulk sbrk(0) = 69632
malloctest: bulk stamps survived
malloctest: post-bulk sbrk(0) = 135168
malloctest: done
pid 2 exit(0)
```

- 100-byte alloc → pattern write → readback OK.
- `sbrk(0)` after small alloc returned 69632 (heap grew 17 pages on
  the first `morecore`).
- 32 × 2 KiB chunks (64 KiB) with start+end stamps — all stamps
  survived → no overlap, no clobber.
- Heap grew to 135168 (16 more pages) for the bulk allocation.
- Fragment-and-refill test passed (frees every other chunk, mallocs
  them back, then frees the rest).

## Files

- `crates/kernel/src/syscall.rs`:
  - `sys_sbrk(n)` — returns OLD break. Growth allocates fresh
    zero-filled frames and maps them URW, capped to keep one
    guard page between the heap top and the user stack. Shrink is
    metadata-only for now (pages aren't unmapped — that's
    `pending/09-vm-reaping`).
- `crates/kernel/user/umalloc.c` — direct port of xv6's K&R
  first-fit allocator. Linked into every C user binary.
- `crates/kernel/user/malloctest.c` — the demo above.
- `crates/kernel/build.rs`:
  - Compile `umalloc.o` once and pass it as a runtime object
    alongside `ulib.o` for every C binary.
  - `-fno-builtin` so gcc doesn't try to match `malloc`/`free`
    against its built-in libc signatures (warning-only, but
    irritating).

Total: ~50 LoC kernel, ~75 LoC user `umalloc.c` (ported), ~85 LoC
user `malloctest.c`. No new unsafe.

## Design notes

- **Growth path**: allocate one frame at a time inside a single
  pagetable-lock critical section. Cheap and simple; the per-CPU
  magazine in `kalloc` keeps the allocation fast.
- **Stack/heap collision** is prevented by reserving one guard
  page below `STACK_VA_BASE`. The user gets `-1` from `sbrk` if
  growth would breach it, and umalloc surfaces that as a `NULL`
  return from `malloc`.
- **`fork` already copies the heap**: `Proc::fork_from` iterates
  `0..proc.size` and copies every page, so a child inherits the
  exact heap contents at fork time.
- **Partial-allocation leak**: if `sys_sbrk` runs out of frames
  mid-growth, we return `-1` without rolling back the pages we
  already mapped. That's a leak; `09-vm-reaping` will close it.

## Verified at

`make fs.img && qemu-system-riscv64 ... < <(echo malloctest)`
