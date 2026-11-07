# 35: Lift filesystem filename limit (`DIRSIZ = 14`) and expand image capacity

**Status:** Pending — Phase 3 foundation (needs discovery, research & verification)
**Scope:** `crates/xv6-fs-layout/src/lib.rs`, `mkfs/src/main.rs`, `crates/kernel/src/fs/`, `crates/kernel/user/ls.c`, `crates/kernel/user/usertests.c`

## Problem statement

The on-disk xv6 directory entry (`crates/xv6-fs-layout/src/lib.rs`) uses a 16-byte `Dirent` (`u16 inum` + `[u8; 14] name`), capping filenames at 14 bytes (`DIRSIZ = 14`), and `FSSIZE` / block-addressing limits total disk capacity and maximum file size. Real Unix software (`uutils/coreutils`, `sbase`, `BusyBox`, `musl` test suites, Lua/Python scripts) requires longer filenames (at least 62 bytes) and enough filesystem blocks to hold multi-megabyte static ELF binaries.

*(Note: This task is the fast, low-risk upgrade to the in-tree async WAL filesystem; see [`40-vfs-and-external-filesystems`](../40-vfs-and-external-filesystems/) for the full VFS trait layer, `tmpfs`/`procfs`, and external `#![no_std]` filesystem crates like `ext4_rs` / `fatfs`.)*

## Discovery & research checklist

1. **Audit every `DIRSIZ` and `Dirent` assumption in kernel and userspace:**
   - Search `crates/xv6-fs-layout/`, `mkfs/`, `crates/kernel/src/fs/{dir.rs,path.rs,inode.rs}`, `crates/kernel/src/syscall.rs` (`sys_getdents`, `sys_unlink`, `sys_rename`), and `crates/kernel/user/*.c` (`ls.c`, `usertests.c`, etc.) for hardcoded `14` or `sizeof(struct dirent) == 16`.
   - Verify that widening `Dirent` to 64 bytes (`u16 inum` + `[u8; 62] name`) still divides `BSIZE = 1024` evenly (`16` dirents per block), preserving the invariant that no `Dirent` crosses a block boundary.
2. **Audit `FSSIZE`, `NINODES`, and `MAXFILE`:**
   - Check `mkfs/src/main.rs` and `crates/xv6-fs-layout/src/lib.rs` for `FSSIZE`, bitmap block count (`bmap` blocks — currently 1 bitmap block covers $1024 \times 8 = 8192$ blocks = 8 MiB; supporting multiple bitmap blocks scales the image to 64–128 MiB).
   - Check `crates/kernel/src/fs/bmap.rs` and `inode.rs` for multi-block bitmap support (`balloc`/`bfree` across `(block / BPB)` bitmap blocks).

## Verification

- Verify `mkfs` builds a larger `fs.img` / `fs-aarch64.img` with 62-byte `DIRSIZ` and multi-block bitmaps.
- Add tests creating, reading, renaming, symlinking, and unlinking files with 50+ character names, and writing files larger than 4 MiB.
- Ensure the full 74-test suite passes on both `riscv64` and `aarch64`.
