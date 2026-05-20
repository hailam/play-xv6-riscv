# 15: xv6 binary + source compatibility

**Status:** Pending — **next**
**Estimated:** 2–3 sessions (~400 LoC kernel + ~400 LoC user-runtime port from xv6)
**Depends on:** —
**Unblocks:** running unmodified xv6 user programs and (eventually)
xv6's full `usertests.c` against our kernel; mounting xv6's own
`fs.img` on us and ours on xv6.

## Goal

A user binary compiled against xv6 upstream's `user/user.h` + ulib
should load and run on our kernel. Conversely, xv6 upstream should
mount the `fs.img` our `mkfs` produces. Verified by running xv6's
`user/usertests.c` (3246 LoC, ~52 distinct test functions) against
our kernel.

## What's already compatible

The full audit (against `/Users/maix/apps/clang/xv6-riscv`) found
these surfaces are already byte-for-byte identical:

- All 21 syscall numbers (kernel/syscall.h vs uapi.rs).
- `O_RDONLY/WRONLY/RDWR/CREATE/TRUNC` (kernel/fcntl.h vs uapi.rs).
- `T_DIR/T_FILE/T_DEVICE` (kernel/stat.h vs xv6-fs-layout).
- `struct superblock` — 8×u32, 32 bytes (kernel/fs.h vs xv6-fs-layout).
- `struct dinode` — type/major/minor/nlink/size/addrs[NDIRECT+1], 64 bytes.
- `struct dirent` — `inum:u16, name:[u8;14]`, 16 bytes.
- `DIRSIZ=14`, `NDIRECT=12`, `FSMAGIC=0x10203040`, `NINODES=200`.
- Disk-layout convention: `[boot | sb | log | inodes | bmap | data]`.
- mkfs little-endian encoding.

## What diverges — the gap list

### G1. `BSIZE` 1024 vs 512  **[CRITICAL — disk-format break]**

- xv6: `BSIZE = 1024` (kernel/fs.h:6).
- us:  `BSIZE = 512` (xv6-fs-layout/src/lib.rs:6).

