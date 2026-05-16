# 13: fs writes — `mkdir`/`mknod`/`unlink`/`link` + `O_CREATE` + `writei`

**Status:** Pending
**Estimated:** ~300 LoC (kernel) + small ulib changes
**Depends on:** `21-file-syscalls-read-path` (DONE)
**Unblocks:** `usertests` write-side coverage, shell `>` / `>>`,
log-replay testing

## Why

The current fs is read-only from userspace. `21-file-syscalls-read-path`
stubbed every write-path syscall at -1. This todo wires them up
through the log: any change to disk goes through `begin_op` →
`log_write` → `end_op`.

## Scope

### Kernel-side additions

```rust
// crates/kernel/src/fs/inode.rs
pub async fn writei(li: &LockedInode<'_>, src: &[u8], off: u32) -> usize;
pub async fn iupdate(li: &LockedInode<'_>);     // flush in-memory inode → disk
pub async fn ialloc(dev: u32, typ: u16) -> Arc<Inode>;
pub async fn itrunc(li: &LockedInode<'_>);      // free all data blocks

// crates/kernel/src/fs/bmap.rs (new)
pub async fn balloc(dev: u32) -> u32;
pub async fn bfree(dev: u32, b: u32);

// crates/kernel/src/fs/dir.rs
pub async fn dirlink(dir: &LockedInode<'_>, name: &str, inum: u16) -> bool;
```

All of the above run inside an open transaction; each one calls
`log_write(buf)` instead of writing to disk directly.

### Syscall wiring

```rust
// crates/kernel/src/syscall.rs
async fn sys_chdir(proc, path_va)             -> i64; // per-proc cwd
async fn sys_mkdir(proc, path_va)             -> i64;
async fn sys_mknod(proc, path_va, maj, min)   -> i64;
async fn sys_unlink(proc, path_va)            -> i64;
async fn sys_link(proc, old_va, new_va)       -> i64;
// Extend sys_open to honour O_CREATE / O_TRUNC.
// Wire sys_write on File::Inode to writei.
```

### Per-proc cwd

Add `cwd: SpinLock<Option<Arc<Inode>>>` to `Proc`. Inherit on
`fork`; preserve across `exec`. `fs::namei` consults cwd when the
path doesn't start with `/`.

## Verification

- `ls` → mkdir foo → ls (foo appears) → unlink foo → ls (gone).
- `cat > greeting` (shell redirect, after redirect support lands) →
  read back via `cat greeting`.
- Reboot QEMU on the same fs.img, confirm changes survive (= the
  log replayed correctly on the previous shutdown).

## Risks

- **`iput`-on-zero-links must free the inode + its data blocks.**
  This is the one place where dropping an `Arc<Inode>` should write
  to disk. Easiest plan: `iput` becomes an explicit async function
  called from syscall sites (not from `Drop`), so the caller can
  wrap it in `begin_op` / `end_op`.
- **Log replay on dirty shutdown.** `fs::log::init` already runs
  the recovery path, but we haven't tested it after a real
  half-committed transaction. Add a deliberate-crash test before
  declaring this done.
- **Concurrency.** With write path live, two procs can race on the
  same directory. The existing `ilock` async sleeplock handles
  inode-level concurrency; the log already serialises commits.
  Worth re-auditing once we add `usertests`.

## Code touch points

- New: `crates/kernel/src/fs/bmap.rs`
- Touches: `crates/kernel/src/fs/inode.rs`,
  `crates/kernel/src/fs/dir.rs`,
  `crates/kernel/src/syscall.rs`,
  `crates/kernel/src/proc.rs` (cwd field),
  `crates/kernel/user/sh.c` (`>` / `>>` redirects, optional)
