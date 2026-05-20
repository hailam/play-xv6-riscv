# 28: fs polish — cwd / chdir / link / sh redirects  [DONE]

Closes the deferred-fs items from [[22-fs-writes]] and
[[23-sys-kill-cancellation]].

## What landed

End-to-end shell session (after `mkdir /work` from a previous run
left /work in place):

```
$ cd /work
$ /wr greet hello from cwd
$ /cat greet
hello from cwd
$ /ln greet alias
$ /cat alias
hello from cwd
$ /ls /work
DIR   inum=17  size=64  .
DIR   inum=1   size=304  ..
FILE  inum=19  size=15  greet
FILE  inum=19  size=15  alias        <- same inum = true hard link
$ cd /
$ /echo redirect-result > /tmp-out
$ /cat /tmp-out
redirect-result
```

## Files

- `crates/kernel/src/fs/path.rs` — `namei_from(start, path)` and
  `nameiparent_from(start, path)` accept the resolution start
  inode. Absolute paths still snap to root regardless.
- `crates/kernel/src/proc.rs` — `Proc::cwd: SpinLock<Option<Arc<Inode>>>`;
  `fork_from` clones it.
- `crates/kernel/src/main.rs::bringup_then_init` seeds the init
  proc's cwd to root once the inode cache is up.
- `crates/kernel/src/syscall.rs`:
  - `resolve_path(proc, path)` + `nameiparent_via_cwd(proc, path)`
    helpers thread cwd through `sys_open`, `sys_mkdir`, `sys_mknod`,
    `sys_unlink`, `sys_exec`.
  - `sys_chdir(path)` resolves, asserts T_DIR, replaces cwd.
  - `sys_link(old, new)` bumps `nlink`, then `dirlink` in the
    parent; rolls back on failure. Refuses to link directories
    (matches xv6).
- `crates/kernel/user/sh.c`:
  - `>` and `>>` redirect (one operand each, single command).
  - `cd path` builtin (runs in the shell process — chdir-in-child
    would be lost).
- `crates/kernel/user/ln.c` — new (~20 LoC).
- `crates/kernel/build.rs` + `Makefile` — `ln` added to programs.

Total: ~180 LoC kernel + ~80 LoC user. No new unsafe.

## Incidental: SMP picker fix

`executor::pick_home_cpu` used `Arch::ncpus()` which always returned
`MAX_CPUS = 8`. With `-smp 1`, init was spawned to hart 1 (because
bringup's transient disk-IRQ wakes had left hart 0's ready queue
"loaded" relative to hart 1's empty one), and hart 1 didn't exist
so init never ran.

Fix: track an `ACTIVE_CPUS: AtomicU64` bitmask in `crate::cpu`,
set in `init_this_hart`. `pick_home_cpu` only considers harts
whose bit is set. `cpu::active_cpu_mask()` is the public read.

Before this fix the previous SMP test happened to pass only
because `-smp 3` left enough live harts that any pick was valid.

## Design notes

- **No PATH**: like xv6, the shell calls `exec(argv[0], ...)`
  directly, and `argv[0]` must be a real path (`/echo`, `/ls`,
  `./foo`). Relative names are looked up in cwd, not against any
  search list. Documented in the new `sh.c` comment.
- **`cd` is a shell builtin** — running it as a child would
  chdir-then-exit, leaving the shell's cwd unchanged.
- **`>` semantics**: `O_WRONLY | O_CREATE | O_TRUNC`. `>>` skips
  TRUNC. The redirect-handling runs in the forked child only, so
  the shell's own fds aren't touched.
- **`ln` is a real hard link** — both names point to the same
  inum. `nlink` is incremented before the new dirent is added so
  a crash between the two leaves the file recoverable.

## Verified at

`make fs.img && qemu-system-riscv64 ... -smp 1 ...` then the
session quoted above.
