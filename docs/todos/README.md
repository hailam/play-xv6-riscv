# todos

Tracked work for the Rust xv6-riscv rewrite.

Each todo is a directory under `pending/`, `done/`, or `revisit/`. The
directory contains a `README.md` with the plan/summary, plus optional
`design.md` or `notes.md` for deeper context.

## Status snapshot

| Bucket | Count | What's in it |
|---|---|---|
| `done/` | 37 | Through the phase-2 GUI — ramfb framebuffer, /dev/fb0, virtio-input, and a userspace display server with window protocol + demo clients; **suite (74 tests) green smp1 both arches + smp3**; test-net/test-fb/test-input/test-gui all gated |
| `pending/` | 18 | Consolidation/refinement (`29`–`33`), Phase 3 Unix/Linux-musl ABI & VFS (`34`–`40`), and Phase 4 High-Performance Kernel Architecture (`41`–`46`) |
| `revisit/` | 3 | Decisions to potentially revisit later |

## Pending — priority order

### Consolidation & kernel refinement
1. [29-aarch64-hal-audit-and-trapframe-checks](pending/29-aarch64-hal-audit-and-trapframe-checks/) —
   audit stale skeleton/riscv-only comments across `hal-aarch64` and
   `kernel`, verify `TIMER_INTERVAL` usage, and add compile-time
   `offset_of!` / `size_of` assertions for AArch64 `TrapFrame` matching
   `trampoline.S`.
2. [30-mmap-va-reclamation](pending/30-mmap-va-reclamation/) —
   research and implement virtual-address hole reclamation in `Proc`'s
   VMA allocator (replacing the monotonic `mmap_base` bump pointer),
   verify middle-of-VMA `munmap` splitting, and gate with a VA churn test.
3. [31-syscall-modularization](pending/31-syscall-modularization/) —
   inventory and split the monolithic ~4,300-line `syscall.rs` into
   domain submodules (`uaccess`, `proc`, `mm`, `fs`, `signal`, `time`,
   `io`, `net`) without changing external visibility or behavior.
4. [32-net-and-ipi-refinement](pending/32-net-and-ipi-refinement/) —
   wire precise per-hart `CPUTargetList` masks in AArch64 `GICD_SGIR`
   (`Arch::send_ipi`), audit `Buffer::data_mut` aliasing in concurrent
   `iupdate` calls, and review `net.rs` lock granularity.
5. [33-build-system-consolidation](pending/33-build-system-consolidation/) —
   audit split build orchestration across `Makefile`, `xmake.lua`, and
   `crates/kernel/build.rs` (including dead `*_BIN_PATH` env exports and
   C header dependency tracking).

### Phase 3 — Linux-musl ABI, Rust `std` (`uutils`), and upstream Unix software
6. [34-pseudo-devices-and-tty](pending/34-pseudo-devices-and-tty/) —
   add `/dev/null`, `/dev/zero`, `/dev/urandom`, and `/dev/tty`, wire
   real `isatty` via `TCGETS`, and add process group (`pgid`) support.
7. [35-fs-long-names-and-capacity](pending/35-fs-long-names-and-capacity/) —
   widen `DIRSIZ` from 14 to 62 bytes (`64`-byte `Dirent`) and support
   multi-block bitmaps so long filenames and multi-megabyte static
   binaries fit on `fs.img`.
8. [36-cow-fork](pending/36-cow-fork/) —
   add physical frame reference counting in `kalloc.rs` and Copy-on-Write
   (`PtePerm::COW`) page-table cloning in `Proc::fork` + fault/copyout
   promotion so forking large binaries is fast and memory-efficient.
9. [37-linux-musl-abi-and-auxv](pending/37-linux-musl-abi-and-auxv/) —
   populate ELF `auxv` on the initial user stack in `execve` and add the
   unified `riscv64`/`aarch64` Linux `asm-generic/unistd.h` syscall
   personality (`openat`, `newfstatat`, `getdents64`, `readv`/`writev`,
   `clone`, `futex`, `set_tid_address`, `rt_sig*`, etc.).
