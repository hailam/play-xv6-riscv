# 18: log-wal — write-ahead log

**Done.** xv6-style log: `begin_op` / `log_write` / `end_op` async API,
commit via 4-step protocol with a single durable commit point (the log
header write), recovery on boot if the header is non-empty.

## Verification

End-to-end transactional write + host inspection:

```
log: 2-block transaction committed (10 I/Os)
block 300: TX-BLOCK-A
block 301: TX-BLOCK-B
```

Host `dd if=fs.img` confirms both blocks landed, AND that the log header
(block 2) is zeroed — i.e., the commit completed cleanly and the
"there's pending recovery work" flag is off.

## Concurrency notes

- `begin_op` is async; waits on `COMMIT_WAKER` if the log is committing
  or doesn't have headroom for `MAXOPBLOCKS` more writes.
- `WakerCell` holds at most one parked waker — works because only one
  task does fs ops at a time in Phase 6.5. A real wake-all queue is a
  follow-up if multiple user procs hammer the fs concurrently.
- `log_write` is synchronous (just bookkeeping); `end_op` is async
  because the commit path does multiple `bread`/`bwrite`s.

## Files

- `crates/kernel/src/fs/mod.rs` (new module root)
- `crates/kernel/src/fs/log.rs` (~210 LoC)
- `crates/kernel/src/main.rs::disk_smoke_test` (demo updated)

## Layout assumption (hardcoded for now)

```
block 0       — unused
block 1       — future superblock
block 2       — log header
blocks 3..33  — log data slots (LOGSIZE=30)
blocks 33..   — free; demo writes go to blocks 300/301
```

When `04-fs-inode-and-dir` lands, the layout becomes superblock-driven;
`log::init(start, size)` reads values from `sb` instead of hardcoded.
