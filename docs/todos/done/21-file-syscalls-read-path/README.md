# 21: file syscalls (read path) + exec-from-disk  [DONE]

## What landed

Shell now spawns from disk. `ls /` walks a directory; `echo hello`
spawns and execs the on-disk `/echo`.

Captured boot, with stdin scripted to send `ls /` then `echo hello`:

```
fs: ready (sb@1, log@2..33, inodes@33..58, bmap@58, data@59..)
spawning init proc (360 bytes)
$ DIR   inum=1  size=144  .
DIR   inum=1  size=144  ..
FILE  inum=2  size=360  init
FILE  inum=3  size=760  echo
FILE  inum=4  size=1336  sh
FILE  inum=5  size=680  cat
FILE  inum=6  size=376  hello
FILE  inum=7  size=480  pipetest
FILE  inum=8  size=1576  ls
pid 2 exit(0)
$ hello from disk
pid 3 exit(0)
$
```

The `/sh` binary, the `/ls` binary, and the `/echo` binary are all
loaded by the in-kernel ELF loader off `fs.img` — no embedded user
binaries except `initcode`.

## Scope (read path only)

- New: `File::Inode { ip, off, readable, writable }` variant.
  `Clone` gives each fd its own offset (`Arc<Inode>` is shared);
  `Drop` is a no-op for inode fds (the cache holds them).
- New: `sys_open` (RDONLY / RDWR — no O_CREATE / O_TRUNC yet);
  `sys_fstat`; user-visible `struct stat` in `uapi.rs`.
- Routed: `sys_read` and `sys_write` now match on `File::Inode`.
  Read pulls via `inode::ilock` + `readi`; write returns -1 until
  `13-fs-writes`.
- Switched: `sys_exec` now calls `fs::namei` + `readi` to load the
  ELF, replacing `embed::find`. The `embed.rs` module shrunk to
  just `INITCODE`.
- Stubbed at -1: `sys_chdir`, `sys_mkdir`, `sys_mknod`, `sys_unlink`,
  `sys_link` — all need the write path.
- Real: `sys_getpid`, `sys_uptime`. Still -1: `sys_kill`, `sys_sbrk`
  (each has its own pending todo).
- User-side: added `open/fstat/chdir/mkdir/mknod/unlink/link/kill/`
  `getpid/sbrk/uptime` wrappers to `ulib.S`.
- New user binary: `crates/kernel/user/ls.c` (~120 LoC). Opens a
  path, fstats it; if it's a directory, reads dirents and fstats
  each child for the type/size column.

## Executor fix (incidental)

The executor was treating `Poll::Ready(())` the same as
`Poll::Pending` — it always put the task back in its slot, so a
stale waker firing after a task had completed would re-poll the
finished future and trigger `async fn resumed after completion`.
Fixed by only re-installing the task slot on `Pending`.

This bug only became reachable here because `bringup_then_init` is
the first kernel async task that runs to completion (the older
`disk_smoke_test` ended with `pending::<()>().await`). Worth
flagging because every future "fire-and-finish" kernel task needs
this.

## Files

- `crates/kernel/src/file.rs` — `File::Inode` variant + Clone.
- `crates/kernel/src/uapi.rs` — `Stat`, `O_*` flags.
- `crates/kernel/src/syscall.rs` — `sys_open`, `sys_fstat`,
  `read_file_fully`, routed read/write, exec-from-disk.
- `crates/kernel/src/embed.rs` — just `INITCODE` now.
- `crates/kernel/src/main.rs` — `bringup_then_init` async task
  initialises fs *before* spawning init.
- `crates/kernel/src/executor.rs` — don't re-install completed
  tasks.
- `crates/kernel/user/ulib.S` — every xv6 syscall wrapper.
- `crates/kernel/user/ls.c` — new.
- `crates/kernel/build.rs` + `Makefile` — wire `ls` into the user
  binary set + `fs.img`.

Total: ~310 LoC kernel + 120 LoC user. No new unsafe blocks.

## Verified at

`make fs.img && qemu-system-riscv64 ... -drive file=fs.img,...`
with stdin scripted as `ls /\necho hello from disk\n`.