Effect: an xv6 `fs.img` is unreadable by us (every block is half
of what we expect to read; our bread of "block 1" lands in the
middle of xv6's block 1). Inode-per-block (`IPB`) and indirect-
block fanout (`NINDIRECT`) also flip — 256 vs 128.

Fix: change `BSIZE` in `xv6-fs-layout` to 1024. Cascades through
`bio::Buffer.data`, the virtio request descriptors (which now
need 1024-byte buffers), and `mkfs`. Re-run the existing test
suite end-to-end to confirm no buffer-cache or log assumption
broke.

### G2. `struct stat` 24 vs 20 bytes  **[CRITICAL — fstat ABI break]**

- xv6 (kernel/stat.h:5):
  ```c
  struct stat { int dev; uint ino; short type; short nlink; uint64 size; };
  ```
  20 bytes, NO padding (the trailing `uint64 size` is on a 4-byte
  offset, not 8 — xv6 just doesn't `#pragma pack` it).
- us (uapi.rs:35):
  ```rust
  pub struct Stat { pub dev: i32, pub ino: u32, pub typ: i16,
      pub nlink: i16, pub _pad: u32, pub size: u64 }
  ```
  24 bytes — we forced 8-byte alignment for `size`.

Effect: xv6 binaries doing `fstat(fd, &st)` would read `size`
from the wrong offset.

Fix: drop `_pad`, use `#[repr(C, packed)]` (or `repr(C)` and
manually serialize). The latter is safer — `repr(C, packed)` is a
foot-gun in Rust. Either copy field-by-field in `sys_fstat`, or
define a `#[repr(C)]` private 20-byte layout via `[u8; 20]` and
write field bytes individually.

### G3. `LOGSIZE` 31 vs 30  **[CRITICAL — log-region length break]**

- xv6: `LOGBLOCKS=30` + 1 header block → `nlog=31` (param.h:10,
  fs.h:24).
- us: `LOGSIZE=30` (xv6-fs-layout/src/lib.rs:12).

Effect: xv6's superblock would say `nlog=31` so `inodestart` is
block 33; ours says `nlog=30` so `inodestart` is block 32. One
block of disk offset everywhere.

Fix: rename our `LOGSIZE` → `LOGBLOCKS=30`, compute `nlog =
LOGBLOCKS + 1` in mkfs, update `fs::log::init` to take both the
header block + the data range. Then the on-disk superblock matches
xv6.

### G4. `wait()` signature  **[CRITICAL — wait ABI break]**

- xv6: `int wait(int *status)` (user/user.h:8). Returns child pid;
  writes exit status to `*status`.
- us: `int wait(void)`. Returns exit status directly; ignores `a0`.

Effect: an unmodified xv6 binary doing `int s; int pid =
wait(&s);` gets exit code in `pid`, garbage in `s`. Anything that
branches on `WIFEXITED`-equivalents misbehaves.

Fix: `sys_wait(proc)` now reads `a0` as `status_va`. If non-zero,
`translate_user` + write 4-byte exit code. Return the child pid
(not the exit code). `Wait` future returns `(pid, exit_code)`.

### G5. `sbrk(int, int)` two-arg form

- xv6: `char* sbrk(int n, int lazy)` (user/user.h:24 + user/ulib.c
  lazy variant). `lazy != 0` means just bump `proc.size` without
  mapping pages; faults map them on demand.
- us: `int sbrk(int n)` only.

Effect: xv6 binaries using `sbrklazy` would silently get eager
behavior. Compatible-but-degraded — they still work, just allocate
all pages upfront. `usertests`' `lazy_alloc` test would fail.

Fix: accept the `mode` arg. For `lazy != 0`, only bump `proc.size`,
add a page-fault handler in `usertrap` that maps the page on
demand. That handler then needs the cancellation+kill path too
(this overlaps with G6).

### G6. Fault handling: `panic!` vs `setkilled`  **[CRITICAL — kernel-vs-process death]**

- xv6 (trap.c:38):
  ```
  printf("usertrap(): unexpected scause 0x%lx pid=%d\n", ...);
  setkilled(p);  // process killed cleanly; kernel continues
  ```
- us (usertrap.rs:68):
  ```
  _ => panic!("usertrap: scause={:#x} ...", scause, ...);
  ```

Effect: A faulting user program takes down the whole kernel
instead of just dying. `usertests` deliberately faults
(`stacktest`, `pgbug`, `badwrite`) — would crash us on the first.

Fix: unknown-scause arm: print the same kind of diagnostic, set
`proc.killed = true`, return from usertrap so the executor's next
poll routes through `proc_main` → `sys_exit(-1)`. No `panic!`.

### G7. User C runtime (`ulib.c` + `printf.c`)  **[MISSING — link-break]**

- xv6 user/ulib.c (161 LoC): `strcpy/strcmp/strlen/strchr/memset/
  memmove/memcmp/memcpy/gets/stat/atoi/sbrk/sbrklazy`.
- xv6 user/printf.c (132 LoC): `putc/printint/printptr/vprintf/
  fprintf/printf`.
- us: only the asm syscall wrappers in `ulib.S`. No C-library
  helpers, no printf.

Effect: any xv6 source-compiled binary that uses `printf("hi
%d\n", n)` or `strlen()` won't link.

Fix: copy `user/ulib.c` and `user/printf.c` verbatim from xv6
into our `user/`. Wire `build.rs` to compile each alongside
ulib.S + umalloc.c. Total addition: ~290 LoC of *upstream*
xv6 source, ported verbatim.

Side benefit: our user binaries can immediately stop hand-rolling
their `u_strlen` / `put_u64` helpers.

### G8. naming: `SYS_pause` vs `SYS_sleep` (cosmetic)

- xv6 calls syscall 13 `SYS_pause` (kernel/syscall.h:14).
- We call it `SYS_SLEEP`.

Same number (13), same semantics. xv6's `user/user.h:25` does
`int pause(int);` — caller name. Our ulib.S exports `sleep`.

Effect: zero on the wire, but xv6's `sh.c` calls `sleep` (which
their ulib.c maps to `pause`); ours just exports `sleep` directly.
Fine. Could rename for surface-level parity but not required.

### G9. mkfs `nlog` field-on-disk

xv6 writes `sb.nlog = 31` (header + 30 log blocks). We write
`sb.nlog = 30`. Direct consequence of G3 above; fix is shared.

## Implementation order (proposed)

Each step has its own verify gate.

1. **G6 first** (`panic` → `setkilled`) — cheapest, ~30 LoC, fixes
   "any fault crashes the kernel," lets us run binaries that
   intentionally fault.
2. **G7** (port `ulib.c` + `printf.c`) — pure user-side, no kernel
   risk. Verify by rewriting one of our user binaries (`ls`,
   `cat`) to use `printf` instead of hand-rolled formatting.
3. **G4** (`wait(int*)`) — touches the kernel/user boundary. ~30
   LoC. Update our `sh.c` to use the new signature so we don't
   regress.
4. **G2** (struct stat layout) — touches `sys_fstat`'s
   serialization. Update our `ls.c` accordingly.
5. **G1 + G3 + G9 together** (BSIZE 1024, LOGBLOCKS 30+1, nlog 31)
   — disk-format change. Rebuild fs.img; rerun every existing
   test; bio cache buffers grow 2x.
6. **G5** (lazy sbrk) — needs the page-fault path (G6's hook
   gets reused for the "map on demand" case). ~80 LoC.
7. **usertests porting** — once G1–G7 land, port `user/usertests.c`
   and start fixing whatever it surfaces. Estimate: 2x rounds of
   bug-find / bug-fix because the test deliberately exercises
   edge cases (e.g. `fsfull`, `kernmem`, `validatetest`).

## Verification

- `make fs.img && make qemu` — every existing user binary still runs.
- New: `cargo build` includes ported `printf.c`, `ulib.c` from
  xv6/user/.
- New user binary `xv6_compat_test.c`: calls `wait(&status)` on
  a forked child that `exit(42)`s, asserts `pid > 0` and
  `status == 42`. fstat's a known file, prints `st.size`,
  asserts it matches. Triggers a page fault on purpose,
  expects to be killed (parent sees `wait` return -1).
- Eventually: run xv6's full `user/usertests.c`. Expect partial
  pass at first; track residual failures.

## Risks

- **BSIZE change is invasive.** Every place that uses `BSIZE =
  512` for buffer slicing needs review. The buffer-cache slot
  arithmetic, mkfs's per-block writes, the per-block log layout
  — all need to scale. Plan a careful audit pass.
- **Stat layout change** could break our own user binaries that
  read `st.size`. Audit `cat.c`, `ls.c`, `wr.c` and rebuild
  fs.img.
- **`usertests` is 3246 LoC and unforgiving.** Some tests
  deliberately exhaust resources (`fsfull`, `manywrites`). Our
  log might overflow, our inode cache might fill, etc. Plan for
  several follow-up bugfix passes.

## Code touch points

- `crates/xv6-fs-layout/src/lib.rs` — BSIZE, LOGSIZE rename.
- `crates/kernel/src/uapi.rs` — Stat layout.
- `crates/kernel/src/syscall.rs` — sys_wait sig, sys_sbrk lazy, fstat serializer.
- `crates/kernel/src/usertrap.rs` — fault → killed path.
- `crates/kernel/src/driver/bio.rs` — BSIZE-sized buffers.
- `crates/kernel/src/driver/virtio_blk.rs` — descriptor lengths.
- `crates/kernel/src/fs/log.rs` — LOGBLOCKS rename.
- `mkfs/src/main.rs` — write nlog=31, BSIZE=1024.
- `crates/kernel/user/ulib.c` (new — copy from xv6).
- `crates/kernel/user/printf.c` (new — copy from xv6).
- `crates/kernel/user/usertests.c` (new — copy from xv6, final step).
- `crates/kernel/build.rs` — link ulib.c + printf.c into each C binary.
- `crates/kernel/user/sh.c` — update to use new `wait(int*)`.
- `crates/kernel/user/ls.c` — update for the new stat layout +
  optionally rewrite using `printf`.
