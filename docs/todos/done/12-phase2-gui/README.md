# 12: Phase 2 — minimal GUI

**Status:** **DONE (2026-06-11)** — all four milestones: M1 ramfb framebuffer, M2 /dev/fb0, M3 virtio-input → /dev/input/0, M4 display server + window protocol + clients. The original Phase 2 goal is met.
**Estimated:** ~1500 LoC across multiple sub-phases
**Depends on:** Filesystem (so a display server can be a process)
**Unblocks:** the original Phase 2 goal — "minimal GUI"

## Milestones & progress

- **M1 — ramfb framebuffer driver: DONE (2026-06-11).**
  `crates/kernel/src/driver/ramfb.rs` drives QEMU's fw_cfg device over
  its MMIO+DMA interface (probe "QEMU" signature → confirm DMA feature
  bit → read `FW_CFG_FILE_DIR` → find the `etc/ramfb` selector key →
  DMA-write a big-endian `RAMFBCfg` blob pointing at a 640×480
  XRGB8888 framebuffer). The framebuffer is a page-aligned static in
  kernel BSS (identity-mapped, so its VA == the phys addr QEMU scans
  out, and contiguous by construction — the LIFO frame allocator can't
  promise a 300-page contiguous run). ramfb is a "dumb" framebuffer:
  configure once, then writes to the buffer just appear (no
  flush/scanout command). The kernel draws a 4-quadrant test pattern
  at boot. Both arches (only difference: fw_cfg base — riscv
  `0x10100000`, aarch64 `0x09020000`, wired via `Hal::FWCFG` +
  `EXTRA_MMIO`). Probe-and-disable: headless if qemu has no
  `-device ramfb`.
  - **Two bugs fixed during bring-up:** (1) `RAMFBCfg`/`FwCfgDmaAccess`
    must be `#[repr(C, packed)]` — plain `repr(C)` padded RAMFBCfg to
    32 bytes so the `size == 28` directory check rejected it (and the
    on-wire layout would've been wrong anyway); (2) the framebuffer
    can't come from the page-frame allocator (non-contiguous) — moved
    to a static BSS buffer.
  - **Gated by `make test-fb`** (`scripts/test-fb.py`): boots with
    `-device ramfb` + a QMP socket, lets the kernel draw, `screendump`s
    the scanout to PPM and checks the centre pixel of each quadrant —
    red/green/blue/white all verified. Headless (no display backend).
    Full usertests still green both arches.
- **M2 — `/dev/fb0` device file: DONE (2026-06-11).**
  Userspace can now draw. Pieces:
  - `File::Fb { off }` fd variant; `sys_open` dispatches a `T_DEVICE`
    inode with `major == FB_MAJOR (1)` to it (xv6's devsw model —
    first real device-major dispatch in the kernel; open fails if
    ramfb isn't up). Unknown majors still fall through to plain
    inode I/O.
  - read/write hit the live pixel buffer at the fd's byte offset
    (offset belongs to the open file description, like inode fds);
    `lseek` SEEK_SET/CUR/END works against the fixed fb size — the
    natural way to address a pixel is seek to `y*stride + x*4` and
    write 4 bytes. poll reports always-ready; `ioctl(FBIOGET_DIMS)`
    returns `FbDims { width, height, stride, bpp }` (uapi.rs).
  - `/dev` + `/dev/fb0` are created by **init (pid 1)** at boot via
    plain `mkdir`/`mknod` — idempotent, no kernel-side fs writes
    needed (the devtmpfs-population pattern).
  - `crates/kernel/user/fbtest.c`: opens `/dev/fb0`, queries dims,
    paints a magenta field + yellow band (deliberately distinct from
    the kernel's boot pattern) row-by-row via lseek+write.
  - **Gated by `make test-fb`**, which now runs BOTH checks: `kernel`
    (M1 boot pattern) and `user` (drives the guest shell to run
    `/fbtest`, then screendumps and verifies the userspace pattern).
    Both PASS; full usertests still green both arches.
  - Not done (deliberate): mmap of the framebuffer (zero-copy) needs
    a phys-page VMA kind — deferred until the display server (M4)
    shows row-write bandwidth is actually a bottleneck.
- **M3 — input: DONE (2026-06-11).**
  virtio-input keyboard → `/dev/input/0` raw evdev event stream.
  - `crates/kernel/src/driver/virtio_input.rs`: third virtio-mmio
    slot (riscv `0x10003000`/IRQ 3, aarch64 `0x0a000400`/INTID 50 —
    shares VIRTIO0's page, map deduped). Simplest virtio device yet:
    no device-specific features (VERSION_1 only), eventq (queue 0)
    with 16 pre-posted 8-byte device-writable buffers — the device
    writes exactly one `{le16 type, le16 code, le32 value}` event per
    buffer; statusq (LEDs) legitimately left unconfigured (QEMU
    activates on DRIVER_OK with no queue-ready checks). IRQ handler
    drains used ring → cooked VecDeque (capped 256 events, drop on
    overflow like evdev) → `WakerList::wake_all` (multi-waiter rule).
  - `File::Input` fd variant, `INPUT_MAJOR = 2`; read blocks via a
    two-phase `InputWait` future whose readiness mirrors the read
    loop's admission (the begin_op lesson); O_NONBLOCK + empty → -1;
    poll(POLLIN) wired. write → -1. init mknods `/dev/input/0`.
  - Events mirror Linux evdev: key press = EV_KEY[code,1] + EV_SYN,
    release = EV_KEY[code,0] + EV_SYN; codes are Linux keycodes
    (X=45, ENTER=28). Userspace filters — same contract as evdev.
  - aarch64 GIC API refactored: `gic::init(&[spis])` /
    `init_for_hart(&[spis], timer_ppi)` — stops the signature growing
    with every device.
  - **Gated by `make test-input`** (`scripts/test-input.py`): runs
    `/kbtest` from the guest shell, injects `x` via QMP `send-key`
    (which routes ONLY to the virtio keyboard — serial is a separate
    input path, so the shell stays clean), verifies KEY_X(45)
    down+up reach userspace. PASS. Full usertests + test-fb still
    green both arches.
- **M4 — display server + window protocol + demo clients: DONE
  (2026-06-11).** The Wayland shape the plan called for: kernel
  exposes raw pixels + input, userspace does the windowing.
  - `user/wm.c` (display server, a plain user process): owns
    `/dev/fb0` + `/dev/input/0`, listens on the AF_UNIX socket
    `/wm.sock` (unlinks stale node first), and runs ONE cooperative
    `poll()` loop multiplexing keyboard + listener + client fds.
    Windows tile in fixed 168px slots with 2px white borders; focus =
    most recently created `want_keys` client; closed/EOF'd clients
    get their rect cleared back to the desktop color.
  - `user/wm.h` (protocol): `[WM_CREATE, w, h, want_keys] → [id]`,
    `[WM_BLIT, x, y, w, h] + w*h*4 XRGB8888 bytes` (clipped), and
    raw 8-byte input events server→client for want_keys windows.
    Events only go to clients that asked — a non-reading client can
    never wedge the server's blocking event write.
  - Demo clients: `hello_wm` (cyan 120×90 window, repaints red on
    the first key routed to it), `clock` (color-cycles ~1 Hz —
    proves multi-window compositing + independent animation).
  - `guidemo` launcher forks wm → hello_wm → clock with staggered
    sleeps (the tiny sh has no `&` background jobs).
  - **Gated by `make test-gui`** (`scripts/test-gui.py`): runs
    guidemo, screendumps (hello center cyan, border white, clock
    center ∈ palette), injects a key via QMP send-key, confirms
    "hello: key 45" + a red repaint in a second screendump. PASS.
  - Found along the way: concurrent procs' console prints interleave
    per-byte (no line atomicity) — test harnesses must not
    string-match output that races; pixel checks are the robust
    assertion.
  - Original todo verification, all met: display server draws ✓;
    clock client connects and ticks ✓; input events from the QEMU
    keyboard reach the right client window ✓.

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
