# 05: file syscalls

**Status:** Pending
**Estimated:** ~200 LoC
**Depends on:** `04-fs-inode-and-dir`
**Unblocks:** real userland — `cat /etc/foo`, `ls /`, etc.

## Why

The fd table already supports `File::Console` and `File::Pipe*`. Adding
`File::Inode(Arc<Inode>, off)` lets `sys_read`/`sys_write` route to the
inode read/write paths, and unlocks the rest of the POSIX-ish syscalls.

## Approach

### Extend `File` enum

```rust
pub enum File {
    Console,
    PipeRead(Arc<PipeInner>),
    PipeWrite(Arc<PipeInner>),
    Inode(Arc<Inode>, AtomicU32 /* offset */),  // new
}
```

`File::Clone` increments the inode's ref count (via `iget`-equivalent
helper); `File::Drop` decrements (via `iput`).

### New syscalls

```rust
async fn sys_open(proc, path_va, flags) -> i64;     // SYS_OPEN = 15
async fn sys_close already exists                   // SYS_CLOSE = 21
async fn sys_fstat(proc, fd, stat_va) -> i64;       // SYS_FSTAT = 8
async fn sys_mkdir(proc, path_va) -> i64;           // SYS_MKDIR = 20
async fn sys_mknod(proc, path_va, major, minor) -> i64; // SYS_MKNOD = 17
async fn sys_unlink(proc, path_va) -> i64;          // SYS_UNLINK = 18
async fn sys_link(proc, old_va, new_va) -> i64;     // SYS_LINK = 19
async fn sys_chdir(proc, path_va) -> i64;           // SYS_CHDIR = 9
```

All numbers already in `uapi.rs`. Just wire them up.

### Update sys_read/sys_write

```rust
File::Inode(ip, off) => {
    let mut o = off.load(Ordering::Acquire);
    let n = inode::readi(ip, buf_slice, o).await?;
    off.store(o + n, Ordering::Release);
    Ok(n as i64)
}
```

### Update sys_exec

Switch from `embed::find(&path)` to `fs::namei(&path)` followed by
`readi` to load the ELF bytes. The `embed::INITCODE` constant stays —
it's the initial proc, doesn't go through the fs.

This is where the "exec from disk" milestone happens. Removing the
embedded `/echo`, `/hello`, `/cat`, `/sh` constants is the final step.

### User-side syscall wrappers in `ulib.S`

```asm
DEFINE_SYSCALL open,   15
DEFINE_SYSCALL fstat,  8
DEFINE_SYSCALL mkdir,  20
DEFINE_SYSCALL mknod,  17
DEFINE_SYSCALL unlink, 18
DEFINE_SYSCALL link,   19
DEFINE_SYSCALL chdir,  9
```

## Verification

- `mkfs` puts `/echo` (the ELF) into the root directory.
- Shell types `/echo hello`; shell forks; child execs `/echo`; kernel
  resolves via `namei`, loads via `readi`, success.
- Shell can `cat /init` to print the init binary's bytes.

## Risks

- `Inode::Drop` triggering `iput` which may write to disk (free-on-zero-refs)
  → must be in a `begin_op`/`end_op` for the log layer. Track whether
  `iput` is called inside a transaction or starts its own.
- Same `Send`-future issue as `sys_exec`'s argv handling; raw inode
  pointers should be wrapped or use `Arc`.

## Code touch points

- `crates/kernel/src/file.rs` — add `Inode` variant + Drop logic
- `crates/kernel/src/syscall.rs` — new syscall handlers
- `crates/kernel/src/syscall.rs::sys_exec` — switch to `namei` + `readi` loader
- `crates/kernel/src/embed.rs` — keep only `INITCODE`; remove user bins
- `crates/kernel/user/ulib.S` — new syscall wrappers
