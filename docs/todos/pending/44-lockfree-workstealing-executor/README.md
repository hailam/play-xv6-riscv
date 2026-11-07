# 44 — Lock-Free Work-Stealing Executor, Inline Syscall Futures & Intrusive Wakers

## Problem statement

The kernel's async-first architecture (`crates/kernel/src/executor.rs`, `crates/kernel/src/proc.rs`, `crates/kernel/src/wait.rs`) avoids 1:1 kernel-thread stacks, which is a major architectural advantage over Linux. However, four implementation bottlenecks currently add unnecessary lock and heap overhead to every task poll and wakeup:
1. **Three `SpinLock` acquisitions per single task poll (`crates/kernel/src/executor.rs` lines 191–231):**
   - `poll_one()` locks `exec.ready` (`SpinLock<VecDeque<TaskId>>`) to pop `id`, then locks `exec.tasks` (`SpinLock<Vec<Option<Task>>>`) to `.take()` the `Task` out of its slot, polls the future, and locks `exec.tasks` a second time to put the `Task` back (`slot = Some(task)`).
2. **Heap-allocated `Box::pin` future on every syscall (`crates/kernel/src/syscall.rs` line 253, `crates/kernel/src/proc.rs` line 552):**
   - Every user trap that invokes a syscall calls `syscall::dispatch(proc)`, which returns `Pin<Box<dyn Future<Output = ()> + Send + '_>>`, allocating and freeing a heap block on every single `ecall`/`svc`!
   - Furthermore, fast synchronous syscalls (`getpid`, `dup`, `dup2`, `lseek` on framebuffer, `sbrk`, `close`, `clock_gettime`, non-blocking ready reads/writes) still pay for `Box::pin` + async poll machinery.
3. **Static spawn-time CPU placement (`crates/kernel/src/executor.rs` lines 118–137):**
   - `pick_home_cpu()` assigns `home_cpu` once at `spawn()`. If two CPU-bound or I/O-heavy processes land on hart 0 while harts 1–3 are idle in `wfi`, harts 1–3 never steal ready tasks from hart 0.
4. **Heap-churning `WakerList` (`crates/kernel/src/wait.rs` lines 48–75) & per-byte pipe wakeups (`crates/kernel/src/syscall.rs` lines 1895–2064):**
   - `WakerList::wake_all()` uses `core::mem::take(&mut *self.inner.lock())`, which drops the `Vec<Waker>` buffer after waking so the very next `register()` must allocate a new `Vec` on the kernel heap!
   - `pipe_write` and `pipe_read` (`crates/kernel/src/syscall.rs` lines 1895–2064) transfer 1 byte per iteration and call `wake_all()` on **every byte**.

## Discovery & research checklist

1. **Synchronous Fast-Path Syscall Dispatch (`crates/kernel/src/usertrap.rs`, `crates/kernel/src/syscall.rs`):**
   - Before allocating or polling an async `Future`, add `syscall::try_fast_dispatch(proc: &Arc<Proc>) -> Option<i64>` for syscalls that complete without `.await` (`SYS_GETPID`, `SYS_SBRK`, `SYS_DUP`, `SYS_DUP2`, `SYS_CLOSE`, non-blocking pipe/socket/console hits, `clock_gettime`, `uname`, etc.).
   - For async syscalls, store the syscall future in a reusable per-`Proc` inline storage buffer (or cache the `Box` allocation inside `Proc`) so syscalls perform **zero heap allocations**.
2. **Zero-Take Task Slot Polling & Chase-Lev Work-Stealing Queue (`crates/kernel/src/executor.rs`):**
   - Instead of storing `Option<Task>` in a shared `SpinLock<Vec<Option<Task>>>` and taking/replacing it twice per poll, allocate each `Task` as an `Arc<TaskNode>` containing an atomic state (`IDLE`, `QUEUED`, `RUNNING`, `NOTIFIED`, `COMPLETED`) and an `UnsafeCell<Pin<Box<dyn Future<Output = ()> + Send>>>` (guaranteed exclusive to the single hart that transitions `QUEUED -> RUNNING`).
   - Replace the global/per-CPU `SpinLock<VecDeque<TaskId>>` with a fixed-capacity lock-free ring buffer (Chase-Lev work-stealing deque) per CPU.
   - When a hart's local ready queue is empty before entering `wfi`, scan peer harts and steal half of a busy hart's ready queue, sending an IPI when waking an idle peer.
3. **Zero-Allocation `WakerList` (`crates/kernel/src/wait.rs`):**
   - At minimum, retain capacity across `wake_all()` (e.g., ` SmallVec<[Waker; 4]>` or inline fixed array `[Option<Waker>; 8]` + overflow `Vec` that is `.drain(..)`ed in place or swapped with a thread-local scratch `Vec` instead of `mem::take`d and dropped).
   - Even better: intrusive wait-queue nodes embedded directly inside the waiting `Future` on the task stack, eliminating both `Waker::clone` heap traffic and `Vec` growth.
4. **Page/Slice-Batched Pipe Ring Buffer (`crates/kernel/src/file.rs`, `crates/kernel/src/syscall.rs`):**
   - Replace `PipeInner.buf: SpinLock<VecDeque<u8>>` (`PIPE_SIZE = 512`) with a 16 KiB fixed ring buffer (`[u8; 16384]`, `head: usize`, `tail: usize`).
   - Update `pipe_write` and `pipe_read` (`crates/kernel/src/syscall.rs` lines 1895–2064) to copy whole contiguous slices (`copy_from_user` / `copy_to_user`) per lock acquisition and call `wake_all()` **once** per batch instead of once per byte.

## Verification

- All executor unit tests and `usertests` pass on `-smp 4` across `riscv64` and `aarch64`.
- `bench` null-syscall latency (`getpid`) executes via the synchronous fast path with 0 heap allocations and 0 executor lock acquisitions.
- `bench` pipe throughput (64 KiB streaming) improves by $>50\times$, and 4-process CPU/IO imbalance tests show automatic work stealing across all 4 harts.
