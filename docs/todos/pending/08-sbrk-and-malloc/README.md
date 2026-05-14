# 08: sbrk + user malloc

**Status:** Pending
**Estimated:** ~100 LoC
**Depends on:** —
**Unblocks:** non-trivial user programs (anything that wants a heap)

## Why

User binaries currently only have stack + static BSS. No heap. xv6's
`sbrk` grows the data segment; user-space `malloc.c` arenas live on
top.

## Approach

### Kernel side: `sys_sbrk`

```rust
async fn sys_sbrk(proc: &Arc<Proc>, n: i64) -> i64 {
    let old = proc.size.load(Ordering::Acquire);
    if n == 0 { return old as i64; }
    if n < 0 {
        // Shrink: unmap pages, free frames. Defer (return error or no-op).
        return -1;
    }
    let new = old.checked_add(n as usize)?;
    let start = (old + PGSIZE - 1) & !(PGSIZE - 1);
    let end = (new + PGSIZE - 1) & !(PGSIZE - 1);
    let mut pt = proc.pagetable.lock();
    let mut va = start;
    while va < end {
        let pa = KFRAMES.alloc_zeroed()?;
        if pt.map(va, pa, PGSIZE, PtePerm::URW, &KFRAMES).is_err() {
            return -1;  // partial allocation; should unwind
        }
        va += PGSIZE;
    }
    drop(pt);
    proc.size.store(new, Ordering::Release);
    old as i64
}
```

xv6 semantics: `sbrk(n)` returns the OLD break (byte address); the
caller treats `[old, old+n)` as the freshly-allocated region.

Already-allocated page (when `old` isn't page-aligned) is reused; new
pages start at `start = ceil(old / PGSIZE)`.

### User side: tiny malloc

Port xv6's `user/umalloc.c` (~80 LoC of K&R-style first-fit allocator).
Translates to:

```c
typedef long Align;
union header {
    struct { union header* ptr; uint size; } s;
    Align x;
};
static union header base;
static union header* freep;

void* malloc(uint nbytes);
void  free(void* ap);
```

Calls `sbrk(n)` when out of heap.

### ulib wiring

```asm
DEFINE_SYSCALL sbrk, 12
```

In a new `user/umalloc.c`, include via the build.

## Verification

A new `/malloctest` user binary:

```c
int main(void) {
    char* p = malloc(100);
    for (int i = 0; i < 100; i++) p[i] = 'A' + (i % 26);
    p[99] = 0;
    write(1, p, 99);
    write(1, "\n", 1);
    free(p);
    return 0;
}
```

Run via shell; observe the 99-byte ABC...Z pattern.

## Risks

- `fork`'s page copy must include the heap. Currently `fork_from`
  iterates `0..size`. If `sbrk` extended `size`, those pages are
  included automatically. Verify.
- VM reaping (`09-vm-reaping`): on `exit`, freeing the heap pages is
  important to avoid leaks. Currently we leak; that's `09`.

## Code touch points

- `crates/kernel/src/syscall.rs` — add `sys_sbrk`
- `crates/kernel/src/uapi.rs` — `SYS_SBRK = 12` already exists
- `crates/kernel/user/ulib.S` — add `sbrk` wrapper
- New: `crates/kernel/user/umalloc.c` — ported from xv6
- `crates/kernel/build.rs` — compile umalloc as part of every C user binary (or link only when needed)
- New: `crates/kernel/user/malloctest.c` for the demo
