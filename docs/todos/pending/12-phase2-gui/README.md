# 12: Phase 2 — minimal GUI

**Status:** Pending (longest horizon)
**Estimated:** ~1500 LoC across multiple sub-phases
**Depends on:** Filesystem (so a display server can be a process)
**Unblocks:** the original Phase 2 goal — "minimal GUI"

## Why

Original plan said:
> Phase 2 (future): a minimal GUI. Architecture chosen so the GUI is
> just another in-kernel module / driver behind the HAL — no rework needed.

This is the long-horizon goal that influenced architectural choices
(modular HAL, async wakers for I/O, async fb syscalls).

## Approach (sketch — refine when starting)

### Sub-phases

1. **fb-driver** (~200 LoC) — virtio-gpu or ramfb. Register in HAL,
   expose `fb_init`, `fb_blit(x, y, w, h, src)`.
2. **fb device file** (~50 LoC) — `/dev/fb0` as a `File::Fb` variant
   exposed via the existing fd table. Maps writes to `fb_blit`.
3. **input** (~100 LoC) — virtio-input or PS/2; deliver to a `/dev/input/0`
   stream. Similar async waker pattern to UART RX.
4. **display server** (~400 LoC user) — manages windows, composites.
   Just a user process with read access to `/dev/fb0` + `/dev/input/0`.
5. **window protocol** (~200 LoC user lib) — clients connect via a
   Unix-domain-socket equivalent (a named pipe in fs), exchange messages.
6. **demo clients** (~200 LoC) — `clock`, `terminal`, `hello`.

### Key architecture choice

The plan stated:
> phase-2 GUI is a kernel module / driver behind the HAL — no rework
> needed.

In practice: the framebuffer driver is kernel-side (MMIO access). The
display server is **user-side** (managing windows, drawing, compositing).
That's better than putting the display server in kernel, because:
- It can be killed/restarted without rebooting
- Different display servers (terminal-only, full compositor) can coexist
- The kernel only needs the minimum: a writable framebuffer + an input
  stream

Same shape as Wayland: kernel exposes raw pixels/input, userspace does
windowing.

## Verification

- Boot kernel; spawn display server.
- It draws a "hello, world" pattern.
- A clock client connects, ticks every second.
- Input events from QEMU keyboard reach the right client window.

## Risks

- virtio-gpu is non-trivial (DMA descriptors, resource creation, set scanout).
  ramfb is simpler — QEMU exposes a flat framebuffer via fw_cfg + mmio.
  Start with ramfb.
- Without filesystem, the display server has no on-disk state. Initial
  config can be hardcoded; later, fs makes it real.
- This is essentially a separate research project layered on top of the
  kernel. Treat as "stretch goal" and plan independently.

## Code touch points

When starting, this will spawn ~10 sub-todos. Likely first ones:

- `pending/13-virtio-gpu` or `pending/13-ramfb-driver`
- `pending/14-input-events`
- `pending/15-fb-device-file`
- `pending/16-display-server-userspace`
