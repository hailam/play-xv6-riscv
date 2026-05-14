# 04: fs — inode + directory + path resolution

**Status:** Pending
**Estimated:** ~400 LoC
**Depends on:** `03-log-wal` (writes go through log), `02-bio-write`
**Unblocks:** `05-file-syscalls`, `06-mkfs-host-tool`

The chunky middle of the filesystem. Once this lands, the kernel can
walk `/some/path/file` and read its bytes.

## Why

Without an inode layer, the disk is just an array of blocks. xv6's fs
adds:

- **Superblock** — disk layout constants
- **Inode bitmap + data bitmap** — track allocation
- **Inodes** — file metadata (type, size, direct + indirect block ptrs)
- **Directory entries** — `(name, inum)` pairs in directory inodes
- **Path resolution** — walk slash-separated paths starting from root

## Approach — follow xv6 `fs.c` closely

### On-disk layout (`mkfs` builds this)

```
  block 0:       boot (unused)
  block 1:       superblock (n_blocks, n_inodes, log_start, log_size, ...)
  blocks 2..L+1: log
  blocks L+2..:  inodes (NINODES / IPB per block)
  blocks ..:     inode bitmap (1 bit per inode)
  blocks ..:     data bitmap (1 bit per block)
  blocks ..:     data
```

### On-disk inode (`dinode`)

```rust
#[repr(C)]
struct DInode {
    typ: u16,       // T_DIR / T_FILE / T_DEV
    major: u16,
    minor: u16,
    nlink: u16,
    size: u32,
    addrs: [u32; NDIRECT + 1],  // NDIRECT direct, 1 indirect
}
```

`NDIRECT = 12`, indirect block holds another `NINDIRECT = BSIZE/4 = 128`
pointers. Max file = (12 + 128) * 512 = 71 680 bytes (~70 KiB). Fine
for our purposes; double-indirect can come later.

### In-memory inode (`Inode`)

```rust
pub struct Inode {
    pub dev: u32,
    pub inum: u32,
    pub valid: AtomicBool,
    pub state: SpinLock<InodeState>,
}

pub struct InodeState {
    pub typ: u16,
    pub nlink: u16,
    pub size: u32,
    pub addrs: [u32; NDIRECT + 1],
    pub locked: bool,        // sleeplock-equivalent
    pub locked_waker: WakerCell,
}
```

`Arc<Inode>` is the shareable handle. The inode cache (similar to bio)
maps `(dev, inum)` to `Arc<Inode>`.

### Key functions

```rust
pub async fn iget(dev: u32, inum: u32) -> Arc<Inode>;   // get cached inode
pub async fn ilock(ip: &Arc<Inode>);                    // sleeplock-equivalent; loads from disk if !valid
pub fn iunlock(ip: &Arc<Inode>);
pub async fn iput(ip: Arc<Inode>);                      // drop one ref, free if last

pub async fn readi(ip: &Arc<Inode>, dst: &mut [u8], off: u32) -> usize;
pub async fn writei(ip: &Arc<Inode>, src: &[u8], off: u32) -> usize;  // calls log_write

pub async fn dirlookup(dp: &Arc<Inode>, name: &str) -> Option<Arc<Inode>>;
pub async fn dirlink(dp: &Arc<Inode>, name: &str, inum: u32) -> Result<(), ()>;

pub async fn namei(path: &str) -> Option<Arc<Inode>>;
pub async fn nameiparent(path: &str) -> Option<(Arc<Inode>, alloc::string::String)>;
```

### Cancellation

Every `.await` here should check `proc.killed` once `07` lands. Until
then, requests can hang on a killed proc — acceptable for now.

## Verification

Stage 1 (read-only):
- Boot, kernel reads superblock from block 1.
- `namei("/init")` returns an inode for /init (assumes `mkfs` populated it).
- `readi` of /init's data returns the ELF bytes; identical to embedded `/echo`.

Stage 2 (writes):
- Within a `begin_op` / `end_op`, allocate an inode, populate it, link
  into a directory.
- Re-open the file by path, read it back.

## Risks

- xv6's lock ordering: bio lock → log lock → inode lock. Async equivalents
  must respect this to avoid deadlock-like wait cycles.
- The `WakerCell`-as-sleeplock pattern is repetitive; consider a
  `SleepLock<T>` helper that wraps it.
- 200-line file as an async port is moderate; budget more than the
  estimate if locks get hairy.

## Code touch points

- New: `crates/kernel/src/fs/mod.rs` — re-exports
- New: `crates/kernel/src/fs/superblock.rs`
- New: `crates/kernel/src/fs/inode.rs`
- New: `crates/kernel/src/fs/dir.rs`
- New: `crates/kernel/src/fs/path.rs`
- Touches `proc.rs` for `cwd: Option<Arc<Inode>>` (per-proc current directory)
