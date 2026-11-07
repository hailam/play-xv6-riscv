# 46 — Trapless Async Syscall Rings (`io_uring`) & Zero-Copy VirtIO-Net

## Problem statement

To decisively outperform Linux on I/O-intensive server, database, and networking workloads, we should exploit `play-xv6-riscv`'s native async-Rust kernel architecture:
1. **In Linux, `io_uring` is bolted onto a synchronous thread-per-task kernel** (requiring complex workqueues, `io-wq` kernel threads, and context-switch fallbacks). In `play-xv6-riscv`, **every syscall is already a Rust `Future` polled by `executor.rs`** — meaning a shared user/kernel submission & completion ring maps 1:1 onto our native executor with zero kernel thread overhead!
2. **Current `virtio_net.rs` & `net.rs` bottlenecks (`crates/kernel/src/driver/virtio_net.rs`, `crates/kernel/src/net.rs`, `crates/kernel/src/syscall.rs`):**
   - `virtio_net.rs` (lines 28–29) uses `NUM = 8` RX descriptors and `NUM = 8` TX descriptors (`tx_free: u8`), limiting the NIC to 8 packets in flight!
   - `repost_rx` (line 314) and `TxToken::consume` (line 414) write `MMIO_QUEUE_NOTIFY` on **every single packet**, causing a hypervisor/VM-exit trap per packet.
   - Checksums are computed in software (`ChecksumCapabilities::default()` in `crates/kernel/src/net.rs`) and `tcp_read` (`crates/kernel/src/syscall.rs` lines 2440–2479) copies into a 4 KiB stack array and then copies byte-by-byte via `translate_user_write`.

## Discovery & research checklist

1. **Native Async Syscall Rings (`sys_uring_setup` / `sys_uring_enter`, `crates/kernel/src/uring.rs`):**
   - Allocate a shared physical page (or pair of pages) mapped into both kernel space and user space (`mmap` / `sys_uring_setup(entries, params)`):
     - **Submission Queue (SQ):** Lock-free single-producer (user) single-consumer (kernel) ring of `SqEntry { opcode, flags, fd, addr, len, off, user_data }`.
     - **Completion Queue (CQ):** Lock-free single-producer (kernel) single-consumer (user) ring of `CqEntry { user_data, res, flags }`.
   - Support core async opcodes directly backed by our existing syscall futures: `OP_NOP`, `OP_READ`, `OP_WRITE`, `OP_PREAD`, `OP_PWRITE`, `OP_ACCEPT`, `OP_CONNECT`, `OP_RECV`, `OP_SEND`, `OP_POLL_ADD`, `OP_TIMEOUT`.
   - **Batch submission (`sys_uring_enter`):** Drain $N$ SQEs in a single syscall trap, spawn their Rust `Future`s onto the local hart's executor, and write CQEs directly into the shared page as each future resolves.
   - **`IORING_SETUP_SQPOLL` (Trapless mode):** Optional kernel poller task on a dedicated idle hart that polls the user SQ ring’s atomic tail pointer and writes CQEs without user space executing a single `ecall`/`svc` instruction!
2. **High-Throughput `virtio-net` & `net.rs` Pipeline (`crates/kernel/src/driver/virtio_net.rs`, `crates/kernel/src/net.rs`):**
   - Widen RX and TX virtqueues from `NUM = 8` to `NUM = 128` or `256` (bounded by `MMIO_QUEUE_NUM_MAX`).
   - Batch RX descriptor reposting: repost all drained RX buffers in `poll_rx` before issuing a single `MMIO_QUEUE_NOTIFY` write.
   - Negotiate `VIRTIO_F_RING_EVENT_IDX` (bit 29) and `VIRTIO_NET_F_CSUM` / `VIRTIO_NET_F_GUEST_CSUM` where supported by QEMU `virt`, avoiding redundant doorbell traps and software TCP/IP checksum passes.
   - Implement NAPI-style adaptive polling: when a NIC IRQ fires, mask the NIC interrupt and poll `net::poll()` until a budget (e.g. 64 packets) is exhausted or RX is empty before re-enabling the IRQ.
   - Replace the per-byte `translate_user_write` loop in `tcp_read` (`crates/kernel/src/syscall.rs` lines 2462–2477) and `tcp_write` with page-chunked `Proc::copy_to_user` / `Proc::copy_from_user` (or direct `smoltcp` ring buffer `recv(|slice| ...)` / `send(|slice| ...)` closures to eliminate the 4 KiB intermediate `tmp` buffer on the stack!).

## Verification

- Add a `uring_test` and `uring_bench` user binary verifying batched file reads/writes, pipe I/O, and TCP echo server operations via SQ/CQ rings (including `SQPOLL` zero-syscall mode).
- `bench` TCP loopback and `virtio-net` throughput and latency tests pass on both `riscv64` and `aarch64` under `-smp 4`.
- Compare batched IOPS and TCP echo RPS against single-syscall `read`/`write` and against the Linux baseline in `bench`.
