# 02: bio bwrite + dirty tracking

**Status:** Pending
**Estimated:** ~50 LoC
**Depends on:** —
**Unblocks:** `03-log-wal`, `04-fs-inode-and-dir`

## Why

bio currently only reads. `virtio_blk::write_block_async` exists but no
one calls it. For any write path (log, inode allocation, file
writes) we need a way to commit a buffer back to disk.

## Approach

xv6 has explicit `bwrite(buf)`. Caller modifies `buf->data`, then calls
`bwrite` to flush. We can do the same:

```rust
pub async fn bwrite(buf: &Arc<Buffer>) -> Result<(), DiskError> {
    let block_no = buf.block_no.load(Ordering::Acquire);
    let addr = buf.data_addr();  // existing accessor
    virtio_blk::write_block_async(block_no as u64, addr).await
}
```

Optional: dirty bit on `Buffer`. `bwrite` sets it; eviction checks it
(refuses to evict dirty buffers, or flushes first). For Phase 6c.5 keep
it simple — caller is responsible for `bwrite`-ing before drop.

Add `data_addr()` returning the raw pointer (currently `data_addr` is
private; expose).

For writers to mutate data: expose a `pub unsafe fn data_mut(&self)
-> &mut [u8; BSIZE]` and document that only one writer at a time is
allowed (logically enforced by the log layer's transaction
serialization).

## Verification

- Write a marker byte to block 5 via `bwrite`.
- Hexdump fs.img on host after QEMU shutdown; verify the marker landed.
- Or: re-read block 5 after `bwrite`; observe the new value.

## Risks

- Without a log, partial writes leave fs.img inconsistent across
  crashes. Acceptable until `03-log-wal` lands; only the log layer
  *cares* about crash safety.

## Code touch points

- `crates/kernel/src/driver/bio.rs` — add `bwrite`, expose `data_addr`/`data_mut`
- (Optional) add dirty bit, blocking eviction until flushed
