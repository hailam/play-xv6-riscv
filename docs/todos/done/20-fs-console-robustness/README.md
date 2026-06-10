# 20: FS + console robustness

**Status:** DONE (2026-06-10) — full suite (now 74 tests incl. two new
regressions) **ALL TESTS PASSED** on riscv64 `-smp 1`/`-smp 3` and
aarch64 `-smp 1`/`-smp 4`; interactive `^D`/echo verified live
(`cat` + typed line + `^D` → EOF exit, shell stays healthy).

## What landed

### FS
1. **Sparse files are real files now.** `bmap` returns 0 for a hole at
   ANY level of the block tree (the asserts it replaces were
   user-reachable panics via `ftruncate`-grow + read; the direct range
   was worse — it silently `bread(0)`'d the BOOT BLOCK as file
   content). `readi` renders holes as zeros (POSIX). `ftruncate`/
   `truncate` reject lengths past `MAXFILE*BSIZE` (EFBIG-style)
   instead of letting reads run off the addressable tree.
2. **`iget` waits instead of panicking** when all 50 cache slots are
   held (`NPROC × NOFILE` distinct files exceeds 50): `iget_wait`
   yield-retries like `bread`'s cache-full path; every runtime caller
   (namei/dirlookup/ialloc/getcwd) converted; the sync `iget` remains
   for boot.
3. **Lazy pages demand-map in the syscall path.** `translate_user_perm`
   now synchronously maps a zero page for lazy-heap and anonymous-VMA
   addresses (with a raced-fault re-check), so `read(fd, sbrklazy_buf,
   n)` / `write(fd, lazy_buf, n)` behave like xv6's copyin/copyout.
   File-backed VMAs stay fault-path-only (they need an async inode
   read).

### Console
4. **Line discipline** (xv6 `consoleintr` port, in `console_in.rs`):
   an EDIT buffer with echo, backspace/DEL rubout (`\b \b`), `^U`
   kill-line; lines release to readers on `\n`; `^D` flushes the
   partial line plus an in-band `0x04` EOF mark that `console_read`
   consumes — returning the partial count first and 0 (EOF) next,
   exactly xv6's semantics. `cat` with no args is now exitable;
   TCGETS's advertised `ICANON|ECHO` is finally true.
5. **`console_write` plays fair**: 256-byte chunks copied in, emitted
   under the console lock (user output no longer interleaves
   byte-wise with kernel `println!`), yielding between chunks with a
   killed-check — a huge write can't monopolize a cooperative hart.
6. **`poll` parks on wakers** (console reader list, pipe read/write
   lists, plus a deadline timer) via a two-phase `PollWait` future —
   readiness latency went from a 10 ms duty-cycle to interrupt-grade,
   and an idle infinite poll costs nothing.

## New regression tests (quicktests)
* `sparsefile` — holes in direct + indirect ranges read zero;
  write-into-hole readback; EOF at size; `ftruncate` past MAXFILE
  fails.
* `lazyio` — `read` into and `write` from never-faulted lazy pages.

## Residual notes
* Raw/termios mode toggling (turning ICANON/ECHO *off* via TCSETS) is
  not implemented — cooked mode only.
* `FIONREAD`/POLLIN count cooked bytes only (the line being edited is
  invisible until newline — correct for canonical mode).
* File-backed-VMA buffers passed to syscalls still fail translate
  (fault-path-only); revisit if a real program needs it.

## Key files
`fs/inode.rs` (bmap/readi/iget_wait), `fs/dir.rs`, `fs/path.rs`,
`proc.rs` (demand_map_zero_page), `console_in.rs` (rewrite),
`console.rs` (write_bytes), `syscall.rs` (console_read/write, poll
PollWait, ftruncate clamp), `user/usertests.c`.
