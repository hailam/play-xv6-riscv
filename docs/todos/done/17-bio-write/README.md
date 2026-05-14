# 17: bio bwrite + unsafe `data_mut`

**Done.** `bio::bwrite(&buf)` flushes a buffer to disk via
`virtio_blk::write_block_async`. `Buffer::data_mut` exposes a `&mut
[u8; BSIZE]` to writers; safety contract is documented on the method.

## Verification

End-to-end persistence: kernel writes `WROTE-BY-KERNEL!` to block 100,
evicts it from the cache (40 unrelated reads push it out), re-reads
it. `dd if=fs.img skip=100 bs=512 count=1 | xxd` on the **host**
confirms `fs.img` has the new bytes — the write reached the device.

```
bio write: wrote marker to block 100 (2 I/Os used)
bio re-read block 100 (1 fresh I/Os), first 16 bytes: WROTE-BY-KERNEL!
```

## Notes

- No dirty-bit / writeback batching yet. Callers `bwrite` explicitly.
  The log layer (`03-log-wal`) is where atomic group-commit will go.
- `data_mut` is `unsafe` because aliasing rules can't be enforced
  statically — concurrent readers of the same buffer must be excluded
  by the caller (e.g., by holding the only `Arc<Buffer>`, or via a
  sleeplock-equivalent in the log layer).
- Persistence across QEMU runs: `make qemu` doesn't regenerate
  `fs.img` (the rule has no force). Once written, changes stick.

## Files

- `crates/kernel/src/driver/bio.rs::{bwrite, Buffer::data_mut}`
- (Demo) `crates/kernel/src/main.rs::disk_smoke_test`
