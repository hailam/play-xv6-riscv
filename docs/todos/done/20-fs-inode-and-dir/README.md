# 20: fs — inode + directory + path resolution  [DONE]

The chunky middle of the filesystem. With this landed, the kernel
walks `/some/path/file` and reads its bytes.

## What landed

Boot-time verification on hart 0:

```
fs: superblock OK (size=2048, log@2, inodes@33, bmap@58)
fs: / contents (128 bytes of dirents):
  inum   1  .
  inum   1  ..
  inum   2  init
  inum   3  echo
  inum   4  sh
  inum   5  cat
  inum   6  hello
  inum   7  pipetest
fs: /echo typ=2 size=672 bytes, read 32
     first 16 bytes: 7f 45 4c 46 02 01 01 00 00 00 00 00 00 00 00 00
fs: ELF magic confirmed — exec-from-disk path is unblocked!
```

`namei("/echo")` resolved, `readi` returned the file's first 32 bytes,
and the ELF magic `7f 45 4c 46` is present — meaning we can build
`sys_exec`-from-disk on top of this without further fs work.

## Files

- New: `crates/xv6-fs-layout/{Cargo.toml,src/lib.rs}` — shared
  on-disk types/constants (`Superblock`, `DInode`, `Dirent`,
  `BSIZE`, `FSMAGIC`, `IPB`, `NDIRECT`, `NINDIRECT`, `T_*`, `DIRSIZ`,
  `LOGSIZE`, `NINODES`). Both `mkfs` (host) and kernel consume it.
- New: `crates/kernel/src/fs/superblock.rs` (~25 LoC) — `init()` reads
  block 1 once at boot, asserts magic, caches in a `SpinLock`.
- New: `crates/kernel/src/fs/inode.rs` (~230 LoC) — `Inode` with
  `locked: AtomicBool` + `WakerCell` sleeplock-equivalent;
  `LockedInode<'_>` RAII guard; `iget`, async `ilock`,
  `load_from_disk`, `readi`, `bmap` (direct + 1-level indirect).
- New: `crates/kernel/src/fs/dir.rs` (~85 LoC) — `dirlookup` and
  `for_each_entry` iteration helper.
- New: `crates/kernel/src/fs/path.rs` (~60 LoC) — `namei` /
  `nameiparent` over the (cwd-less for now) root.
- Updated: `crates/kernel/src/fs/mod.rs`, `crates/kernel/Cargo.toml`,
  `crates/kernel/src/main.rs::disk_smoke_test`.
- Refactor: `mkfs/src/main.rs` now imports types from
  `xv6-fs-layout` instead of duplicating constants.

Total new kernel code: ~400 LoC. Unsafe lines added: ~12, all in
`UnsafeCell` access on `Inode.state` (gated by `locked == true` and
the `LockedInode` guard) and `read_unaligned` for on-disk struct
loads.

## Design notes

- The "sleeplock" pattern: `AtomicBool locked` + per-inode
  `WakerCell`. `ilock(&ip).await` registers a waker, double-checks
  the state, parks. `Drop` of `LockedInode` clears the bool and
  wakes any registered task. This avoids a queue: at most one async
  task is parked per inode at a time, and the next contender wakes
  itself on the spurious wake (it'll just re-poll the CAS).
- Inode cache uses `Arc::strong_count == 1` for free-slot detection,
  matching the buffer cache.
- `namei` always starts from inode 1. Per-proc `cwd` lands with
  `05-file-syscalls`.

## Verified at

`make fs.img && qemu-system-riscv64 -M virt ... -drive file=fs.img,...`
