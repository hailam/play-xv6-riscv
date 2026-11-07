# 32: Targeted AArch64 SGI mask & networking/inode concurrency refinement

**Status:** Pending — needs discovery, research & verification
**Scope:** `crates/hal-aarch64/src/gic.rs`, `crates/hal-aarch64/src/lib.rs`, `crates/kernel/src/net.rs`, `crates/kernel/src/fs/inode.rs`

## Problem statement

Three concurrency/SMP simplifications remain in the tree:
1. **Broadcast wake-IPI on AArch64:** `AArch64::send_ipi(hart_mask)` (`crates/hal-aarch64/src/lib.rs`, lines 136–143) ignores `hart_mask` and calls `gic::sgi_all_except_self(0)`, waking every secondary core on every cross-CPU task wakeup.
2. **Single global `NET` spinlock:** `crates/kernel/src/net.rs` wraps both the `Loopback` interface and the `virtio-net` Ethernet interface in one `SpinLock<Option<NetStack>>`, serializing loopback IPC against NIC traffic.
3. **Disjoint-byte concurrent writes in `iupdate`:** As noted in `crates/kernel/src/fs/inode.rs`, two harts calling `iupdate` on different inodes that share the same disk block concurrently call `Buffer::data_mut(&self)` (`UnsafeCell<[u8; BSIZE]>`), creating two `&mut [u8; BSIZE]` references simultaneously even though they touch disjoint `Dinode` slices.

## Discovery & research checklist

1. **AArch64 GICv2 `GICD_SGIR` target mask:**
   - Check the ARM GICv2 architecture spec for `GICD_SGIR` (`0xF00`): with `TargetListFilter = 0b00` (bits `[25:24]`), `CPUTargetList` (bits `[23:16]`) is an 8-bit mask of target CPU interfaces (`MAX_CPUS = 8`).
   - Verify on QEMU `-machine virt -cpu cortex-a72 -smp 4` whether `(hart_mask & 0xff) << 16` delivers SGI #0 strictly to the requested harts.
2. **`Buffer` aliasing soundness in `bio.rs` / `inode.rs`:**
   - Audit all callers of `Buffer::data(&self)` and `Buffer::data_mut(&self)` across `crates/kernel/src/driver/bio.rs` and `crates/kernel/src/fs/`.
   - Determine whether `Buffer` should expose a locked or raw-pointer/slice-range API (or per-buffer spinlock/sleeplock) so concurrent `iupdate` calls on distinct inodes in the same block never construct aliasing `&mut [u8; BSIZE]` references.
3. **`net.rs` lock granularity:**
   - Measure lock hold duration in `net_task` and socket syscalls, and evaluate whether separating `Loopback` and `eth` state into independent locks is worthwhile or if keeping a single `NetStack` lock is preferable for simplicity.

## Verification

- Boot `qemu-system-aarch64 -smp 4` and run `smptest` + `usertests` to verify targeted SGI delivery.
- Run concurrent filesystem tests (`fourfiles`, `manywrites`, `createdelete`, `truncate3`) under `-smp 3` / `-smp 4` and `make test-net` + `tcploop`.
