# 06: mkfs — host tool to build `fs.img`

**Status:** Pending
**Estimated:** ~150 LoC
**Depends on:** `04-fs-inode-and-dir` (uses the on-disk layout)
**Unblocks:** "exec from disk" — load `/sh`, `/echo`, etc. from a real fs

## Why

Currently `make fs.img` produces a 1 MiB blob with a banner at offset 0.
That's enough for the disk-driver smoke test but useless for real fs:
no superblock, no inodes, no root directory.

We need a host-side program that writes a valid fs.img with:
- Superblock
- Empty log region
- Root directory inode with entries pointing at `/sh`, `/echo`, `/cat`, `/hello`, `/pipetest`
- Inode + data blocks for each of those user binaries

## Approach

xv6 has `mkfs/mkfs.c`. We port the same algorithm to host Rust.

### Location

`xtask/` crate, or a new `mkfs/` crate. The latter is more honest about
what it is. New workspace member.

### CLI

```
mkfs fs.img <user_binary>...
```

Reads each user binary (already built as ELF by build.rs), writes to
the fs as a regular file. Hardcodes "/" as root, with entries for each.

### Algorithm

1. Decide layout constants: `NINODES`, `LOGSIZE`, `NBLOCKS_TOTAL`, etc.
2. Write superblock to block 1.
3. Zero the log region.
4. Reserve inode 1 for root directory; mark allocated in inode bitmap.
5. For each input binary:
   - Allocate inode N (mark in bitmap).
   - Allocate enough data blocks (mark in data bitmap).
   - Write data blocks with file contents.
   - Update inode N's `addrs[]` and `size`.
   - Add a dirent `(filename, N)` to root directory's data.
6. Write everything out.

### Wiring into the build

`build.rs` or a top-level Makefile target invokes `mkfs` after building
user binaries:

```
fs.img: user_binaries
    cargo run -p mkfs --release -- fs.img \
        target/.../sh.elf target/.../echo.elf \
        target/.../cat.elf target/.../hello.elf \
        target/.../pipetest.elf
```

## Verification

- `mkfs fs.img sh.elf` produces a fs.img.
- `od -An -tx1` of fs.img shows expected pattern (superblock magic at block 1).
- Booting kernel, calling `namei("/sh")`, returns an inode with the right size.
- `readi` of that inode returns bytes byte-for-byte identical to `sh.elf`.
- Shell exec `/sh` works (well, re-exec; sh execs sh).

## Risks

- Layout must EXACTLY match what the kernel's `fs` code expects. Use
  shared `repr(C)` structs in a new `xv6-fs-layout` crate? Or duplicate
  the constants. (Sharing is cleaner; recommended.)
- Byte order: both kernel and mkfs are little-endian (riscv64 + host
  x86_64/aarch64-Darwin). Safe; but document the assumption.

## Code touch points

- New crate: `mkfs/` (host bin, depends on `xv6-fs-layout`)
- New crate: `xv6-fs-layout/` (no_std, shared by kernel and mkfs) with on-disk struct definitions and constants
- Top-level Makefile: replace the `dd` hack with `cargo run -p mkfs`
- `crates/kernel/src/fs/superblock.rs` etc.: `use xv6_fs_layout::...`
