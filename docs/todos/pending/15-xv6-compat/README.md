# 15: xv6 binary + source compatibility

**Status:** In progress — 5 of 9 gaps closed; usertests' `exitwait` passes.
**Estimated remaining:** 1–2 sessions for the rest + open-ended
usertests-driven bug hunting.
**Depends on:** —
**Unblocks:** running unmodified xv6 user programs and (eventually)
xv6's full `usertests.c` against our kernel; mounting xv6's own
`fs.img` on us and ours on xv6.

## Progress

| Gap | Status |
|---|---|
| G2 struct stat 24→20 | **NON-ISSUE** — verified `sizeof = 24` on the upstream C source via clang/gcc; C's natural alignment inserts the same 4-byte pad before `uint64 size` whether `_pad` is explicit or not. The audit was wrong on this one. |
| G4 `wait(int *status)` | **DONE** — `sys_wait` reads `a0` as user ptr, writes exit code, returns pid. `faulttest`/`killtest`/`smptest`/`sh.c` migrated. |
| G5 lazy `sbrk(int, int)` | **DONE** — kernel honours `lazy != 0`; new `lazy_map_page` hook in `usertrap.rs` maps zero frames on faults inside `[code_end, proc.size)`. New `/lazytest` exercises 8 lazy pages. |
| G6 user fault → killed | **DONE** — unknown scause prints diagnostic + sets `proc.killed`; no `panic!`. `/faulttest` deref's `0xdeadc0de` and asserts kernel survives + child reaped with status -1. |
| G7 `ulib.c` + `printf.c` port | **DONE** — ~210 LoC from xv6 verbatim, shared `user/user.h`, `build.rs` links into every C binary. `/xv6test` exercises `printf` / `strcpy` / `strlen` / `atoi` / `strcmp` / `memset` / `memmove` / `%p`. |
| G1 BSIZE 512→1024 | Deferred — only matters for cross-mounting xv6's actual `fs.img`; our internally-consistent BSIZE=512 stack works fine. usertests' `bigfile` (one specific test) will hit our 70 KB single-file ceiling and fail. |
| G3 + G9 LOGSIZE 30→31 + sb.nlog | Deferred — same reason as G1; affects only cross-mounting. |
| **TR (trampoline race in `return_to_user`)** | **FIXED** — the countfree loop crash turned out to be a missing `intr_off()` at the top of `usertrap.rs::return_to_user`. The kernel was setting `stvec = uservec` then doing some more work (set sepc, jump to `userret`) with S-mode interrupts ENABLED. If a timer interrupt fired in that window, `uservec` ran from S-mode and saved kernel register state into the user trapframe (a0 := TRAPFRAME, sp := kernel-stack, sepc := uservec-internal PC, etc.). The next `sret` would then jump the user PC into the trampoline page, faulting. Classic xv6-style bug; xv6 has `intr_off()` at the very top of `usertrapret` for exactly this reason. After the fix: unbounded `countfree` runs cleanly; broader usertests pass-rate jumped. |

## Bonus fixes shipped this round

- **`sys_sbrk` shrink now actually unmaps pages.** Added
  `PageTableOps::unmap_page(va) -> Option<PA>` to the trait;
  riscv64 impl walks to the leaf PTE, clears it, returns the freed
  PA. aarch64 impl is a skeleton placeholder. `sys_sbrk` shrink
  walks the unmapped range and `KFRAMES.free`s each frame. Without
  this, a second `sbrk(+n)` after `sbrk(-n)` returned `Remap`
  errors — caught by usertests' double-countfree pattern.
- **`xv6test` user binary** exercises the ported runtime end-to-end.
- **`faulttest` / `lazytest` user binaries** for G6 / G5 verification.

## Verified

usertests pass / fail (`/usertests <name>`):

| Test | Status |
|---|---|
| `exitwait` (100 fork/wait cycles) | ✅ OK |
| `bsstest` (BSS zero-init) | ✅ OK |
| `stacktest` (deliberate fault on invalid SP — kernel survives) | ✅ OK |
| `pgbug` (page-table bug probe) | ✅ OK |
| `createtest` (many file creates/unlinks) | ✅ OK |
| `copyin` | ❌ — `open(copyin1) failed` on the first call; specific to copyin/copyout's setup pattern; under investigation |
| `copyout` | ❌ — `open(README) failed`; same family |
| `writebig` | ❌ — fails at `i=140` because our `MAXFILE = NDIRECT + NINDIRECT = 12 + BSIZE/4 = 12 + 128 = 140` blocks, and `bigfile` writes 140 blocks of 1024 bytes each (140 KB). xv6's BSIZE=1024 → 140 KB fits; ours BSIZE=512 → 140*512 = 70 KB → block 140 is past `MAXFILE`. **This is G1.** |

## What was already compatible

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
