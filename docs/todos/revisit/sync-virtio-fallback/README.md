# revisit: sync virtio fallback

**Why deferred:** `virtio_blk::sync_read_block` / `sync_write_block`
exist but no one calls them now. Kept because:
- The pattern of "busy-loop on wfi waiting for IRQ" might be useful for
  diagnostic code that runs before the executor (early boot debug)
- Removing it costs ~50 LoC and risks losing a useful fallback

**What would trigger revisit:**
- Confirming no early-boot diagnostic ever needs a sync disk read
- Code-pruning pass to lower LoC count

**Alternative:** Convert to thin wrapper that does
`block_on(read_block_async(...))` using a no-op waker, eliminating the
duplicate path. ~30 LoC change.
