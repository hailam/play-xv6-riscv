# 40: VFS abstraction layer, `tmpfs`/`procfs`, and external `#![no_std]` filesystems (`ext4_rs` / `fatfs`)

**Status:** Pending — Phase 3 milestone (needs discovery, research & verification)
**Depends on:** `31-syscall-modularization`, `34-pseudo-devices-and-tty`
**Scope:** `crates/kernel/src/fs/`, `crates/kernel/src/file.rs`, `crates/kernel/src/syscall/`, `crates/kernel/Cargo.toml`

## Problem statement

`crates/kernel/src/syscall.rs` and `crates/kernel/src/file.rs` are currently hardcoded against `crates/kernel/src/fs/inode::Inode` (the xv6 on-disk format) and explicitly bracket every mutating syscall with `fs::log::begin_op().await` / `end_op().await`. This prevents:
1. Mounting in-memory pseudo-filesystems that Unix software expects (`/tmp` via `tmpfs`, `/proc/self/...` via `procfs`, `/dev` via `devfs`).
2. Plugging in mature third-party `#![no_std]` + `alloc` Rust filesystem crates (`ext4_rs`, `lwext4_rust`, `fatfs`) or QEMU host-shared directories (`virtio-9p` / `virtio-fs`).

## Discovery & research checklist

1. **Design an async-compatible VFS trait surface (`fs/vfs.rs`):**
   - Audit all `Inode` and `fs::dir` / `fs::path` operations used by `file.rs`, `proc.rs`, `user_vm.rs`, and `syscall.rs` (`lookup`, `create`, `mkdir`, `rmdir`, `unlink`, `link`, `symlink`, `readlink`, `rename`, `read_at`, `write_at`, `truncate`, `stat`, `readdir`, `chmod`, `chown`).
   - Move `fs::log::begin_op()` / `end_op()` inside the `Xv6Fs` VFS implementation so syscalls invoke VFS methods without knowing whether the underlying mount uses a write-ahead log.
   - Design a mount table supporting longest-prefix path resolution across `/`, `/dev`, `/tmp`, `/proc`, and optional secondary block/host mounts (`/mnt`).
2. **Research the sync-vs-async block device bridge for external crates (`ext4_rs`, `fatfs`):**
   - Our kernel's `virtio_blk` and `bio` layers are `async fn`-based, whereas most existing `#![no_std]` filesystem crates (`ext4_rs`, `fatfs`) define synchronous `BlockDevice` traits (`fn read_offset(&self, ...)`).
   - Evaluate the three architectural options for bridging them:
     - **Option A (Async-native or `embedded-io-async` crate):** Check whether any `#![no_std]` ext4/FAT crate supports `async fn` block traits (Rust 1.75+ native AFIT).
     - **Option B (Buffer-cache pre-fetch / sync virtio fallback):** Revisit `docs/todos/revisit/sync-virtio-fallback/` — determine whether a synchronous block-device shim backed by `bio` cache hits + interrupt/poll completion on misses is safe on our cooperative per-hart executor, or if it risks starving other tasks on the same hart.
     - **Option C (`virtio-9p` async transport):** Implement a small native `async` VirtIO-9P client (`crates/kernel/src/driver/virtio_9p.rs`) that speaks 9P2000.L over an async virtqueue, mounting a host directory directly with zero sync/async mismatch.
3. **Specify `tmpfs` and minimal `procfs` requirements:**
   - `tmpfs` (`/tmp`): page-alloc / `Vec<u8>` backed files and directories in RAM; needed by compilers (`tcc`), `sqlite`, `tar`, and shell heredocs (`<<EOF`).
   - `procfs` (`/proc`): `/proc/self/exe` (symlink or path to current binary, heavily used by Rust `std::env::current_exe()` and `uutils/coreutils` multicall dispatch!), `/proc/mounts`, `/proc/meminfo`, and `/proc/<pid>/stat`.

## Verification

- Verify all 74 existing tests pass against `/` (`Xv6Fs`) through the new VFS layer with no regressions.
- Mount `tmpfs` at `/tmp` and `procfs` at `/proc`; verify `/tmp` file creation/deletion/mmap and `readlink("/proc/self/exe")`.
- Mount a secondary external filesystem image (`ext4` or `FAT32`) or `virtio-9p` share at `/mnt` and run file + directory regression tests across mount boundaries.
