# 41 — In-Guest Benchmark Suite & Hardware PMU Cycle Counters (`bench`)

## Problem statement

To systematically outperform Linux across single-core latency, SMP scalability, memory management, storage, and networking, the kernel needs reproducible cycle-accurate micro- and macro-benchmarks. Currently, there is no benchmark binary in `user/` and no standardized cycle counter harness comparing `play-xv6-riscv` against a reference Linux/musl guest on identical QEMU `-machine virt` configurations (1-core and 4-core SMP, `riscv64` and `aarch64`).

## Discovery & research checklist

1. **Architecture cycle & timer counters (`crates/hal-riscv64/src/lib.rs`, `crates/hal-aarch64/src/lib.rs`):**
   - On RISC-V (`crates/hal-riscv64/src/lib.rs` line 40), `now_ticks()` reads the `time` CSR (`rdtime`, 10 MHz in QEMU `virt`). Verify whether user mode (`U-mode`) can read `cycle` (`rdcycle`) and `instret` (`rdinstret`) directly: check `sCounteren` CSR initialization in `crates/hal-riscv64/src/boot.rs` (currently `scounteren` is not explicitly set to `0b111` (`CY | TM | IR`), which causes an illegal-instruction trap if user space executes `rdcycle` or `rdtime`!).
   - Also check `mcounteren` in M-mode entry or OpenSBI defaults.
   - On AArch64 (`crates/hal-aarch64/src/lib.rs` lines 42–49), `now_ticks()` reads `cntvct_el0` and `cntfrq_el0`. Verify `cntkctl_el1` (`EL0VCTEN | EL0PCTEN`) and `pmuserenr_el0` / `pmcr_el0` / `pmcntenset_el0` initialization in `crates/hal-aarch64/src/boot.rs` so user space can read `cntvct_el0` and `pmccntr_el0` without trapping to EL1.
2. **Micro-benchmark workloads (`user/bench.c` or `user/rs-bench`):**
   - **Null syscall latency:** `getpid()` in a 100,000-iteration tight loop (measures raw `ecall`/`svc` -> `uservec` -> `usertrap` -> `executor::poll` -> `userret` cost in cycles/ns).
   - **Pipe ping-pong latency & throughput:**
     - Latency: 1-byte token ping-pong between two processes over a pair of pipes (measures context switch + scheduler wakeup + `WakerList` + TLB/ASID switch cost).
     - Throughput: 64 KiB chunked streaming between writer and reader (exposes the current per-byte `PipeInner` lock and `translate_user` bottleneck in `crates/kernel/src/syscall.rs` lines 1895–2064).
   - **VM & Page Fault latency:**
     - `mmap` anonymous 16 MiB + sequential/stride-4096 write fault latency (measures `kalloc::alloc_zeroed` + `UserVm::map_anon_range` / COW fault cost).
     - `fork()` + `wait()` of a process with a 16 MiB mapped arena (measures eager copy vs. COW fork + TLB shootdown).
     - `fork()` + `exec("/bin/true")` + `wait()` throughput (shells/sec).
   - **Filesystem & Block I/O:**
     - Sequential `write()` and `read()` of a 4 MiB file in 4 KiB buffers (exposes per-byte `translate_user_write`, 512B `bio`, and 4-barrier WAL commit in `crates/kernel/src/fs/log.rs`).
     - Random 4 KiB `pread()` across a cached file (measures buffer/page cache lookup + user copyout cost).
     - Metadata storm: create, `stat`, and `unlink` 200 small files in a directory.
   - **Networking:**
     - Loopback (`127.0.0.1`) TCP 1-byte ping-pong latency and 1 MiB stream throughput (`crates/kernel/src/net.rs` and `crates/kernel/src/syscall.rs` `tcp_read`/`tcp_write`).
   - **SMP Scaling (1 vs. 2 vs. 4 harts):**
     - Parallel `kalloc`/`mmap` fault storm across $N$ worker processes (`fork()` $\times N$ cpus) to measure global `SpinLock` contention in `kalloc.rs`, `heap.rs`, `executor.rs`, and `bio.rs`.
3. **Kernel-side PMU/profiling counters (optional `/proc/perf` or `sysinfo` extension):**
   - Track per-CPU counters for trap entries, TLB shootdowns, `kalloc` calls, `heap` allocations, `executor::poll` invocations, and `bio` cache hits/misses using relaxed per-CPU atomics.

## Verification

- Enable user-mode `rdtime`/`rdcycle` on RISC-V (`scounteren = 7`) and `cntvct_el0`/`pmccntr_el0` on AArch64 (`cntkctl_el1`).
- Build and pack `bench` into `fs.img`.
- Running `bench` inside QEMU (`xmake run` on both `riscv64` and `aarch64`) prints structured JSON/tabular results for all micro- and macro-benchmarks with zero panics.
- Record baseline numbers before Phase 4 tasks `42`–`46` and verify step-function improvements after each task.
