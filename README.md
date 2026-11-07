# play-xv6-riscv

A Rust rewrite of [xv6-riscv](https://github.com/mit-pdos/xv6-riscv)
with two unusual architectural choices:

1. **Async-first kernel concurrency.** No `swtch.S`; each process is
   an async `Task` and every syscall is `async fn`. The per-CPU
   executor runs the async loop directly on the kernel stack — no
   per-process kernel stacks to save / restore.
2. **HAL behind a trait.** All arch-specific code lives in
   `hal-riscv64` and `hal-aarch64`; the kernel itself sees only the
   `hal::Hal` surface. Both RISC-V 64 and AArch64 boot SMP (`-smp 3` /
   `-smp 4`), run the interactive shell and GUI window server, and pass
   the full 74-test suite.

Total workspace (`crates/`): **~16.5 KLoC** (~11.5 KLoC arch-independent
kernel + ~5 KLoC across `hal`, `hal-riscv64`, `hal-aarch64`, and
`xv6-fs-layout`), with `unsafe` confined to HAL / driver MMIO / frame
allocator / trapframe glue.

## What works today

`make qemu` (RISC-V 64) or `make qemu-aarch64` (AArch64), single hart
by default (`CPUS=3` / `CPUS=4` for SMP):

- Boot to a shell prompt on top of `fs.img` / `fs-aarch64.img` (built
  by the host-side `mkfs` tool, with `init` at pid 1 and orphan
  reparenting).
- `ls /`, `cat /file`, `echo hi > /out`, `mkdir /dir`, `cd /dir`,
  `rm /file`, `ln /file /alias` (hard link), symlinks, `pwd`, `env`,
  `chmod`, `ftruncate`, `lseek`, `mmap`, `poll`, `ioctl`.
- Pipelines: `/ls / | /cat`.
- Forks, `wait`, `kill <pid>` (kill cancels sleeping / blocked procs
  within tens of ms), and POSIX signals (`sigaction`, `sigprocmask`,
  `sigreturn`, `alarm`).
- `malloc` / `free` in userspace on top of `sbrk`, plus `picolibc`-linked
  C programs including `bc`, `dc`, and `lua` (Lua 5.4).
- Networking: `AF_UNIX` sockets (with filesystem-visible bindings) and
  TCP/IP via `smoltcp` 0.12 (loopback + `virtio-net`, host↔guest TCP
  echo gated by `make test-net`).
- Display & GUI: `ramfb` framebuffer (`640×480` `XRGB8888` via `fw_cfg`
  DMA, exposed as `/dev/fb0`), `virtio-input` keyboard (`/dev/input/0`),
  and a userspace window server (`wm`) over `AF_UNIX` with demo clients
  (`hello_wm`, `clock`, `guidemo`).
- SMP: with `-smp 3` / `-smp 4`, user procs visibly run across harts.
  Sticky `home_cpu` per task at spawn time; cross-CPU wakes send real
  wake-IPIs (`Arch::send_ipi` via RISC-V CLINT MSIP→SSIP trampoline and
  AArch64 GICv2 SGI).
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
- `rustup target add riscv64gc-unknown-none-elf aarch64-unknown-none-softfloat`
- A riscv64 cross toolchain (e.g. `riscv64-elf-gcc` from Homebrew's
  `riscv-gnu-toolchain`) for RISC-V user binaries; AArch64 user
  binaries use host `clang --target=aarch64-none-elf` + rustup's `lld`
- `xmake` (fetches & builds `picolibc` on first build for `bc`/`dc`/`lua`)
- `qemu-system-riscv64` and/or `qemu-system-aarch64`

```
make fs.img              # builds the kernel, user binaries, and disk image (riscv64)
make qemu                # boots riscv64 into the shell
make qemu CPUS=3         # riscv64 SMP
make qemu-aarch64 CPUS=4 # boots aarch64 SMP into the shell
make test-net            # host<->guest TCP echo over virtio-net
make test-fb             # headless ramfb + /dev/fb0 screendump checks
make test-input          # headless virtio-input QMP send-key check
make test-gui            # headless wm + hello_wm + clock compositing & key routing
```

Interactive Ctrl-A x to exit QEMU; or pipe stdin with a heredoc /
`sleep` script for non-interactive tests.

## Layout

```
crates/
  hal/                # trait surface — no arch code
  hal-riscv64/        # `impl Hal for Riscv64` + asm + drivers (CLINT/PLIC/UART)
  hal-aarch64/        # `impl Hal for AArch64` + asm + drivers (GICv2/PL011/PSCI)
  xv6-fs-layout/      # on-disk structs shared with mkfs
  kernel/             # the kernel binary itself
    src/
      main.rs         # kmain → executor::run on every hart
      executor.rs     # per-CPU async runtime, sticky home_cpu + wake-IPIs
      proc.rs         # `Proc`, async `proc_main`
      syscall.rs      # async fn sys_* + dispatch
      net.rs          # smoltcp loopback + virtio-net TCP/IP stack + AF_UNIX
      fs/             # superblock, log, bmap, inode, dir, path
      driver/         # virtio-blk, virtio-net, virtio-input, ramfb, bio
      sync.rs         # SpinLock, WakerCell
      vm.rs           # kernel pagetable
      user_vm.rs      # ELF loader, argv layout
      ...
    user/             # C user binaries (sh, ls, cat, wm, usertests, ...)
mkfs/                 # host tool: builds fs.img / fs-aarch64.img
third_party/          # picolibc meson cross files + bc/libc glue
scripts/              # headless QEMU/QMP integration tests (net, fb, input, gui)
xmake.lua             # builds picolibc (libc.a + libm.a) for riscv64 & aarch64
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
                proc.trapframe().set_arg(0, ret as u64);
            }
            TrapEvent::PageFault { va, write } => { /* lazy / mmap fault */ }
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

`Drop for PageTable` (`riscv64` and `aarch64`) walks the tree:

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

All 37 planned todos across Phases 1 and 2 are complete (`done/`: 37,
`pending/`: 0):

- **Dual-arch SMP (`14-aarch64-completion`, `19-smp-hardening`)** —
  Both `riscv64` and `aarch64` boot SMP (`-smp 3` / `-smp 4`) with
  hardware wake-IPIs and pass the full 74-test suite (`usertests` +
  POSIX/xv6 regression binaries).
- **POSIX & C ecosystem (`15-xv6-compat`, `16-posix-compat`, `20-fs-console-robustness`)** —
  POSIX signals (`sigaction`, `sigprocmask`, `sigreturn`, `alarm`),
  `mmap`/`munmap`, symlinks, `poll`, `ioctl`, console line discipline,
  `AF_UNIX` sockets, `smoltcp` TCP/IP networking (`loopback` +
  `virtio-net`), and `picolibc` userspace (`bc`, `dc`, `lua`).
- **Phase 2 GUI (`12-phase2-gui`)** — `ramfb` framebuffer (`fw_cfg`
  DMA, `/dev/fb0`), `virtio-input` keyboard (`/dev/input/0`), and the
  userspace window server (`wm`) + `AF_UNIX` window protocol + demo
  clients (`hello_wm`, `clock`, `guidemo`), gated by headless QMP tests
  (`test-fb`, `test-input`, `test-gui`).

Full per-todo history is in [`docs/todos/`](docs/todos/) — see
[`docs/todos/README.md`](docs/todos/README.md) for the index, and one
folder per todo under `done/` with what landed + verification notes.

## License

MIT (mirrors upstream xv6).
