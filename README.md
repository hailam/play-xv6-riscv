# play-xv6-riscv

A Rust rewrite of [xv6-riscv](https://github.com/mit-pdos/xv6-riscv)
with two unusual architectural choices:

1. **Async-first kernel concurrency.** No `swtch.S`; each process is
   an async `Task` and every syscall is `async fn`. The per-CPU
   executor runs the async loop directly on the kernel stack — no
   per-process kernel stacks to save / restore.
2. **HAL behind a trait.** All arch-specific code lives in
   `hal-riscv64` (or `hal-aarch64`, skeleton); the kernel itself
   sees only the `hal::Hal` surface. RISC-V 64 boots and runs the
   full shell today; aarch64 is a compiling-but-not-yet-booting
   second impl that proves the trait surface is honest.

Total kernel: **~6.5 KLoC, ~158 unsafe-ish lines (~2.4%)**, all
confined to HAL / driver MMIO / frame allocator / trapframe glue.

## What works today

`make qemu` (RISC-V 64, single hart by default; `make qemu CPUS=3`
for SMP):

- Boot to a shell prompt on top of `fs.img` (built by the host-side
  `mkfs` tool).
- `ls /`, `cat /file`, `echo hi > /out`, `mkdir /dir`, `cd /dir`,
  `rm /file`, `ln /file /alias` (hard link).
- Pipelines: `/ls / | /cat`.
- Forks, `wait`, `kill <pid>` (kill cancels sleeping / blocked
  procs within tens of ms).
- `malloc` / `free` in userspace on top of `sbrk`.
- SMP: with `-smp 3`, user procs visibly run on hart 0, 1, 2.
  Sticky `home_cpu` per task at spawn time; cross-CPU wakes pick
  up on the next timer tick (IPI plumbing is deferred).
- Write-ahead log; every fs change goes through `begin_op` /
  `end_op` and is durable across reboot.
- Page-table reaping: kernel free-frame count is flat across long
  fork/exec/exit cycles (no leak).

Sample session (after a previous `mkdir /work`):

```
$ cd /work
$ /wr greet hello from cwd
$ /cat greet
hello from cwd
$ /ln greet alias
$ /ls /work
DIR   inum=17  size=64   .
DIR   inum=1   size=304  ..
FILE  inum=19  size=15   greet
FILE  inum=19  size=15   alias        <- same inum: hard link
$ cd /
$ /echo redirect-result > /tmp-out
$ /cat /tmp-out
redirect-result
```

## Build & run

Requires:
- `rustup target add riscv64gc-unknown-none-elf` (the workspace
  `.cargo/config.toml` defaults to this)
- A riscv64 cross toolchain (e.g. `riscv64-elf-gcc` from Homebrew's
  `riscv-gnu-toolchain`) for the user binaries
- `qemu-system-riscv64`

```
make fs.img     # builds the kernel, the user binaries, and the disk image
make qemu       # boots into the shell
make qemu CPUS=3  # SMP
```

Interactive Ctrl-A x to exit QEMU; or pipe stdin with a heredoc /
`sleep` script for non-interactive tests.

## Layout

```
crates/
  hal/                # trait surface — no arch code
  hal-riscv64/        # `impl Hal for Riscv64` + asm + drivers
  hal-aarch64/        # skeleton (compiles, doesn't boot yet)
  xv6-fs-layout/      # on-disk structs shared with mkfs
  kernel/             # the kernel binary itself
    src/
      main.rs         # kmain → executor::run on every hart
      executor.rs     # per-CPU async runtime, sticky home_cpu
      proc.rs         # `Proc`, async `proc_main`
      syscall.rs      # async fn sys_* + dispatch
      fs/             # superblock, log, bmap, inode, dir, path
      driver/         # UART, virtio-blk, bio (block cache)
      sync.rs         # SpinLock, WakerCell
      vm.rs           # kernel pagetable
      user_vm.rs      # ELF loader, argv layout
      ...
    user/             # C user binaries (sh, ls, cat, echo, ...)
mkfs/                 # host tool: builds fs.img
docs/todos/           # work tracking (see below)
```

## Architecture notes

### Async kernel

Each user process gets one task: `Pin<Box<dyn Future + Send + 'static>>`
in the per-CPU executor's `tasks` vec. The task's future is
`proc_main`, which loops:

```rust
async fn proc_main(proc: Arc<Proc>) {
    loop {
        let event = UserMode::run(&proc).await;   // -> Pending, sets `user_target`
        match event {
            TrapEvent::Syscall { nr } => {
                let ret = syscall::dispatch(&proc, nr).await;
                proc.trapframe().a0 = ret as u64;
            }
            TrapEvent::Timer | TrapEvent::Devintr => {}
        }
        if proc.is_zombie() { return; }
    }
}
```

`UserMode::poll` sets `cpu::user_target` and returns `Pending`. The
executor sees the target after the poll and calls
`return_to_user(proc)` — a noreturn jump through the trampoline
page into S→U mode. On the next trap, the trampoline restores
kernel SATP, jumps to `rust_usertrap`, which sets `proc.pending_trap`,
calls `executor::wake(task_id)`, and re-enters `executor::run` —
which polls the task again, sees the pending trap, and runs the
syscall body.

There's no `swtch.S`. There's no kernel stack per proc. The
"context switch" between procs is one `executor::run` iteration.

### Cancellation

`Proc::killed: AtomicBool` is checked in every blocking future's
`poll`. `sys_kill(pid)` sets the flag, wakes the relevant waker
slots, and the proc's next poll bails (returning a sentinel that
`proc_main` translates into `sys_exit(-1)`). A 10000-tick sleep
(~10s) is cancelled within ms by `kill`.

