# 45 — Unified 4 KiB Page Cache, Multi-Sector VirtIO-Blk Batching & Pipelined WAL

## Problem statement

The storage and filesystem I/O stack is currently bound by five xv6-era design limits:
1. **32 KiB total buffer cache with $O(N)$ linear scan (`crates/kernel/src/driver/bio.rs` lines 23–27, 123–197):**
   - `BSIZE = 512` bytes and `NBUF = 64` buffers (32 KiB total cache!).
   - `bread()` locks a single global `CACHE` `SpinLock` and scans all 64 buffers linearly on every single 512-byte block read.
2. **2-request-max VirtIO block queue (`crates/kernel/src/driver/virtio_blk.rs` line 25):**
   - `NUM = 8` descriptors. Since each `virtio-blk` request uses a 3-descriptor chain (header, data, status), at most **2 requests of 512 bytes** can be in flight globally across the entire machine!
3. **4-phase sequential synchronous WAL commit (`crates/kernel/src/fs/log.rs` lines 219–256):**
   - `commit()` writes each pinned 512-byte buffer to the log area **one by one** (`bio::bwrite(&log_buf).await` inside a `for` loop), then writes the log header (`await`), then writes each pinned buffer to its home block **one by one** (`bio::bwrite(buf).await` inside a `for` loop), then zeroes the log header (`await`). A 10-block transaction issues **22 serialized single-sector disk requests**!
4. **Double-buffering between `bio`, `inode_read`, and `mmap` (`crates/kernel/src/syscall.rs` lines 1568–1587, `crates/kernel/src/fs/inode.rs` lines 286–320):**
   - `inode_read` allocates a temporary `vec![0u8; want]` on the kernel heap, copies 512B chunks from `bio` buffers into `tmp`, and then copies from `tmp` to user memory. File-backed `mmap` also allocates separate physical pages instead of sharing cached file pages directly.
5. **50-slot linear-scan inode cache (`crates/kernel/src/fs/inode.rs` lines 27, 194–216):**
   - `NINODE_CACHE = 50` with a linear scan under a single global `SpinLock`.

## Discovery & research checklist

1. **Hash/Radix-Indexed Unified 4 KiB Page Cache (`crates/kernel/src/driver/bio.rs`, `crates/kernel/src/fs/`):**
   - Index cached blocks/pages by `(dev, block_or_page_idx)` using a sharded hash table (e.g., 64 lock shards) or radix tree (`BTreeMap`/XArray-style per-inode page tree) instead of an $O(N)$ global linear scan.
   - Increase cache capacity dynamically using physical frames from `kalloc` (with LRU/clock eviction on memory pressure) rather than a fixed 64-slot 32 KiB pool.
   - Allow `inode_read` (`crates/kernel/src/syscall.rs` line 1548) and `sys_pread` (line 1702) to copy **directly** from the pinned cache buffer/page into user virtual memory via `Proc::copy_to_user`, completely eliminating the intermediate `alloc::vec![0u8; want]` heap allocation and extra memory copy!
   - Allow `MAP_SHARED` / `MAP_PRIVATE` (COW) read-only file `mmap` to map the page cache physical frame directly into the user page table (zero-copy `mmap`).
2. **Wide VirtIO-Blk Ring (`NUM = 128`) & Scatter-Gather Batching (`crates/kernel/src/driver/virtio_blk.rs`):**
   - Increase `NUM` from `8` to `128` (capped by `MMIO_QUEUE_NUM_MAX` read at init, allowing up to 42 concurrent multi-sector requests in flight).
   - Replace `free: u8` bitmap with a `u128` bitmap or free-index stack (`free_head: u16`).
   - Add `rw_batch(sector: u64, bufs: &[&[u8]], write: bool)` or multi-sector contiguous read/write (`len = N * 512`) so sequential file reads/writes and WAL commits issue a single DMA transfer and a single `MMIO_QUEUE_NOTIFY` trap.
3. **Concurrent / Pipelined WAL Commit (`crates/kernel/src/fs/log.rs`):**
   - In `commit()` (lines 229–246), submit all `lh.n` log-slot writes **concurrently** (e.g., batching contiguous log blocks `log_start + 1 .. log_start + 1 + n` into a single multi-sector `virtio-blk` request, or joining all `bwrite` futures before a single `MMIO_QUEUE_NOTIFY` doorbell) instead of awaiting each 512-byte sector sequentially.
   - Similarly, submit all `pinned` home-block writes in parallel before clearing the log header.
4. **Hashed Inode & Directory Entry Cache (`crates/kernel/src/fs/inode.rs`):**
   - Expand `NINODE_CACHE` (e.g., 512 inodes) with a hash bucket table keyed by `(dev, inum)` and add a negative/positive dentry lookup cache (`dcache`) so `namex` path resolution (`resolve_path` for `/bin/sh`, `/lib/...`, etc.) resolves in $O(1)$ memory lookups without disk/bio reads.

## Verification

- All filesystem tests in `usertests` (`bigfile`, `manywrites`, `fourfiles`, `dirtest`, `fsfull`, `writebig`, `openiput`, `iput`) and crash-recovery log tests pass on both `riscv64` and `aarch64`.
- `bench` sequential 4 MiB read/write throughput, random 4 KiB `pread`, and 200-file metadata creation/lookup improve by $>10\times$.
