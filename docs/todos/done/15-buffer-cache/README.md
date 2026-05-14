# 15: buffer cache (bio)

**Done.** Static 32-buffer pool, `async fn bread(block_no) -> Arc<Buffer>`.
Concurrent reads of the same block coalesce on a per-buffer `io_waker`
so only one disk I/O fires.

## Notes
- `IO_COUNT` counter in `virtio_blk` is the cleanest signal that the
  cache works (two `bread`s of same block → count stays at 1).
- No eviction yet — `bread` panics after 32 distinct blocks. See
  `pending/01-bio-eviction`.
- `Buffer.data` is `UnsafeCell<[u8; BSIZE]>` with manual `Send`/`Sync`;
  the loading flag serves as logical lock during I/O.

## Files
- `crates/kernel/src/driver/bio.rs`
- `crates/kernel/src/driver/virtio_blk.rs::IO_COUNT`
