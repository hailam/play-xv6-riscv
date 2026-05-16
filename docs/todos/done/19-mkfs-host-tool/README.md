# 19: mkfs — host tool to build fs.img

**Done.** Standalone `mkfs/` host binary that writes a 1 MiB fs.img with
valid superblock, zeroed log region, inode table, data bitmap, and a
root directory populated from `name:path` command-line entries.

## Verification

`make fs.img` builds with the embedded user binaries:

```
mkfs: layout sb=1 log=2..33 inodes=33..58 bmap=58 data=59..2048
  /init     → inum 2 (360 bytes)
  /echo     → inum 3 (672 bytes)
  /sh       → inum 4 (1248 bytes)
  /cat      → inum 5 (592 bytes)
  /hello    → inum 6 (376 bytes)
  /pipetest → inum 7 (480 bytes)
mkfs: fs.img written (1048576 bytes, 70 blocks used)
```

Host-side `dd` confirms:
- Superblock at block 1 (magic 0x10203040, all fields correct)
- Log header at block 2 = zero (no pending recovery)
- Root inode at block 33+offset 64: typ=T_DIR, nlink=1, size=128, addrs[0]=59
- Block 59 (root dir data): 8 dirents `(inum, name)` — `.`, `..`, init, echo, sh, cat, hello, pipetest
- Data bitmap at block 58: blocks 0..69 marked allocated

The kernel still boots cleanly against the new fs.img and runs the log smoke test (the log header is correctly read as empty at boot; the transaction commits and clears it).

## Notes

- mkfs is in the workspace but excluded from `default-members` — only built when explicitly requested with `cargo build -p mkfs --target=$(HOST)`. Otherwise the default riscv64 target would fail (mkfs uses std).
- `crates/kernel/build.rs` copies the stripped user ELFs to
  `target/user/<name>.elf` so the Makefile can find them at a stable path.
- Layout constants (`BSIZE`, `NDIRECT`, `LOGSIZE`, etc.) are currently
  duplicated between `mkfs/src/main.rs` and (future) kernel `fs/` code.
  When `pending/04-fs-inode-and-dir` lands, factor into a shared
  `crates/xv6-fs-layout/` crate (the plan for 04 mentions this).
- mkfs doesn't yet support double-indirect blocks; max file size = 12 +
  128 = 140 blocks = 70 KiB. Plenty for our user binaries.

## Files

- New: `mkfs/Cargo.toml`, `mkfs/src/main.rs` (~270 LoC)
- `crates/kernel/build.rs` — copies stripped ELFs to `target/user/`
- `Makefile` — `mkfs` target + `fs.img` rule chains kernel build → mkfs → fs.img
- `Cargo.toml` — `mkfs` added to workspace, `default-members = ["crates/kernel"]`
