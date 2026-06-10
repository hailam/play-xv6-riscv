# 17: Kernel correctness audit + concurrency/robustness fixes

**Status:** DONE (2026-06-10)
**Scope:** Full-kernel correctness audit (one deep-dive + three parallel
subsystem audits covering every `.rs` file, incl. a complete panic-site
inventory), then fixes for everything blocking the usertests gate.
**Result:** Full xv6 `usertests` suite — **ALL TESTS PASSED on riscv64
AND aarch64** (`-smp 1`), from a 56/69 starting point. ~20 files,
+750/−287.

## The bug classes found and fixed

### 1. Lost-wakeup class (the hang cluster: fourfiles/forkforkfork/createdelete/manywrites/nowrite)
Single-slot `WakerCell` used where MULTIPLE tasks park — `register`
overwrites the previous waker, `wake` reaches only the last registrant,
everyone else parks forever:
- `bio.rs` `Buffer.io_waker` (N tasks missing on the same block)
- `fs/inode.rs` `lock_waker` (the ilock sleeplock — siblings creating
  in one directory)
- `file.rs` pipe `read_waker`/`write_waker`
- `console_in.rs` `READER`
- `fs/log.rs` `COMMIT_WAKER` (fixed earlier, same class)

Fix: `WakerList` (multi-waiter, `wake_all`, `will_wake` dedup) in
`wait.rs`; all five sites converted.

### 2. begin_op livelock (fourfiles/manywrites, found by instrumentation)
`WaitCommit::poll`'s readiness test ignored `outstanding`, while
`begin_op`'s admission includes it — a parked op woke instantly,
failed admission, re-parked... synchronously Ready every iteration, so
the loop NEVER yielded and on a cooperative single hart the in-flight
ops never reached `end_op`. 216k spins observed. Fix: poll mirrors the
admission test exactly.

### 3. Panic-instead-of-wait (exposed once the wakeups worked)
- virtio: NUM=8 descriptors = max 2 in-flight requests; the 3rd got
  `NoFreeDescriptor` → `bread` panicked. Now parks on `DESC_WAITERS`
  (woken from `finish`), xv6's sleep-on-`disk.free` equivalent.
- bio: all-buffers-held panicked (`pick_evict_slot().expect`). Now
  yield-retries (`yield_now`); NBUF 32 → 64 (a committing log pins up
  to 30).

### 4. Kernel heap: bump allocator never freed (reparent2's 800 forks → OOM panic)
Replaced `heap.rs` wholesale: first-fit, address-ordered free list
with two-way coalescing over the same 16 MiB arena; 16-byte size
headers; SpinLock(irq-off)-guarded so it's safe from task and IRQ
context on any hart. `stats()` exposes used/peak.

### 5. Open-file-description semantics (sharedfd)
`fork`, `dup`, `dup2` deep-cloned `File` → private offsets (POSIX:
shared description). Now all duplication paths share the `Arc<File>`
(`File` no longer implements `Clone`); pipe end counts live purely in
`Drop for File` (= xv6's `struct file` refcount). Offset
read-modify-write moved INSIDE the inode lock in `inode_read`/
`inode_write` so concurrent sharers serialize; dup2 keeps `nonblock`.

### 6. User-len-driven kernel allocations (argptest panic)
`read/write/pread/pwrite` did `vec![0u8; len]` with raw user len
(`read(fd, p, -1)` = 2^64 alloc → panic). Reads clamp to file size
under the inode lock before allocating; writes stage through a fixed
`WRITE_CHUNK` buffer.

### 7. Unsplit write transactions (log-overflow assert)
One `write()` logged its whole byte count in a single `begin_op`
transaction (xv6 chunks at `((MAXOPBLOCKS-1-1-2)/2)*BSIZE`). Writes
(and pwrite) now loop per-chunk transactions.

### 8. Page-fault handler ignored the access type (nowrite hang)
A store to a mapped-but-readonly page (VA 0 = R-X code) was classified
"heap, already mapped → handled" → re-executed forever. The fault's
`write` flag now rides `TrapEvent::PageFault`; mapped-with-
insufficient-perms faults kill the proc.

### 9. Lazy-sbrk interactions (lazy_copy/lazy_unmap)
- fork aborted on never-faulted lazy holes (`translate(va)?`); now
  skips them (child stays lazy).
- sbrk over-shrink returned -1; xv6 no-ops returning the old break.

### 10. OOM frame leaks (execout's "lost some free pages")
ELF loader + stack builder leaked the freshly allocated frame when
`map` failed; fork leaked the trapframe on every early-out. All freed
on the failure paths now (fork restructured as `fork_from_inner`).

### 11. Smaller lifecycle fixes
- bitmap `balloc/bfree` RMW now under `BITMAP_LOCK` (was a torn-update
  double-allocation risk at SMP).
- `>14`-char path components: lookup/create now truncate like xv6
  (`fourteen`); reject removed from `create_at_path`.
- `wait` validates the status pointer BEFORE reaping (failing after
  lost the exit status forever); reaped subtree now dropped OUTSIDE
  the children lock (was a long irq-off section).
- `task_id` published before the task is enqueued (SMP lost-wake);
  `current_proc` cleared when a task completes (dangling raw pointer).
- `sys_pipe` error path closes both fds; `pipe_read` returns the
  partial count on copy-out failure.

### usertests port constants (the tests, not the kernel)
- `USERSTACK` 1 → 8 (our fixed 8-page stack), `HEAPTOP` define (break
  cap is `TRAPFRAME - 9*PGSIZE`: 8 stack pages + 1 guard) — used by
  `stacktest`/`lazy_sbrk`.
- `lazy_copy`'s `bad[]` addresses moved off our mapped stack pages.
- `MAXVA` is now per-arch (`1<<47` aarch64, `1<<38` riscv Sv39).

## Verification
- riscv64 `-smp 1`: full suite, **ALL TESTS PASSED** (~52 s).
- aarch64 `-smp 1`: full suite, **ALL TESTS PASSED** (~21 s).
- `-smp 3`: hangs early (≈`truncate3`) — documented as the starting
  point of [19-smp-hardening](../../pending/19-smp-hardening/).
- Audit corrections recorded: riscv `translate` DOES bound-check MAXVA
  (earlier claim retracted); aarch64 `unmap_page` is fully implemented
  (15-todo's "skeleton" note was stale).

## Key files
`wait.rs` (WakerList + yield_now), `heap.rs` (free-list allocator),
`driver/bio.rs`, `driver/virtio_blk.rs`, `fs/log.rs`, `fs/inode.rs`,
`fs/bmap.rs`, `fs/dir.rs`, `file.rs`, `console_in.rs`, `proc.rs`,
`usertrap.rs`, `executor.rs`, `elf.rs`, `user_vm.rs`, `syscall.rs`,
`user/usertests.c`.
