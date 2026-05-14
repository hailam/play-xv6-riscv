# 16: bio LRU eviction

**Done.** `Buffer.last_used: AtomicU64` bumped on hit/load. `bread`'s
eviction path: first prefers any (`!valid && !loading`) slot; then
falls back to the idle valid slot (`Arc::strong_count == 1 && !loading`)
with the smallest `last_used`.

## Verification

Reading 64 distinct blocks through a 32-slot cache works without
panic. `IO_COUNT` shows 1 → 64 → 64 (re-read of block 63 = hit):

```
bio test (after 2 reads of block 0): 1 I/Os submitted
bio test (after reading blocks 0..64): 64 I/Os submitted
bio test (after re-reading block 63 (expect hit)): 64 I/Os submitted
```

## Notes

- Marking the evicted slot `valid = false` *inside* the CACHE lock
  prevents a concurrent `bread` for the old `block_no` from seeing a
  half-evicted buffer.
- `Arc::strong_count` is a hint; for single-CPU executor it's reliable
  because the executor never preempts a `bread`'s critical section.
  SMP will need re-validation after taking the slot.
- The hit-path now bumps `last_used = Arch::now_ticks()` so recently
  used buffers are last to evict.

## Files
- `crates/kernel/src/driver/bio.rs::{Buffer.last_used, pick_evict_slot, bread}`
