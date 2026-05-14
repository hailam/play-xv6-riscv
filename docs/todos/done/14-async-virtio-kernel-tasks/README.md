# 14: async virtio + kernel-only tasks

**Done.** `read_block_async`/`write_block_async` via per-descriptor
`WakerCell`. Executor supports kernel tasks (`Task.proc = Option<Arc<Proc>>`).

## Notes
- `read_block_async` takes `usize` instead of `*mut u8` so the returned
  future is `Send` (Pin<Box<dyn Future + Send + 'static>>).
- IRQ handler now both flips `COMPLETED[id]` AND calls `WAKERS[id].wake()`,
  serving both sync and async paths.

## Files
- `crates/kernel/src/driver/virtio_blk.rs::{read_block_async, BlockOp, WAKERS}`
- `crates/kernel/src/executor.rs::{spawn_kernel, Task.proc}`
