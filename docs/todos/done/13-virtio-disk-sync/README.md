# 13: virtio_disk driver (sync first)

**Done.** virtio-mmio handshake, page-aligned static rings, descriptor
allocator, sync `read_block`/`write_block`, PLIC-routed IRQ handler
that drains the used ring + flips `COMPLETED[head]`.

## Notes
- First "real device" with rings + IRQ + busy-wait completion.
- Static ring memory (instead of kalloc) since we initialize before
  much of the kernel is up.

## Files
- `crates/kernel/src/driver/virtio_blk.rs` (initial)
- `crates/kernel/src/trap.rs::kernel_on_external` (added VIRTIO0_IRQ dispatch)
- `Makefile::fs.img` (build target)