### Filesystem

xv6 fs layout, transactional via a write-ahead log:

```
block 0 | sb | log[0..30] | inodes[0..25] | bmap | data ...
```

Every mutating syscall (`mkdir`, `open(O_CREATE)`, `write`,
`unlink`, `link`, `chdir`) wraps its work in `begin_op` / `end_op`.
The log replays on boot if the previous shutdown crashed
mid-commit. Verified by running the test sequence, then rebooting
against the same `fs.img` and observing the surviving directory.

### VM reaping

`Drop for PageTable` (riscv64) walks the tree:

- Frees leaves with `PTE_U` set (user data / heap / stack).
- Skips kernel-owned leaves (trampoline RX, trapframe RW —
  those have other owners).
- Reclaims every intermediate L1/L0 table, then the root.

`Drop for Proc` returns `trapframe_pa`. `sys_exit` eagerly
replaces the proc's pagetable with an empty fresh root so the old
tree drops right there. Result: free-frame count is flat across
20 sequential fork/exec/exit cycles and across 3 sequential
`malloctest` runs (each malloctest grows the heap to ~33 pages and
returns it all on exit).

## Status

Original 12-phase plan: through phase 8 (modest perf gains:
per-CPU bcache shards, async virtio batching) is essentially
done. Two stretch items remain:

- **`14-aarch64-completion`** — fill in the aarch64 skeleton:
  scrub the kernel's direct `use hal_riscv64::*` (currently 6
  files), write trap vectors + EL2→EL1 drop + GIC v2 driver + real
  pagetable populate. Untouched in this environment because no
  `aarch64-elf-gcc` is installed.
- **`12-phase2-gui`** — the original Phase 2 stretch goal. A
  ramfb driver + `/dev/fb0` + a userspace display server. Tracked
  as ~5 sub-todos when started.

Full per-todo history is in [`docs/todos/`](docs/todos/) — see
`docs/todos/README.md` for the index, and one folder per todo
under `done/` with what landed + verification notes.

## License

MIT (mirrors upstream xv6).
