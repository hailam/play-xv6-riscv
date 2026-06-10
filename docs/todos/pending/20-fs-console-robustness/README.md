# 20: FS + console robustness

**Status:** Pending
**Estimated:** ~2 sessions
**Depends on:** — (independent of 18/19)

## Why (audit evidence, 2026-06-10)

User-reachable panics and conformance gaps that survived the 17 round
because no current usertest exercises them:

### FS
1. **Sparse-file reads hit asserts / read the boot block.**
   `ftruncate(fd, big)` just bumps `size` ("the gap becomes a sparse
   hole" — inode.rs `itrunc_to`); reading the hole then either
   `bread(0)`s the BOOT BLOCK as file content (direct range, addr 0)
   or PANICS on `bmap`'s `assert!(ind_blkno != 0)` /
   `assert!(bn < MAXFILE)` (indirect ranges, ftruncate to 4 GiB).
   Fix: `bmap` returns `Option`; holes read as zeros (POSIX); clamp
   ftruncate length to `MAXFILE*BSIZE`.
2. **`iget` panics when the 50-entry inode cache fills** (xv6 parity,
   but reachable: NPROC=64 procs × distinct cwds/open files). Wait
   (WakerList) instead, or grow + wait.
3. **Lazy pages aren't demand-mapped in the syscall path** —
   `translate_user(_write)` returns None for a never-faulted lazy
   page, so `read(fd, fresh_sbrklazy_buf, n)` fails where xv6's
   copyout faults the page in. Demand-map in `translate_user_perm`
   when `va < size` (reuse `lazy_map_page_async`'s heap arm).

### Console
4. **No line discipline**: no `^D` → EOF (so `cat` with no args can
   NEVER terminate), no `^H`/DEL erase, no echo — while `sys_ioctl`
   TCGETS *advertises* `ICANON|ECHO`. Port xv6's `consoleintr`.
5. **`console_write` monopolizes the hart**: synchronous unbounded
   loop, no lock vs `println!`, no yield — a large write stalls every
   other task on a cooperative executor. Chunk + yield, take the
   console lock per chunk.
6. **poll() duty-cycles at 10 ms** — replace the cooperative re-poll
   loop with registration on the (now multi-waiter) file/pipe/console
   WakerLists; keep the timeout via the timer wheel.

## Verification

- New tests: ftruncate-grow + read-back zeros; open/`^D` interactive
  check (or scripted pty); poll latency < 1 ms for pipe readiness.
- Full suite stays green both arches.
