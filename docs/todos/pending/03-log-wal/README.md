# 03: log-wal — write-ahead log

**Status:** Pending
**Estimated:** ~200 LoC
**Depends on:** `02-bio-write`
**Unblocks:** `04-fs-inode-and-dir` (real fs writes wrap in transactions)

## Why

xv6's fs only writes through `log_write`. The log batches dirty buffers
into a transaction; commit writes a header indicating the transaction
is complete, then copies log blocks to their real homes. On crash,
recovery replays a committed log; partial-write transactions are
discarded. Without this, a power loss mid-write can leave the fs
inconsistent (e.g., bitmap says block allocated but inode doesn't
reference it).

For our project this is the canonical xv6 design — should follow it.

## Approach

Port xv6's `log.c` faithfully. The log occupies a known region of disk
(after the superblock):

```
disk layout:
  block 0:      bootblock (we don't use)
  block 1:      superblock
  blocks 2..N+1:  log header + log data
  blocks N+2..:   inodes, bitmap, data
```

### On-disk structures

```rust
struct LogHeader {
    n: u32,             // number of valid block-mapping entries below
    block: [u32; LOGSIZE], // block[i] = destination block for log slot i+2
}
```

`LOGSIZE` ≈ 30 in xv6.

### In-memory state

```rust
struct LogState {
    start: u32,         // first log block (from superblock)
    size: u32,          // total log blocks
    outstanding: u32,   // active begin_op count
    committing: bool,
    dev: u32,
    lh: LogHeader,      // mirrors on-disk header
}
```

### API

```rust
pub async fn begin_op();              // wait until safe to start a transaction
pub fn log_write(buf: &Arc<Buffer>);  // mark a buffer as part of the current transaction
pub async fn end_op();                // if last begin_op active, commit
```

`log_write` does NOT do disk I/O; it just records the block_no in the
log header. The buffer stays in bio with the modified data.

`end_op` (when `outstanding == 0`):
1. Set `committing = true`, drop lock so concurrent `begin_op`s can wait.
2. For each entry in `lh.block`: `bread` the bio cache for that
   real block, then write its contents to the corresponding log block
   slot (`start + 1 + i`).
3. Write `lh` to disk (block `start`); this is the **commit point**.
4. For each entry: copy data from log block back to real home block on disk.
5. Zero out `lh` (in memory + on disk header).
6. Set `committing = false`, wake waiters in `begin_op`.

### Recovery (on boot)

```rust
pub async fn recover() {
    let header_buf = bio::bread(LOG_START_BLOCK).await;
    let lh: LogHeader = read from buf;
    if lh.n > 0 {
        // Replay
        for i in 0..lh.n {
            copy log block i+1 → real block lh.block[i]
        }
        // Clear header.
        zero lh, bwrite.
    }
}
```

## Verification

- Modify a block via `bwrite` directly (no log): observable.
- Modify via `begin_op` / `log_write` / `end_op`: same observable result.
- Crash test (harder): kill QEMU between commit and copy-back; on
  restart, recovery replays. Skip for first cut.

## Risks

- Lock ordering with bio: `log_write` holds log lock; bio buffer
  modification doesn't need bio lock (caller already has the `Arc`).
- Async safety: `begin_op` may need to await for committing transaction
  to finish — use `WakerCell` for the commit-done event.
- Sleeplock semantics (xv6) translate to "park on a `WakerCell` until
  signal". Each await point in begin_op should check `proc.killed` once
  `07-sys-kill-cancellation` lands.

## Code touch points

- New: `crates/kernel/src/fs/log.rs`
- Reads superblock to find log region (so `04-fs-inode-and-dir` provides the superblock layout — see its README; superblock can be defined here and the field reused)
- Calls `bio::bread` + `bio::bwrite`
