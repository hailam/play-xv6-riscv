# 43 — Per-CPU Page Magazines & $O(1)$ Size-Class Slab Allocator

## Problem statement

Both the physical page allocator (`crates/kernel/src/kalloc.rs`) and the kernel heap allocator (`crates/kernel/src/heap.rs`) serialize all CPU cores on a single global `SpinLock`:
1. **`kalloc.rs` (`FrameAllocImpl`, lines 55–108):**
   - Uses a single global `SpinLock<Inner>` protecting an intrusive singly linked list of 4 KiB pages.
   - `add_range` (line 80) zeroes all 32,768 pages synchronously at boot (`ptr::write_bytes(..., 0, PGSIZE)`), and then `alloc_zeroed` (line 100) zeroes the entire 4 KiB page *again* on every allocation.
   - Under SMP (`-smp 4`), concurrent page faults, `fork`, `exec`, and page-table allocations contend heavily on `KALLOC.inner`.
2. **`heap.rs` (`ListAlloc`, lines 57–187):**
   - Uses a single global `SpinLock<HeapState>` guarding an address-ordered singly linked free list over a 16 MiB static buffer (`HEAP_BUF`).
   - Both `alloc` (first-fit traversal, lines 115–152) and `dealloc` (sorted insertion + neighbor coalescing, lines 155–186) are $O(N_{\text{free}})$ linear scans with interrupts disabled!
   - Because `syscall::dispatch` boxes a `Future` (`Box::pin(...)`) on every syscall and `WakerList` / `Vec` / `Arc` allocate from `HEAP`, every syscall on every core contends on `HEAP.state`.

## Discovery & research checklist

1. **Per-CPU Physical Page Magazines (`crates/kernel/src/kalloc.rs`):**
   - Maintain a per-CPU magazine (`[PerCpuMagazine; MAX_CPUS]`) indexed by `Arch::cpu_id()` while interrupts are disabled (`push_off` / `pop_off`).
   - Each `PerCpuMagazine` holds a bounded stack of free physical frame addresses (e.g., `frames: [usize; 64]`, `len: usize`).
   - **Fast path (`alloc_raw` / `dealloc_raw`):** Disable interrupts (`push_off`), pop/push directly from `magazines[cpu_id]` with **zero shared lock acquisition** ($O(1)$, ~5 instructions).
   - **Slow path (refill / drain):** When a magazine is empty on `alloc`, lock the global `KALLOC` once and batch-pop 32 frames into the local magazine. When full on `dealloc`, batch-push 32 frames back to the global list.
   - **Eliminate redundant boot zeroing:** Either skip `write_bytes` in `add_range` at boot (since `alloc_zeroed` zeroes on demand with caches warm right before use) or track pre-zeroed status.
2. **Per-CPU Size-Class Slab Cache + Buddy/Page Backend (`crates/kernel/src/heap.rs`):**
   - Define power-of-two size classes: `16, 32, 64, 128, 256, 512, 1024, 2048` bytes (covering `Arc<Proc>`, `Arc<Inode>`, `Waker`, boxed syscall `Future`s, `PipeInner`, etc.).
   - Give each CPU (`MAX_CPUS = 8`) a per-class intrusive free list (`per_cpu_slabs[cpu_id][class_idx]`).
   - **Fast path (`alloc` / `dealloc` for `size <= 2048`):** With interrupts disabled (`push_off`), pop/push the head pointer from `per_cpu_slabs[cpu_id][class_idx]` in $O(1)$ time with zero cross-CPU lock contention.
   - **Slab refill:** Carve a 4 KiB page from `kalloc` (or `HEAP_BUF` backing arena) into `4096 / class_size` objects in one pass.
   - **Large allocations (`size > 2048`):** Fall back to a page-granularity buddy allocator or the existing coalescing arena lock (which is now rarely hit).

## Verification

- All existing `kalloc` and `heap` unit tests (`test_alloc_free_roundtrip`, `test_vec_and_box_work`) pass.
- `usertests` (especially `forkfork`, `forkforkfork`, `manywrites`, `sbrkbasic`, `mmap`) passes under `-smp 4` on both `riscv64` and `aarch64`.
- `bench` SMP parallel page-fault / `fork` / syscall microbenchmarks show near-linear scaling from 1 to 4 cores.