10. [38-rust-std-and-uutils-coreutils](pending/38-rust-std-and-uutils-coreutils/) —
    compile and run static Rust `std` binaries (`riscv64gc-unknown-linux-musl`
    and `aarch64-unknown-linux-musl`) and bring up the [`uutils/coreutils`](https://github.com/uutils/coreutils)
    multicall binary on both architectures.
11. [39-musl-c-ports-sbase-busybox](pending/39-musl-c-ports-sbase-busybox/) —
    cross-compile `musl` `libc.a` for `riscv64` and `aarch64` and port
    upstream Unix C suites (suckless `sbase`, `dash`/`ash`, SQLite).
12. [40-vfs-and-external-filesystems](pending/40-vfs-and-external-filesystems/) —
    extract an async VFS trait + mount table (`/`, `/tmp`, `/proc`,
    `/dev`), add `tmpfs` and `procfs` (`/proc/self/exe`), and research
    integrating external `#![no_std]` filesystem crates (`ext4_rs`,
    `fatfs`) or async `virtio-9p`.

### Phase 4 — High-performance kernel architecture
13. [41-bench-suite-and-pmu-counters](pending/41-bench-suite-and-pmu-counters/) —
    enable user-mode hardware cycle/timer counters (`scounteren` on
    RISC-V, `cntkctl_el1` / `pmuserenr_el0` on AArch64) and build an
    in-guest `bench` suite covering syscall, pipe, page-fault, `fork`,
    storage, TCP, and SMP scaling latency/throughput.
14. [42-split-va-asid-and-hugepages](pending/42-split-va-asid-and-hugepages/) —
    split user/kernel address spaces (`TTBR1_EL1` / shared global L2
    PTEs) with 16-bit hardware ASIDs to eliminate per-trap page-table
    swaps and `sfence.vma` / `tlbi vmalle1is` shootdowns, map
    `KERNBASE..PHYSTOP` with 2 MiB huge pages, and replace per-byte
    `translate_user` walks with page-chunked / hardware-MMU (`SUM`/`PAN`)
    `copy_from_user` and `copy_to_user`.
15. [43-percpu-slab-and-page-allocators](pending/43-percpu-slab-and-page-allocators/) —
    add lock-free per-CPU 4 KiB frame magazines in `kalloc.rs` (and
    remove redundant double-zeroing) plus per-CPU $O(1)$ size-class slab
    caches (`16..2048` B) in `heap.rs`.
16. [44-lockfree-workstealing-executor](pending/44-lockfree-workstealing-executor/) —
    add a synchronous syscall fast-path and inline per-`Proc` syscall
    future storage, replace double-locking `tasks.lock()` polling with
    atomic task states and Chase-Lev work-stealing ready queues, retain
    `WakerList` capacity across wakeups, and batch pipe ring buffer I/O.
17. [45-unified-page-cache-and-virtio-batching](pending/45-unified-page-cache-and-virtio-batching/) —
    replace the 64-slot 512B linear-scan `bio` cache with a hash/radix
    4 KiB unified Page Cache (zero-copy shared with `inode_read` and
    `mmap`), widen `virtio-blk` to `NUM = 128` with multi-sector
    scatter-gather DMA, and pipeline WAL commits.
18. [46-async-uring-and-zero-copy-net](pending/46-async-uring-and-zero-copy-net/) —
    expose shared user/kernel async submission/completion rings
    (`io_uring` mapped 1:1 onto kernel `Future`s, including trapless
    `SQPOLL` mode) and upgrade `virtio-net` to 128/256 descriptors with
    batched notifications, `EVENT_IDX`, checksum offload, and NAPI polling.

## Done — chronological

| ID | Title | LoC |
|---|---|---|
| 00 | [scaffold-boot](done/00-scaffold-boot/) | +250 |
| 01 | [hal-spinlock-percpu](done/01-hal-spinlock-percpu/) | +250 |
| 02 | [kalloc-paging](done/02-kalloc-paging/) | +500 |
| 03 | [trap-timer](done/03-trap-timer/) | +200 |
| 04 | [async-executor-first-user](done/04-async-executor-first-user/) | +800 |
| 05 | [fork](done/05-fork/) | +200 |
| 06 | [sleep-wait-wakers](done/06-sleep-wait-wakers/) | +100 |
| 07 | [exec-multiple-bins](done/07-exec-multiple-bins/) | +140 |
| 08 | [plic-uart-shell](done/08-plic-uart-shell/) | +260 |
| 09 | [pipes](done/09-pipes/) | +340 |
| 10 | [shell-pipelines](done/10-shell-pipelines/) | +180 |
| 11 | [argv](done/11-argv/) | +90 |
| 12 | [elf-loader](done/12-elf-loader/) | +220 |
| 13 | [virtio-disk-sync](done/13-virtio-disk-sync/) | +425 |
| 14 | [async-virtio-kernel-tasks](done/14-async-virtio-kernel-tasks/) | +75 |
| 15 | [buffer-cache](done/15-buffer-cache/) | +190 |
| 16 | [bio-eviction](done/16-bio-eviction/) | +60 |
| 17 | [bio-write](done/17-bio-write/) | +50 |
| 18 | [log-wal](done/18-log-wal/) | +210 |
| 19 | [mkfs-host-tool](done/19-mkfs-host-tool/) | +270 (host) |
| 20 | [fs-inode-and-dir](done/20-fs-inode-and-dir/) | +400 |
| 21 | [file-syscalls-read-path](done/21-file-syscalls-read-path/) | +310 (+120 user) |
| 22 | [fs-writes](done/22-fs-writes/) | +570 (+120 user) |
| 23 | [sys-kill-cancellation](done/23-sys-kill-cancellation/) | +150 (+70 user) |
| 24 | [sbrk-and-malloc](done/24-sbrk-and-malloc/) | +50 (+160 user) |
| 25 | [vm-reaping](done/25-vm-reaping/) | +130 |
| 26 | [smp-user-procs](done/26-smp-user-procs/) | +150 (+25 user) |
| 27 | [aarch64-hal-skeleton](done/27-aarch64-hal-skeleton/) | +210 (hal-aarch64) |
| 28 | [fs-polish](done/28-fs-polish/) | +180 (+80 user) |
| 29 | [aarch64-completion](done/14-aarch64-completion/) | +1500 — boots interactive shell under qemu-system-aarch64 -smp 4 |
| 30 | [xv6-compat](done/15-xv6-compat/) | (spread) — **full usertests suite passes, both arches**; G1/G3/G9 ruled obsolete |
| 31 | [correctness-audit](done/17-correctness-audit/) | +750/−287 — kernel-wide audit; lost-wakeup waker class, begin_op livelock, free-list heap allocator, fd description sharing, fault-type checks, OOM leak fixes |
| 32 | [proc-lifecycle](done/18-proc-lifecycle/) | ~+330 — real init (pid 1) + orphan reparenting, deferred-iput inode reaper, executor slot reuse; +2 regression tests |
| 33 | [smp-hardening](done/19-smp-hardening/) | ~+150 — riscv M-mode CLINT-MSIP→SSIP IPI trampoline, aarch64 SGI wiring, wake-IPIs, pid registry; **full suite green at -smp 3/4** |
| 34 | [fs-console-robustness](done/20-fs-console-robustness/) | ~+300 — sparse-hole reads, ftruncate clamp, iget-wait, lazy demand-map in copyin/copyout, console line discipline (^D/erase/echo), fair console_write, waker-driven poll; +2 tests |
| 35 | [posix-compat (sockets+TCP/IP)](done/16-posix-compat/) | AF_UNIX (syscalls 63-68, fs-visible bindings) + TCP/IP via smoltcp 0.12 (loopback + virtio-net, per-interface SocketSets); first external crate; `tcploop`+`unixsock` usertests + `make test-net` host↔guest echo |
| 36 | [phase2-gui](done/12-phase2-gui/) | ~+1400 — ramfb via fw_cfg DMA (640×480 XRGB8888), /dev/fb0 + /dev/input/0 device files (first device-major dispatch), virtio-input keyboard, userspace display server (wm) + window protocol over AF_UNIX + hello_wm/clock clients; gated by test-fb / test-input / test-gui (headless QMP screendump + send-key) |

(Rows 30/31 live in `done/15-xv6-compat` and `done/17-correctness-audit`
— their numeric directory prefixes collide with older done entries; the
table is chronological.)

## Revisit

- [futures-task-dep](revisit/futures-task-dep/) — hand-rolled waker
  plumbing (`WakerCell` and multi-waiter `WakerList`); consider
  `futures-task` only if waker requirements grow more complex.
- [strip-elf-size](revisit/strip-elf-size/) — `sh.elf` is 1.2 KB after
  `--strip-all`; could be smaller
- [sync-virtio-fallback](revisit/sync-virtio-fallback/) —
  `sync_read_block` exists but no longer used

## Workflow

1. Read the task's `README.md` (and `design.md` if present) and complete its discovery/research checklist.
2. Implement and verify against the task's test gate on both architectures.
3. Move the directory from `docs/todos/pending/<id-name>` to `docs/todos/done/`.
4. Update the task's `README.md` with what landed and key files touched, and update the table above.
