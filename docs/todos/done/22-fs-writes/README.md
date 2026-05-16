# 22: fs writes — `mkdir` / `O_CREATE` / `unlink` / `writei`  [DONE]

The fs is now read-write from userspace. mkdir creates new
directories, open(O_CREATE) creates new files, write goes through
writei → log, unlink removes entries (and frees the inode + data
blocks when the last name disappears).

## What landed

Scripted shell session (with each command separated by ~2s for the
async path to settle):

```
$ ls /
DIR   inum=1  size=192  .
... (init, echo, sh, cat, ls, mkdir, rm, wr, ...)
$ mkdir /foo
$ ls /
... + DIR   inum=12  size=32  foo
$ wr /greet hello from the disk
$ cat /greet
hello from the disk
$ rm /greet
$ ls /
... + DIR   inum=12  size=32  foo    (/greet gone)
```

Then a **second boot** against the same fs.img:

```
$ ls /
... + DIR   inum=12  size=32  foo
```

`/foo` survived; `/greet` is still absent — log replay + commit
ordering are working.

## Files

- New: `crates/kernel/src/fs/bmap.rs` (~80 LoC) — `balloc` / `bfree`.
  `balloc` zero-fills the freshly-allocated block (so callers see
  clean contents) and log_writes both the bmap block *and* the
  newly-allocated data block in the same transaction.
- Updated: `crates/kernel/src/fs/inode.rs` (+~190 LoC) —
  - `LockedInode::state_mut` (only callable through `&mut self`)
  - `bmap_or_alloc` (write-side counterpart of `bmap`)
  - `writei`, `iupdate`, `itrunc`, `ialloc`
- Updated: `crates/kernel/src/fs/dir.rs` (+~110 LoC) —
  - `dirlink` (reuses zeroed slots, refuses duplicates)
  - `dirunlink_at`, `dirlookup_full`, `dir_is_empty` helpers
- Updated: `crates/kernel/src/syscall.rs` (+~190 LoC) —
  - Routed `sys_write(File::Inode)` through `writei` inside
    `begin_op`/`end_op`
  - `sys_open` honours `O_CREATE` (allocates via `create_at_path`)
    and `O_TRUNC` (calls `itrunc`)
  - `sys_mkdir`, `sys_mknod`, `sys_unlink`
  - Shared `create_inside_op` / `unlink_inside_op` helpers
- New user binaries (~120 LoC total): `mkdir.c`, `rm.c`, `wr.c`.
  Cat extended to take filename args (in addition to stdin mode).

Total: ~570 LoC kernel + ~120 LoC user. Unsafe added: ~20 lines,
all `UnsafeCell`-via-`state_mut` and `read_unaligned` for on-disk
struct loads.

## Design notes

- Every fs-mutating syscall is a single transaction:
  `begin_op() → ... → end_op()`. ialloc, dirlink, writei, iupdate,
  itrunc, balloc, bfree each just `log_write(buf)` instead of
  `bwrite`. The commit path in `fs::log` flushes everything atomically.
- `dirlink` reuses zeroed (`inum == 0`) slots before extending the
  directory. `dirunlink_at` zeros the slot; the dir size never
  shrinks (matches xv6).
- Unlink-while-open: we drop the dirent immediately, but only free
  inode + blocks if `Arc::strong_count(&child_ip) <= 2` (the cache
  + our local). If a fd still holds it, the inode stays alive with
  nlink=0 and typ != 0 (lazy free comes with proper iput-on-Drop,
  deferred to a follow-up). This matches xv6's semantics modulo
  the fd-induced delay.
- Per-proc `cwd` is **not** in this iteration. All paths must start
  with `/`. Deferred to a follow-on (file naming below).
- `sys_link` and `sys_chdir` still stubbed at -1. Both are small
  follow-ons.

## Stuff deferred / written down

- `iput`-on-Drop for proper cross-fd lazy free.
- Per-proc cwd + `sys_chdir`.
- `sys_link` (hard links).
- Shell `>` / `>>` redirects (use `wr` for now).

These can land as a single small todo, or get folded into
`07-sys-kill-cancellation` once that's done — the kill audit will
need to pass through every `.await`, so adding cwd at the same time
is cheap.

## Verified at

- `make fs.img && qemu-system-riscv64 ... -drive file=fs.img,...`
  driven through stdin with mkdir/wr/cat/rm + a follow-up ls.
- A second boot against the modified fs.img: `/foo` durable, no
  errors from log replay.
