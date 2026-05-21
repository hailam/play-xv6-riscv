//! Async syscall dispatch.

use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::future::Future;
use core::pin::Pin;
use core::sync::atomic::{AtomicU32, Ordering};
use core::task::{Context, Poll};

use hal::{FrameAllocator, Hal, PageTableOps, PtePerm, TrapFrameAccess};

use crate::arch::Arch;
use crate::cpu;
use crate::executor;
use crate::file::{File, PipeInner};
use crate::fs;
use crate::kalloc::KFRAMES;
use crate::proc::{Proc, ProcState};
use crate::user_vm::STACK_VA_BASE;

type TrapFrame = <Arch as Hal>::TrapFrame;
use crate::uapi::{
    Stat, FD_CLOEXEC, F_DUPFD, F_DUPFD_CLOEXEC, F_GETFD, F_GETFL, F_SETFD, F_SETFL,
    O_APPEND, O_CLOEXEC, O_CREATE, O_NONBLOCK, O_RDONLY, O_RDWR, O_TRUNC, O_WRONLY,
    SEEK_CUR, SEEK_END, SEEK_SET, SYS_CHDIR, SYS_CHMOD, SYS_CHOWN, SYS_CLOSE, SYS_DUP,
    SYS_EXEC, SYS_EXIT, SYS_FCNTL, SYS_FORK, SYS_FSTAT, SYS_FTRUNCATE, SYS_GETEGID,
    SYS_GETEUID,
    SYS_GETGID, SYS_GETPID, SYS_GETUID, SYS_KILL, SYS_LINK, SYS_LSEEK, SYS_MKDIR,
    SYS_MKNOD, SYS_OPEN, SYS_PIPE, SYS_PREAD, SYS_PWRITE, SYS_READ, SYS_SBRK, SYS_SETGID,
    SYS_SETUID, SYS_SIGACTION, SYS_SIGPROCMASK, SYS_SIGRETURN, SYS_SLEEP, SYS_STAT,
    SYS_TRUNCATE, SYS_UMASK, SYS_UNLINK, SYS_UPTIME, SYS_WAIT, SYS_WRITE,
    SYS_DUP2, SYS_GETCWD, SYS_RENAME,
    SYS_WAITPID, SYS_PAUSE, SYS_ALARM, WNOHANG,
    SYS_CLOCK_GETTIME, SYS_GETDENTS, CLOCK_MONOTONIC, CLOCK_REALTIME,
    Timespec, UserDirent,
};

use crate::arch::{PGSIZE, TIMER_INTERVAL};

pub async fn dispatch(proc: &Arc<Proc>, nr: usize) -> i64 {
    match nr {
        SYS_FORK => sys_fork(proc).await,
        SYS_EXIT => {
            let code = proc.trapframe().arg(0) as i32;
            sys_exit_inner(proc, code).await
        }
        SYS_WAIT => {
            let status_va = proc.trapframe().arg(0) as usize;
            sys_wait(proc, status_va).await
        }
        SYS_WRITE => {
            let tf = proc.trapframe();
            let fd = tf.arg(0) as i32;
            let buf_va = tf.arg(1) as usize;
            let len = tf.arg(2) as usize;
            sys_write(proc, fd, buf_va, len).await
        }
        SYS_READ => {
            let tf = proc.trapframe();
            let fd = tf.arg(0) as i32;
            let buf_va = tf.arg(1) as usize;
            let len = tf.arg(2) as usize;
            sys_read(proc, fd, buf_va, len).await
        }
        SYS_SLEEP => {
            let ticks = proc.trapframe().arg(0);
            sys_sleep(proc, ticks).await
        }
        SYS_EXEC => {
            let tf = proc.trapframe();
            let path_va = tf.arg(0) as usize;
            let argv_va = tf.arg(1) as usize;
            sys_exec(proc, path_va, argv_va).await
        }
        SYS_PIPE => {
            let pipefd_va = proc.trapframe().arg(0) as usize;
            sys_pipe(proc, pipefd_va).await
        }
        SYS_CLOSE => {
            let fd = proc.trapframe().arg(0) as i32;
            sys_close(proc, fd).await
        }
        SYS_DUP => {
            let fd = proc.trapframe().arg(0) as i32;
            sys_dup(proc, fd).await
        }
        SYS_OPEN => {
            let tf = proc.trapframe();
            let path_va = tf.arg(0) as usize;
            let flags = tf.arg(1) as u32;
            sys_open(proc, path_va, flags).await
        }
        SYS_FSTAT => {
            let tf = proc.trapframe();
            let fd = tf.arg(0) as i32;
            let stat_va = tf.arg(1) as usize;
            sys_fstat(proc, fd, stat_va).await
        }
        SYS_GETPID => proc.pid as i64,
        SYS_UPTIME => (Arch::now_ticks() / TIMER_INTERVAL) as i64,
        SYS_KILL => {
            let tf = proc.trapframe();
            let pid = tf.arg(0) as i32;
            let sig = tf.arg(1) as i32;
            sys_kill(proc, pid, sig).await
        }
        SYS_SBRK => {
            let tf = proc.trapframe();
            // sbrk takes `int` in C — must sign-extend from the lower
            // 32 bits because AArch64's AAPCS64 leaves the upper half
            // of an argument register unspecified for 32-bit args.
            // (On RISC-V the calling convention sign-extends already,
            // so this is a no-op there.)
            let n = tf.arg(0) as i32 as i64;
            let lazy = tf.arg(1) as i32 as i64;
            sys_sbrk(proc, n, lazy).await
        }
        SYS_MKDIR => {
            let path_va = proc.trapframe().arg(0) as usize;
            sys_mkdir(proc, path_va).await
        }
        SYS_MKNOD => {
            let tf = proc.trapframe();
            let path_va = tf.arg(0) as usize;
            let major = tf.arg(1) as u16;
            let minor = tf.arg(2) as u16;
            sys_mknod(proc, path_va, major, minor).await
        }
        SYS_UNLINK => {
            let path_va = proc.trapframe().arg(0) as usize;
            sys_unlink(proc, path_va).await
        }
        SYS_CHDIR => {
            let path_va = proc.trapframe().arg(0) as usize;
            sys_chdir(proc, path_va).await
        }
        SYS_LINK => {
            let tf = proc.trapframe();
            let old_va = tf.arg(0) as usize;
            let new_va = tf.arg(1) as usize;
            sys_link(proc, old_va, new_va).await
        }
        SYS_LSEEK => {
            let tf = proc.trapframe();
            let fd = tf.arg(0) as i32;
            // off_t is 64-bit. AAPCS64 / RISC-V both pass it whole
            // in a single 64-bit register, so no sign-extension dance
            // here — unlike the 32-bit `int n` in sbrk.
            let offset = tf.arg(1) as i64;
            let whence = tf.arg(2) as i32;
            sys_lseek(proc, fd, offset, whence).await
        }
        SYS_PREAD => {
            let tf = proc.trapframe();
            let fd = tf.arg(0) as i32;
            let buf_va = tf.arg(1) as usize;
            let len = tf.arg(2) as usize;
            let offset = tf.arg(3) as i64;
            sys_pread(proc, fd, buf_va, len, offset).await
        }
        SYS_PWRITE => {
            let tf = proc.trapframe();
            let fd = tf.arg(0) as i32;
            let buf_va = tf.arg(1) as usize;
            let len = tf.arg(2) as usize;
            let offset = tf.arg(3) as i64;
            sys_pwrite(proc, fd, buf_va, len, offset).await
        }
        SYS_STAT => {
            let tf = proc.trapframe();
            let path_va = tf.arg(0) as usize;
            let stat_va = tf.arg(1) as usize;
            sys_stat(proc, path_va, stat_va).await
        }
        SYS_CHMOD => {
            let tf = proc.trapframe();
            let path_va = tf.arg(0) as usize;
            // mode_t is u32 in POSIX. Truncate the 12 low bits we
            // store (rwxrwxrwx + sticky/setuid/setgid).
            let mode = (tf.arg(1) as u32) & 0o7777;
            sys_chmod(proc, path_va, mode as u16).await
        }
        SYS_CHOWN => {
            let tf = proc.trapframe();
            let path_va = tf.arg(0) as usize;
            let uid = tf.arg(1) as u16;
            let gid = tf.arg(2) as u16;
            sys_chown(proc, path_va, uid, gid).await
        }
        SYS_GETUID | SYS_GETEUID => proc.uid.load(Ordering::Acquire) as i64,
        SYS_GETGID | SYS_GETEGID => proc.gid.load(Ordering::Acquire) as i64,
        SYS_SETUID => {
            let new_uid = proc.trapframe().arg(0) as u32;
            if proc.uid.load(Ordering::Acquire) != 0 {
                // Non-root: POSIX would allow set to real/effective/
                // saved. Without saved IDs the conservative choice is
                // to refuse any change. Root can freely setuid below.
                -1
            } else {
                proc.uid.store(new_uid, Ordering::Release);
                0
            }
        }
        SYS_SETGID => {
            let new_gid = proc.trapframe().arg(0) as u32;
            if proc.uid.load(Ordering::Acquire) != 0 {
                -1
            } else {
                proc.gid.store(new_gid, Ordering::Release);
                0
            }
        }
        SYS_UMASK => {
            // POSIX: returns the previous mask, sets the new one.
            // mode_t is u32; we store as u32. Mask to 0o777 (xv6
            // doesn't have setuid/setgid/sticky bits to mask).
            let mask = (proc.trapframe().arg(0) as u32) & 0o777;
            proc.umask.swap(mask, Ordering::AcqRel) as i64
        }
        SYS_FCNTL => {
            let tf = proc.trapframe();
            let fd = tf.arg(0) as i32;
            let cmd = tf.arg(1) as i32;
            let arg = tf.arg(2) as i64;
            sys_fcntl(proc, fd, cmd, arg)
        }
        SYS_FTRUNCATE => {
            let tf = proc.trapframe();
            let fd = tf.arg(0) as i32;
            let length = tf.arg(1) as i64;
            sys_ftruncate(proc, fd, length).await
        }
        SYS_TRUNCATE => {
            let tf = proc.trapframe();
            let path_va = tf.arg(0) as usize;
            let length = tf.arg(1) as i64;
            sys_truncate(proc, path_va, length).await
        }
        SYS_SIGACTION => {
            let tf = proc.trapframe();
            let signum = tf.arg(0) as i32;
            let handler = tf.arg(1) as usize;
            let restorer = tf.arg(2) as usize;
            let mask = tf.arg(3) as u32;
            sys_sigaction(proc, signum, handler, restorer, mask)
        }
        SYS_SIGRETURN => sys_sigreturn(proc),
        SYS_SIGPROCMASK => {
            let tf = proc.trapframe();
            let how = tf.arg(0) as i32;
            let set = tf.arg(1) as u32;
            let oldset_va = tf.arg(2) as usize;
            sys_sigprocmask(proc, how, set, oldset_va)
        }
        SYS_DUP2 => {
            let tf = proc.trapframe();
            let oldfd = tf.arg(0) as i32;
            let newfd = tf.arg(1) as i32;
            sys_dup2(proc, oldfd, newfd)
        }
        SYS_GETCWD => {
            let tf = proc.trapframe();
            let buf_va = tf.arg(0) as usize;
            let len = tf.arg(1) as usize;
            sys_getcwd(proc, buf_va, len).await
        }
        SYS_RENAME => {
            let tf = proc.trapframe();
            let old_va = tf.arg(0) as usize;
            let new_va = tf.arg(1) as usize;
            sys_rename(proc, old_va, new_va).await
        }
        SYS_WAITPID => {
            let tf = proc.trapframe();
            let pid = tf.arg(0) as i32;
            let status_va = tf.arg(1) as usize;
            let options = tf.arg(2) as i32;
            sys_waitpid(proc, pid, status_va, options).await
        }
        SYS_PAUSE => sys_pause(proc).await,
        SYS_ALARM => {
            let secs = proc.trapframe().arg(0) as u32;
            sys_alarm(proc, secs)
        }
        SYS_CLOCK_GETTIME => {
            let tf = proc.trapframe();
            let clk = tf.arg(0) as i32;
            let ts_va = tf.arg(1) as usize;
            sys_clock_gettime(proc, clk, ts_va)
        }
        SYS_GETDENTS => {
            let tf = proc.trapframe();
            let fd = tf.arg(0) as i32;
            let buf_va = tf.arg(1) as usize;
            let len = tf.arg(2) as usize;
            sys_getdents(proc, fd, buf_va, len).await
        }
        _ => {
            crate::println!("syscall: unknown nr {}", nr);
            -1
        }
    }
}

/// `char* sys_sbrk(int n, int lazy)` — xv6-compatible semantics:
///   * Always returns the OLD break (the byte at the start of the
///     freshly-allocated region).
///   * `lazy == 0` (SBRK_EAGER) **or** `n < 0` → grow/shrink eagerly,
///     allocating real frames now (or just updating size on shrink).
///   * `lazy != 0` and `n >= 0` → just bump `proc.size`; the next
///     access fault inside the grown range triggers `lazy_map_page`
///     in `usertrap` which maps a zero frame on demand.
///
/// Returns `-1` on error (OOM, would-exceed-stack-guard, etc.).
async fn sys_sbrk(proc: &Arc<Proc>, n: i64, lazy: i64) -> i64 {
    let old = proc.size.load(Ordering::Acquire);
    if n == 0 {
        return old as i64;
    }
    if n < 0 {
        let shrink = (-n) as usize;
        if shrink > old {
            return -1;
        }
        let new_size = old - shrink;
        // Unmap every fully-shrunk page and return its frame to the
        // pool. Without this, a subsequent `sbrk(+n)` for the same
        // VA range would hit `VmError::Remap` and fail. xv6 does the
        // same via `uvmdealloc` -> `uvmunmap`.
        let start = (new_size + PGSIZE - 1) & !(PGSIZE - 1);
        let end = (old + PGSIZE - 1) & !(PGSIZE - 1);
        {
            let mut pt = proc.pagetable.lock();
            let mut va = start;
            while va < end {
                if let Some(pa) = pt.unmap_page(va) {
                    unsafe { KFRAMES.free(pa) };
                }
                va += PGSIZE;
            }
        }
        proc.size.store(new_size, Ordering::Release);
        return old as i64;
    }
    let new = match old.checked_add(n as usize) {
        Some(x) => x,
        None => return -1,
    };
    let heap_top = STACK_VA_BASE.saturating_sub(PGSIZE);
    if new > heap_top {
        return -1;
    }
    if lazy != 0 {
        // Lazy path: just bump size. Pages get mapped on first
        // access via `usertrap`'s lazy-fault handler.
        proc.size.store(new, Ordering::Release);
        return old as i64;
    }

    // Eager path.
    let start = (old + PGSIZE - 1) & !(PGSIZE - 1);
    let end = (new + PGSIZE - 1) & !(PGSIZE - 1);
    {
        let mut pt = proc.pagetable.lock();
        let mut va = start;
        while va < end {
            let Some(pa) = KFRAMES.alloc_zeroed() else {
                // Roll back any pages we've successfully mapped so far
                // in this call, so a failed grow doesn't leak.
                let mut rb = start;
                while rb < va {
                    if let Some(p) = pt.unmap_page(rb) {
                        unsafe { KFRAMES.free(p) };
                    }
                    rb += PGSIZE;
                }
                return -1;
            };
            if pt.map(va, pa, PGSIZE, PtePerm::URW, &KFRAMES).is_err() {
                unsafe { KFRAMES.free(pa) };
                let mut rb = start;
                while rb < va {
                    if let Some(p) = pt.unmap_page(rb) {
                        unsafe { KFRAMES.free(p) };
                    }
                    rb += PGSIZE;
                }
                return -1;
            }
            va += PGSIZE;
        }
    }
    proc.size.store(new, Ordering::Release);
    old as i64
}

/// Called from `usertrap` on a load/store page fault. Returns true
/// if the fault was a "lazy region" miss and we mapped a fresh
/// zero page (caller should resume the trapping instr by not
/// advancing epc); false if the fault is genuinely illegal.
pub fn lazy_map_page(proc: &Arc<Proc>, fault_va: usize) -> bool {
    let size = proc.size.load(Ordering::Acquire);
    if fault_va >= size {
        return false;
    }
    let page = fault_va & !(PGSIZE - 1);
    // Already mapped? Could happen if two harts faulted at once;
    // bail rather than double-map.
    {
        let pt = proc.pagetable.lock();
        if pt.translate(page).is_some() {
            return true;
        }
    }
    let Some(pa) = KFRAMES.alloc_zeroed() else {
        return false;
    };
    let mut pt = proc.pagetable.lock();
    if pt.map(page, pa, PGSIZE, PtePerm::URW, &KFRAMES).is_err() {
        // Free the frame if mapping fails (only happens on Remap,
        // which our check above already filtered).
        unsafe { KFRAMES.free(pa) };
        return false;
    }
    true
}

/// POSIX `kill(pid, sig)`.
///
/// * `sig == 0` is "check that `pid` exists and we may signal it"; no
///   signal is delivered. Returns 0 on existence, -1 otherwise.
/// * For signals whose default disposition is to terminate
///   ([`sig_default_kills`]), we set `proc.killed` and wake all
///   blocking futures so the target unwinds into `sys_exit(-1)`. No
///   user-installed handlers yet — sigaction lands in a later slice.
/// * Ignorable signals (CHLD, CONT, STOP) are no-ops in this slice.
/// * Unknown `sig` returns -1.
async fn sys_kill(proc: &Arc<Proc>, pid: i32, sig: i32) -> i64 {
    if pid <= 0 {
        return -1;
    }
    if sig < 0 || sig > 31 {
        return -1;
    }
    // The executor `take()`s the currently-running task from the
    // per-CPU table while polling, so `find_proc_by_pid` can't see
    // the caller itself. Short-circuit on self-pid using the
    // already-held caller reference.
    let target = if (pid as usize) == proc.pid {
        Arc::clone(proc)
    } else {
        match executor::find_proc_by_pid(pid as usize) {
            Some(p) => p,
            None => return -1,
        }
    };
    if sig == 0 {
        // Existence check — caller can signal `pid` (we don't yet
        // enforce uid checks on kill; that'll come with a fuller
        // capability pass).
        return 0;
    }
    use crate::uapi::{SIGKILL, SIGSTOP, SIG_DFL, SIG_IGN};
    // SIGKILL is uncatchable — always proc.killed. Skip the handler
    // table lookup.
    if sig == SIGKILL {
        target.killed.store(true, Ordering::Release);
        target.wait_waker.wake();
        crate::console_in::wake();
        executor::wake(target.task_id.load(Ordering::Relaxed));
        return 0;
    }
    if sig == SIGSTOP {
        // No job control yet — accept-but-ignore (real SIGSTOP would
        // suspend the proc until SIGCONT).
        return 0;
    }
    let action = target.sig_actions.lock()[sig as usize];
    match action.handler {
        SIG_DFL => {
            if crate::uapi::sig_default_kills(sig) {
                target.killed.store(true, Ordering::Release);
                target.wait_waker.wake();
                crate::console_in::wake();
                executor::wake(target.task_id.load(Ordering::Relaxed));
            }
            // else: default-ignore signals (CHLD/CONT) are no-ops.
            0
        }
        SIG_IGN => 0,
        _ => {
            // User installed a handler — queue the signal as pending
            // and wake any blocking await so delivery happens at the
            // next return-to-user.
            let bit = 1u32 << sig;
            target.sig_pending.fetch_or(bit, Ordering::AcqRel);
            target.wait_waker.wake();
            crate::console_in::wake();
            executor::wake(target.task_id.load(Ordering::Relaxed));
            0
        }
    }
}

/// Helper for futures: is the currently-polling proc killed?
fn current_proc_killed() -> bool {
    cpu::current_proc()
        .map(|p| p.killed.load(Ordering::Acquire))
        .unwrap_or(false)
}

async fn sys_fork(parent: &Arc<Proc>) -> i64 {
    let Some(child) = Proc::fork_from(parent) else {
        return -1;
    };
    let child_pid = child.pid as i64;
    let child_arc = Arc::new(child);
    *child_arc.parent.lock() = Some(Arc::downgrade(parent));
    parent.children.lock().push(child_arc.clone());
    // Pipe reader/writer counts are bumped inside `Proc::fork_from`'s
    // file table clone via `File::Clone`.
    crate::proc::spawn_proc_main(child_arc);
    child_pid
}

/// Set as `pub(crate)` so `proc_main` can route killed procs into a
/// clean exit.
pub(crate) async fn sys_exit(proc: &Arc<Proc>, code: i32) -> i64 {
    sys_exit_inner(proc, code).await
}

async fn sys_exit_inner(proc: &Arc<Proc>, code: i32) -> i64 {
    // Drop all fds so pipe end counts decrement now (rather than
    // waiting for the Proc itself to be reclaimed).
    proc.files.lock().clear();

    // Tear down the user pagetable now — this is what reclaims the
    // user data pages, heap, and L1/L0 table pages. After this the
    // proc is a Zombie; its parent will reap the (already-small)
    // remaining `Proc` via `wait`.
    let fresh = <Arch as Hal>::PageTable::new(&KFRAMES).expect("exit: dummy pt");
    let old_pt = core::mem::replace(&mut *proc.pagetable.lock(), fresh);
    drop(old_pt); // <- triggers `Drop for PageTable`

    proc.exit_code.store(code, Ordering::Relaxed);
    proc.state.store(ProcState::Zombie as i32, Ordering::Release);
    let parent_weak = proc.parent.lock().clone();
    if let Some(p) = parent_weak.and_then(|w| w.upgrade()) {
        p.wait_waker.wake();
    }
    crate::println!(
        "pid {} exit({code}) on hart {} kalloc.free={}",
        proc.pid,
        Arch::hartid(),
        KFRAMES.free_count(),
    );
    0
}

/// xv6 `int wait(int *status)` semantics: returns the reaped
/// child's pid (or -1 if there are no children / killed). If
/// `status_va != 0`, the child's exit code is written through that
/// user pointer.
async fn sys_wait(proc: &Arc<Proc>, status_va: usize) -> i64 {
    sys_waitpid(proc, -1, status_va, 0).await
}

/// POSIX `waitpid(pid, &status, options)`:
///   * `pid == -1` → any child (matches `wait()`)
///   * `pid > 0`   → that specific child
///   * `pid == 0` / `pid < -1` → no process-group semantics yet
///     (treat as "any" for safety)
///   * `options & WNOHANG` → don't block; return 0 if no eligible
///     exited child
async fn sys_waitpid(
    proc: &Arc<Proc>,
    pid: i32,
    status_va: usize,
    options: i32,
) -> i64 {
    let filter = if pid > 0 { Some(pid as usize) } else { None };
    let nonblock = (options & WNOHANG) != 0;
    let (reaped_pid, code) =
        match (WaitFor { proc, filter, nonblock }).await {
            WaitOutcome::Reaped(p, c) => (p, c),
            WaitOutcome::NoChildren => return -1,
            WaitOutcome::WouldBlock => return 0,
            WaitOutcome::Killed => return -1,
        };
    if status_va != 0 {
        let Some(kva) = proc.translate_user_write(status_va) else {
            return -1;
        };
        unsafe { *(kva as *mut i32) = code };
    }
    reaped_pid as i64
}

enum WaitOutcome {
    Reaped(usize, i32),
    NoChildren,
    WouldBlock,
    Killed,
}

struct WaitFor<'a> {
    proc: &'a Arc<Proc>,
    filter: Option<usize>,
    nonblock: bool,
}

impl Future for WaitFor<'_> {
    type Output = WaitOutcome;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<WaitOutcome> {
        if self.proc.killed.load(Ordering::Acquire) {
            return Poll::Ready(WaitOutcome::Killed);
        }
        self.proc.wait_waker.register(cx.waker());
        let mut children = self.proc.children.lock();
        // No children at all — there's nothing to wait for.
        if children.is_empty() {
            return Poll::Ready(WaitOutcome::NoChildren);
        }
        // Look for an exited child that matches the filter (if any).
        let mut idx = None;
        let mut any_match = false;
        for (i, c) in children.iter().enumerate() {
            let pid_match = self.filter.map_or(true, |p| c.pid == p);
            if !pid_match {
                continue;
            }
            any_match = true;
            if c.is_zombie() {
                idx = Some(i);
                break;
            }
        }
        if let Some(i) = idx {
            let dead = children.remove(i);
            let pid = dead.pid;
            let code = dead.exit_code.load(Ordering::Relaxed);
            return Poll::Ready(WaitOutcome::Reaped(pid, code));
        }
        // Filter named a specific pid but we have no child with that
        // pid at all — POSIX semantics: ECHILD-equivalent.
        if self.filter.is_some() && !any_match {
            return Poll::Ready(WaitOutcome::NoChildren);
        }
        if self.nonblock {
            return Poll::Ready(WaitOutcome::WouldBlock);
        }
        Poll::Pending
    }
}

async fn sys_sleep(_proc: &Arc<Proc>, ticks: u64) -> i64 {
    let now = Arch::now_ticks();
    let deadline = now + ticks * TIMER_INTERVAL;
    Sleep { deadline }.await;
    0
}

/// POSIX `pause()` — block until a signal handler runs (or the proc
/// is killed). Always returns -1; POSIX requires errno=EINTR which
/// we conflate with the return code.
async fn sys_pause(proc: &Arc<Proc>) -> i64 {
    Pause { proc }.await;
    -1
}

struct Pause<'a> {
    proc: &'a Arc<Proc>,
}

impl Future for Pause<'_> {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        if self.proc.killed.load(Ordering::Acquire) {
            return Poll::Ready(());
        }
        // Wake on either pending-signal-after-mask or kill. sys_kill
        // pings wait_waker for both, so we register there.
        self.proc.wait_waker.register(cx.waker());
        let pending = self.proc.sig_pending.load(Ordering::Acquire);
        let blocked = self.proc.sig_blocked.load(Ordering::Acquire);
        if pending & !blocked != 0 {
            // A deliverable signal is queued — let return-to-user
            // dispatch it.
            return Poll::Ready(());
        }
        Poll::Pending
    }
}

/// POSIX `clock_gettime(clk, &ts)` — fills `ts` with the current
/// time. Only `CLOCK_MONOTONIC` is supported (we have no RTC); we
/// alias `CLOCK_REALTIME` to MONOTONIC so portable code at least
/// gets monotonic semantics for both.
///
/// Time source: `Arch::now_ticks()`. Per the comment in `stattime`,
/// the tick rate differs per arch (riscv ~10 MHz, aarch64 generic
/// timer typ. 62.5 MHz). We compute (sec, nsec) using TIMER_INTERVAL
/// as the conversion divisor that matches our timer-IRQ rate — the
/// resulting values are monotonic-correct even if the absolute
/// "seconds" doesn't track wall time precisely.
fn sys_clock_gettime(proc: &Arc<Proc>, clk: i32, ts_va: usize) -> i64 {
    if clk != CLOCK_MONOTONIC && clk != CLOCK_REALTIME {
        return -1;
    }
    let ticks = Arch::now_ticks();
    // ticks per second is roughly TIMER_INTERVAL * 10 (we fire the
    // timer ~10× per second by convention). Compute seconds + ns
    // using the timer-IRQ tick as the unit.
    let ts_per_sec = TIMER_INTERVAL * 10;
    let tv_sec = (ticks / ts_per_sec) as i64;
    let rem = ticks % ts_per_sec;
    let tv_nsec = ((rem as u128 * 1_000_000_000) / ts_per_sec as u128) as i64;
    let ts = Timespec { tv_sec, tv_nsec };
    let bytes = unsafe {
        core::slice::from_raw_parts(
            &ts as *const _ as *const u8,
            core::mem::size_of::<Timespec>(),
        )
    };
    for (i, b) in bytes.iter().enumerate() {
        let Some(kva) = proc.translate_user_write(ts_va + i) else {
            return -1;
        };
        unsafe { *(kva as *mut u8) = *b };
    }
    0
}

/// POSIX-ish `getdents(fd, buf, len)` — reads directory entries into
/// a user buffer as packed `UserDirent` records. Returns the number
/// of bytes written, 0 at EOF, -1 on error. `fd` must be a dir
/// opened for read.
async fn sys_getdents(
    proc: &Arc<Proc>,
    fd: i32,
    buf_va: usize,
    len: usize,
) -> i64 {
    use xv6_fs_layout::{Dirent, DIRSIZ, T_DIR};
    let entry_size = core::mem::size_of::<Dirent>();
    let out_size = core::mem::size_of::<UserDirent>();
    if len < out_size {
        return -1;
    }
    let Some(file) = proc.get_file(fd) else {
        return -1;
    };
    let File::Inode {
        ip, off, readable, ..
    } = &*file
    else {
        return -1;
    };
    if !*readable {
        return -1;
    }
    let (typ, size) = {
        let li = fs::inode::ilock(ip).await;
        (li.state().typ, li.state().size)
    };
    if typ != T_DIR {
        return -1;
    }
    let mut copied: usize = 0;
    let mut cur_off = off.load(Ordering::Acquire);
    while copied + out_size <= len && cur_off < size {
        let mut entry = Dirent::default();
        let n = {
            let li = fs::inode::ilock(ip).await;
            let bytes = unsafe {
                core::slice::from_raw_parts_mut(
                    &mut entry as *mut _ as *mut u8,
                    entry_size,
                )
            };
            fs::inode::readi(&li, bytes, cur_off).await
        };
        if n != entry_size {
            break;
        }
        cur_off += entry_size as u32;
        if entry.inum == 0 {
            continue;
        }
        // Build the user dirent.
        let mut ud = UserDirent {
            d_ino: entry.inum as u64,
            d_reclen: out_size as u16,
            d_namelen: 0,
            d_name: [0; 14],
            _pad: [0; 2],
        };
        let mut nl = 0u16;
        for i in 0..DIRSIZ {
            if entry.name[i] == 0 {
                break;
            }
            ud.d_name[i] = entry.name[i];
            nl += 1;
        }
        ud.d_namelen = nl;
        // Copy out.
        let bytes = unsafe {
            core::slice::from_raw_parts(
                &ud as *const _ as *const u8,
                out_size,
            )
        };
        for (i, b) in bytes.iter().enumerate() {
            let Some(kva) = proc.translate_user_write(buf_va + copied + i) else {
                return -1;
            };
            unsafe { *(kva as *mut u8) = *b };
        }
        copied += out_size;
    }
    off.store(cur_off, Ordering::Release);
    copied as i64
}

/// POSIX `alarm(seconds)` — schedule SIGALRM for the calling proc
/// in `seconds` real time. `seconds == 0` cancels any pending alarm.
/// Returns the remaining seconds of any prior alarm (0 if none).
fn sys_alarm(proc: &Arc<Proc>, seconds: u32) -> i64 {
    let now = Arch::now_ticks();
    let prev_deadline = proc.alarm_deadline.load(Ordering::Acquire);
    let prev_remaining = if prev_deadline > now {
        ((prev_deadline - now) / TIMER_INTERVAL) as i64
    } else {
        0
    };
    // Bump the generation — any in-flight alarm entry for the old
    // deadline will see its generation != proc.alarm_generation when
    // it fires and skip delivery.
    let new_gen = proc.alarm_generation.fetch_add(1, Ordering::AcqRel) + 1;
    if seconds == 0 {
        proc.alarm_deadline.store(0, Ordering::Release);
    } else {
        let deadline = now + (seconds as u64) * TIMER_INTERVAL;
        proc.alarm_deadline.store(deadline, Ordering::Release);
        crate::time::add_alarm(deadline, proc.pid, new_gen);
    }
    prev_remaining
}

struct Sleep {
    deadline: u64,
}

impl Future for Sleep {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        if current_proc_killed() {
            return Poll::Ready(());
        }
        if Arch::now_ticks() >= self.deadline {
            return Poll::Ready(());
        }
        crate::time::add_timer(self.deadline, cx.waker().clone());
        Poll::Pending
    }
}

// ---------- fd-dispatched read/write ----------------------------------------

async fn sys_write(proc: &Arc<Proc>, fd: i32, buf_va: usize, len: usize) -> i64 {
    let Some(entry) = proc.get_fd_entry(fd) else {
        return -1;
    };
    let nonblock = entry.nonblock;
    match &*entry.file {
        File::Console => console_write(proc, buf_va, len),
        File::PipeWrite(p) => pipe_write(p.clone(), proc, buf_va, len, nonblock).await,
        File::PipeRead(_) => -1,
        File::Inode {
            ip,
            off,
            readable: _,
            writable,
            append,
        } => {
            if !*writable {
                return -1;
            }
            inode_write(proc, ip.clone(), off, buf_va, len, *append).await
        }
    }
}

async fn inode_write(
    proc: &Arc<Proc>,
    ip: Arc<crate::fs::inode::Inode>,
    off: &AtomicU32,
    buf_va: usize,
    len: usize,
    append: bool,
) -> i64 {
    // Copy user bytes into a kernel buffer first so that writei (which
    // does many awaits) doesn't have to keep crossing into user VA.
    let mut tmp = alloc::vec![0u8; len];
    for i in 0..len {
        let Some(kva) = proc.translate_user(buf_va + i) else {
            return -1;
        };
        tmp[i] = unsafe { *(kva as *const u8) };
    }

    fs::log::begin_op().await;
    let mut li = fs::inode::ilock(&ip).await;
    // O_APPEND: re-read the inode's size *under the lock* and write
    // from there, ignoring whatever offset the fd holds. This is
    // POSIX's atomicity guarantee — concurrent appenders never
    // overwrite each other.
    let cur = if append {
        li.state().size
    } else {
        off.load(Ordering::Acquire)
    };
    let n = fs::inode::writei(&mut li, &tmp, cur).await;
    drop(li);
    fs::log::end_op().await;
    if n > 0 {
        off.store(cur + n as u32, Ordering::Release);
    }
    n as i64
}

fn console_write(proc: &Proc, buf_va: usize, len: usize) -> i64 {
    let mut va = buf_va;
    let mut written: usize = 0;
    while written < len {
        let Some(kva) = proc.translate_user(va) else {
            return -1;
        };
        let byte = unsafe { *(kva as *const u8) };
        Arch::console_putc(byte);
        va += 1;
        written += 1;
    }
    written as i64
}

async fn sys_read(proc: &Arc<Proc>, fd: i32, buf_va: usize, len: usize) -> i64 {
    if len == 0 {
        return 0;
    }
    let Some(entry) = proc.get_fd_entry(fd) else {
        return -1;
    };
    let nonblock = entry.nonblock;
    match &*entry.file {
        File::Console => console_read(proc, buf_va, len, nonblock).await,
        File::PipeRead(p) => pipe_read(p.clone(), proc, buf_va, len, nonblock).await,
        File::PipeWrite(_) => -1,
        File::Inode {
            ip,
            off,
            readable,
            writable: _,
            append: _,
        } => {
            if !*readable {
                return -1;
            }
            inode_read(proc, ip.clone(), off, buf_va, len).await
        }
    }
}

async fn inode_read(
    proc: &Arc<Proc>,
    ip: Arc<crate::fs::inode::Inode>,
    off: &AtomicU32,
    buf_va: usize,
    len: usize,
) -> i64 {
    let cur = off.load(Ordering::Acquire);
    let mut tmp = alloc::vec![0u8; len];
    let li = fs::inode::ilock(&ip).await;
    let n = fs::inode::readi(&li, &mut tmp, cur).await;
    drop(li);
    if n == 0 {
        return 0;
    }
    // Copy the bytes out into the user buffer, page-aware. Use the
    // write-checking translate so we refuse to scribble on user
    // code / RO pages.
    let mut copied = 0usize;
    while copied < n {
        let Some(kva) = proc.translate_user_write(buf_va + copied) else {
            return -1;
        };
        unsafe { *(kva as *mut u8) = tmp[copied] };
        copied += 1;
    }
    off.store(cur + n as u32, Ordering::Release);
    n as i64
}

async fn console_read(proc: &Proc, buf_va: usize, len: usize, nonblock: bool) -> i64 {
    let mut n: usize = 0;
    while n < len {
        // POSIX O_NONBLOCK: first byte may block normally if we
        // haven't gotten anything yet; if the FIFO is empty and
        // nonblock, return -1 (EAGAIN). After any bytes have been
        // read, return what we have rather than block further.
        if nonblock {
            match crate::console_in::try_pop() {
                Some(b) => {
                    let Some(kva) = proc.translate_user_write(buf_va + n) else {
                        return -1;
                    };
                    unsafe { *(kva as *mut u8) = b };
                    n += 1;
                    if b == b'\n' {
                        break;
                    }
                }
                None => {
                    return if n == 0 { -1 } else { n as i64 };
                }
            }
        } else {
            let Some(b) = ConsoleByte.await else {
                return -1; // killed
            };
            let Some(kva) = proc.translate_user_write(buf_va + n) else {
                return -1;
            };
            unsafe { *(kva as *mut u8) = b };
            n += 1;
            if b == b'\n' {
                break;
            }
        }
    }
    n as i64
}

struct ConsoleByte;

impl Future for ConsoleByte {
    type Output = Option<u8>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<u8>> {
        if current_proc_killed() {
            return Poll::Ready(None);
        }
        crate::console_in::register_waker(cx.waker());
        if let Some(b) = crate::console_in::try_pop() {
            return Poll::Ready(Some(b));
        }
        Poll::Pending
    }
}

// ---------- pipes -----------------------------------------------------------

async fn sys_pipe(proc: &Arc<Proc>, pipefd_va: usize) -> i64 {
    let inner = Arc::new(PipeInner::new());
    let read_file = Arc::new(File::PipeRead(inner.clone()));
    let write_file = Arc::new(File::PipeWrite(inner));

    let Some(rfd) = proc.alloc_fd(read_file) else {
        return -1;
    };
    let Some(wfd) = proc.alloc_fd(write_file) else {
        proc.close_fd(rfd);
        return -1;
    };

    for (i, fd) in [rfd, wfd].iter().enumerate() {
        let va = pipefd_va + i * 4;
        let Some(kva) = proc.translate_user_write(va) else {
            return -1;
        };
        unsafe { *(kva as *mut i32) = *fd };
    }
    0
}

async fn sys_close(proc: &Arc<Proc>, fd: i32) -> i64 {
    proc.close_fd(fd)
}

/// POSIX `pread(fd, buf, len, off)`. Reads `len` bytes from the
/// inode backing `fd` starting at `off`, **without** advancing the
/// file's offset. Only valid on `File::Inode` (pipes/console: -1).
async fn sys_pread(
    proc: &Arc<Proc>,
    fd: i32,
    buf_va: usize,
    len: usize,
    offset: i64,
) -> i64 {
    if len == 0 {
        return 0;
    }
    if offset < 0 || offset > u32::MAX as i64 {
        return -1;
    }
    let Some(file) = proc.get_file(fd) else {
        return -1;
    };
    let File::Inode {
        ip, readable, ..
    } = &*file
    else {
        return -1;
    };
    if !*readable {
        return -1;
    }
    let cur = offset as u32;
    let mut tmp = alloc::vec![0u8; len];
    let li = fs::inode::ilock(ip).await;
    let n = fs::inode::readi(&li, &mut tmp, cur).await;
    drop(li);
    if n == 0 {
        return 0;
    }
    let mut copied = 0usize;
    while copied < n {
        let Some(kva) = proc.translate_user_write(buf_va + copied) else {
            return -1;
        };
        unsafe { *(kva as *mut u8) = tmp[copied] };
        copied += 1;
    }
    n as i64
}

/// POSIX `pwrite(fd, buf, len, off)`. Writes at explicit offset
/// without touching the file's position. Inode-backed fds only.
async fn sys_pwrite(
    proc: &Arc<Proc>,
    fd: i32,
    buf_va: usize,
    len: usize,
    offset: i64,
) -> i64 {
    if len == 0 {
        return 0;
    }
    if offset < 0 || offset > u32::MAX as i64 {
        return -1;
    }
    let Some(file) = proc.get_file(fd) else {
        return -1;
    };
    let File::Inode {
        ip, writable, ..
    } = &*file
    else {
        return -1;
    };
    if !*writable {
        return -1;
    }
    let mut tmp = alloc::vec![0u8; len];
    for i in 0..len {
        let Some(kva) = proc.translate_user(buf_va + i) else {
            return -1;
        };
        tmp[i] = unsafe { *(kva as *const u8) };
    }
    let cur = offset as u32;
    fs::log::begin_op().await;
    let mut li = fs::inode::ilock(ip).await;
    let n = fs::inode::writei(&mut li, &tmp, cur).await;
    drop(li);
    fs::log::end_op().await;
    n as i64
}

/// POSIX `lseek(int fd, off_t offset, int whence)`. Supports
/// SEEK_SET/SEEK_CUR/SEEK_END on `File::Inode`. Pipes and the
/// console don't support seeking; we return -1 for those.
///
/// Returns the new absolute offset on success, or -1 on error. We
/// model the offset as a u32 (matches xv6's inode size). Negative
/// resulting offsets are rejected.
async fn sys_lseek(proc: &Arc<Proc>, fd: i32, offset: i64, whence: i32) -> i64 {
    let Some(file) = proc.get_file(fd) else {
        return -1;
    };
    let File::Inode { ip, off, .. } = &*file else {
        return -1;
    };
    let cur = off.load(Ordering::Acquire) as i64;
    let end = {
        let li = fs::inode::ilock(ip).await;
        li.state().size as i64
    };
    let new = match whence {
        x if x == SEEK_SET => offset,
        x if x == SEEK_CUR => cur + offset,
        x if x == SEEK_END => end + offset,
        _ => return -1,
    };
    if new < 0 || new > u32::MAX as i64 {
        return -1;
    }
    off.store(new as u32, Ordering::Release);
    new
}

async fn sys_dup(proc: &Arc<Proc>, fd: i32) -> i64 {
    let Some(file) = proc.get_file(fd) else {
        return -1;
    };
    // Give the new fd its own `Arc<File>` so its close drops
    // independently. `File::Clone` bumps pipe counts.
    let new_file = Arc::new((*file).clone());
    proc.alloc_fd(new_file).map(|f| f as i64).unwrap_or(-1)
}

/// POSIX `dup2(oldfd, newfd)` — duplicate `oldfd` onto `newfd`,
/// closing whatever was at `newfd` first. If `oldfd == newfd` (and
/// `oldfd` is valid), it's a no-op and returns `newfd`. Returns the
/// new fd number, or -1 on error.
fn sys_dup2(proc: &Arc<Proc>, oldfd: i32, newfd: i32) -> i64 {
    if oldfd < 0 || newfd < 0 {
        return -1;
    }
    // oldfd must be open.
    let Some(file) = proc.get_file(oldfd) else {
        return -1;
    };
    if oldfd == newfd {
        return newfd as i64;
    }
    // Bounds check newfd.
    if (newfd as usize) >= crate::proc::NOFILE {
        return -1;
    }
    // Close any existing entry at newfd (ignoring error — entry
    // may already be empty), then install a fresh FdEntry.
    let _ = proc.close_fd(newfd);
    let new_file = Arc::new((*file).clone());
    {
        let mut files = proc.files.lock();
        files[newfd as usize] = Some(crate::file::FdEntry::new(new_file));
    }
    newfd as i64
}

async fn pipe_write(
    pipe: Arc<PipeInner>,
    proc: &Arc<Proc>,
    buf_va: usize,
    len: usize,
    nonblock: bool,
) -> i64 {
    let mut n: usize = 0;
    while n < len {
        if proc.killed.load(Ordering::Acquire) {
            return -1;
        }
        let Some(kva) = proc.translate_user(buf_va + n) else {
            return -1;
        };
        let byte = unsafe { *(kva as *const u8) };
        if nonblock {
            // Fast-path: try to push directly without awaiting.
            // If the buffer is full and readers are still attached,
            // bail with EAGAIN (or partial); on no-readers, normal
            // failure path.
            let mut buf = pipe.buf.lock();
            if buf.len() < pipe.cap() {
                buf.push_back(byte);
                drop(buf);
                pipe.read_waker.wake();
                n += 1;
                continue;
            }
            drop(buf);
            if pipe.readers.load(Ordering::Acquire) == 0 {
                return -1; // EPIPE-ish
            }
            return if n == 0 { -1 } else { n as i64 };
        }
        if !(PipeWriteByte {
            pipe: pipe.clone(),
            byte,
        }
        .await)
        {
            return -1; // killed mid-write
        }
        n += 1;
    }
    n as i64
}

struct PipeWriteByte {
    pipe: Arc<PipeInner>,
    byte: u8,
}

impl Future for PipeWriteByte {
    type Output = bool; // true on success, false if killed

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<bool> {
        if current_proc_killed() {
            return Poll::Ready(false);
        }
        self.pipe.write_waker.register(cx.waker());
        let mut buf = self.pipe.buf.lock();
        if buf.len() < self.pipe.cap() {
            buf.push_back(self.byte);
            drop(buf);
            self.pipe.read_waker.wake();
            return Poll::Ready(true);
        }
        // No readers left? Writers should fail rather than block.
        if self.pipe.readers.load(Ordering::Acquire) == 0 {
            return Poll::Ready(false);
        }
        Poll::Pending
    }
}

async fn pipe_read(
    pipe: Arc<PipeInner>,
    proc: &Arc<Proc>,
    buf_va: usize,
    len: usize,
    nonblock: bool,
) -> i64 {
    let mut n: usize = 0;
    while n < len {
        // O_NONBLOCK fast-path: don't await if there's nothing
        // ready right now. Return -1 (EAGAIN) only if we haven't
        // copied anything yet; otherwise return what we got.
        if nonblock {
            let popped = pipe.buf.lock().pop_front();
            match popped {
                Some(b) => {
                    pipe.write_waker.wake();
                    let Some(kva) = proc.translate_user_write(buf_va + n) else {
                        return -1;
                    };
                    unsafe { *(kva as *mut u8) = b };
                    n += 1;
                    if pipe.buf.lock().is_empty() {
                        break;
                    }
                }
                None => {
                    // Empty: if writers all closed, EOF (return 0
                    // or what we have). Else EAGAIN.
                    if pipe.writers.load(Ordering::Acquire) == 0 {
                        break;
                    }
                    return if n == 0 { -1 } else { n as i64 };
                }
            }
            continue;
        }
        match (PipeReadByte { pipe: pipe.clone() }).await {
            Some(b) => {
                let Some(kva) = proc.translate_user_write(buf_va + n) else {
                    return -1;
                };
                unsafe { *(kva as *mut u8) = b };
                n += 1;
                // Drain whatever's left without re-blocking; if nothing
                // is left, the next `await` would block, so just return
                // what we have.
                if pipe.buf.lock().is_empty() {
                    break;
                }
            }
            None => break, // EOF: writers all closed and buf empty.
        }
    }
    n as i64
}

struct PipeReadByte {
    pipe: Arc<PipeInner>,
}

impl Future for PipeReadByte {
    type Output = Option<u8>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<u8>> {
        if current_proc_killed() {
            return Poll::Ready(None);
        }
        // Register first, then check — closes the wake-lost race.
        self.pipe.read_waker.register(cx.waker());
        let popped = self.pipe.buf.lock().pop_front();
        if let Some(b) = popped {
            self.pipe.write_waker.wake();
            return Poll::Ready(Some(b));
        }
        if self.pipe.writers.load(Ordering::Acquire) == 0 {
            return Poll::Ready(None);
        }
        Poll::Pending
    }
}

// ---------- sys_open / sys_fstat -------------------------------------------

async fn sys_open(proc: &Arc<Proc>, path_va: usize, flags: u32) -> i64 {
    let Some(path) = read_user_cstring(proc, path_va, 128) else {
        return -1;
    };
    let allowed =
        O_RDONLY | O_WRONLY | O_RDWR | O_CREATE | O_TRUNC | O_APPEND | O_CLOEXEC | O_NONBLOCK;
    if (flags & !allowed) != 0 {
        return -1;
    }

    let ip = if (flags & O_CREATE) != 0 {
        match create_at_path(proc, &path, xv6_fs_layout::T_FILE, 0, 0).await {
            Some(i) => i,
            None => return -1,
        }
    } else {
        let Some(i) = resolve_path(proc, &path).await else {
            return -1;
        };
        i
    };

    let readable = (flags & O_WRONLY) == 0;
    let writable = (flags & (O_WRONLY | O_RDWR)) != 0;
    let append = (flags & O_APPEND) != 0;

    let typ;
    {
        let mut li = fs::inode::ilock(&ip).await;
        typ = li.state().typ;
        if typ == 0 {
            return -1;
        }
        if typ == xv6_fs_layout::T_DIR && (flags & (O_WRONLY | O_RDWR)) != 0 {
            return -1; // can't open a directory for writing
        }
        // POSIX permission check. Skip when the user just got handed
        // back the inode they created (O_CREATE) — they already have
        // the right since we just minted it.
        if (flags & O_CREATE) == 0 {
            let s = li.state();
            if !check_access(proc, s.uid, s.gid, s.mode, readable, writable) {
                return -1;
            }
        }
        if (flags & O_TRUNC) != 0 && typ == xv6_fs_layout::T_FILE {
            fs::log::begin_op().await;
            fs::inode::itrunc(&mut li).await;
            // itrunc already called iupdate; just close the op.
            drop(li);
            fs::log::end_op().await;
        }
    }
    let f = Arc::new(File::Inode {
        ip,
        off: AtomicU32::new(0),
        readable,
        writable,
        append,
    });
    let entry = crate::file::FdEntry {
        file: f,
        cloexec: (flags & O_CLOEXEC) != 0,
        nonblock: (flags & O_NONBLOCK) != 0,
    };
    proc.alloc_fd_entry(entry).map(|fd| fd as i64).unwrap_or(-1)
}

/// POSIX `fcntl(fd, cmd, arg)`. Subset:
///   F_DUPFD(start)         — dup to lowest fd ≥ start
///   F_DUPFD_CLOEXEC(start) — same, with cloexec set
///   F_GETFD                — read FD_CLOEXEC bit
///   F_SETFD(flags)         — write FD_CLOEXEC bit
///   F_GETFL                — read O_APPEND/O_NONBLOCK status flags
///   F_SETFL(flags)         — write O_NONBLOCK (others ignored)
fn sys_fcntl(proc: &Arc<Proc>, fd: i32, cmd: i32, arg: i64) -> i64 {
    let Some(entry) = proc.get_fd_entry(fd) else {
        return -1;
    };
    match cmd {
        x if x == F_DUPFD => {
            let mut e = entry.clone();
            // POSIX F_DUPFD strips cloexec by definition.
            e.cloexec = false;
            proc.alloc_fd_entry_from(e, arg as i32)
                .map(|n| n as i64)
                .unwrap_or(-1)
        }
        x if x == F_DUPFD_CLOEXEC => {
            let mut e = entry.clone();
            e.cloexec = true;
            proc.alloc_fd_entry_from(e, arg as i32)
                .map(|n| n as i64)
                .unwrap_or(-1)
        }
        x if x == F_GETFD => {
            if entry.cloexec {
                FD_CLOEXEC as i64
            } else {
                0
            }
        }
        x if x == F_SETFD => {
            let cloexec = (arg & FD_CLOEXEC as i64) != 0;
            if proc.set_fd_cloexec(fd, cloexec) {
                0
            } else {
                -1
            }
        }
        x if x == F_GETFL => {
            // We carry per-File "append" inside File::Inode; the
            // user-visible O_APPEND in F_GETFL means "this fd was
            // opened append-mode". Pull it from the File enum.
            let mut flags: i64 = 0;
            if let crate::file::File::Inode { append, .. } = &*entry.file {
                if *append {
                    flags |= O_APPEND as i64;
                }
            }
            if entry.nonblock {
                flags |= O_NONBLOCK as i64;
            }
            flags
        }
        x if x == F_SETFL => {
            // Only O_NONBLOCK is a settable status flag in our subset.
            // (O_APPEND mutation post-open isn't supported.)
            let nb = (arg & O_NONBLOCK as i64) != 0;
            if proc.set_fd_nonblock(fd, nb) {
                0
            } else {
                -1
            }
        }
        _ => -1,
    }
}

async fn sys_mkdir(proc: &Arc<Proc>, path_va: usize) -> i64 {
    let Some(path) = read_user_cstring(proc, path_va, 128) else {
        return -1;
    };
    match create_at_path(proc, &path, xv6_fs_layout::T_DIR, 0, 0).await {
        Some(_) => 0,
        None => -1,
    }
}

async fn sys_mknod(proc: &Arc<Proc>, path_va: usize, major: u16, minor: u16) -> i64 {
    let Some(path) = read_user_cstring(proc, path_va, 128) else {
        return -1;
    };
    match create_at_path(proc, &path, xv6_fs_layout::T_DEVICE, major, minor).await {
        Some(_) => 0,
        None => -1,
    }
}

async fn sys_unlink(proc: &Arc<Proc>, path_va: usize) -> i64 {
    let Some(path) = read_user_cstring(proc, path_va, 128) else {
        return -1;
    };
    let Some((dir, name)) = nameiparent_via_cwd(proc, &path).await else {
        return -1;
    };
    if name == "." || name == ".." {
        return -1;
    }
    fs::log::begin_op().await;
    let result: i64 = unlink_inside_op(&dir, &name).await;
    fs::log::end_op().await;
    result
}

async fn unlink_inside_op(
    dir: &Arc<crate::fs::inode::Inode>,
    name: &str,
) -> i64 {
    let mut dir_li = fs::inode::ilock(dir).await;
    let Some((child_ip, off)) =
        crate::fs::dir::dirlookup_full(&dir_li, name).await
    else {
        return -1;
    };
    let mut child_li = fs::inode::ilock(&child_ip).await;
    let child_typ = child_li.state().typ;
    if child_typ == xv6_fs_layout::T_DIR
        && !crate::fs::dir::dir_is_empty(&child_li).await
    {
        return -1;
    }
    crate::fs::dir::dirunlink_at(&mut dir_li, off).await;
    if child_typ == xv6_fs_layout::T_DIR {
        dir_li.state_mut().nlink -= 1;
        fs::inode::iupdate(&dir_li).await;
    }
    child_li.state_mut().nlink -= 1;
    if child_li.state().nlink == 0 && Arc::strong_count(&child_ip) <= 2 {
        fs::inode::itrunc(&mut child_li).await;
        child_li.state_mut().typ = 0;
        fs::inode::iupdate(&child_li).await;
    } else {
        fs::inode::iupdate(&child_li).await;
    }
    0
}

/// Allocate a new inode of `typ` and link it at `path`. For T_DIR,
/// also populates `.` and `..`. Returns the new inode on success.
async fn create_at_path(
    proc: &Arc<Proc>,
    path: &str,
    typ: u16,
    major: u16,
    minor: u16,
) -> Option<Arc<crate::fs::inode::Inode>> {
    let (dir, name) = nameiparent_via_cwd(proc, path).await?;
    if name.is_empty() || name == "." || name == ".." || name.len() > xv6_fs_layout::DIRSIZ
    {
        return None;
    }
    // POSIX default mode by file type, then mask with the proc's
    // umask. With the default umask=0o022 this matches what we used
    // to hard-code (0o644 file, 0o755 dir).
    let default_mode: u16 = match typ {
        xv6_fs_layout::T_DIR => 0o777,
        xv6_fs_layout::T_FILE => 0o666,
        xv6_fs_layout::T_DEVICE => 0o666,
        _ => 0o666,
    };
    let umask = proc.umask.load(Ordering::Acquire) as u16;
    let mode = default_mode & !umask;
    let uid = proc.uid.load(Ordering::Acquire) as u16;
    let gid = proc.gid.load(Ordering::Acquire) as u16;
    fs::log::begin_op().await;
    let result =
        create_inside_op(&dir, &name, typ, major, minor, mode, uid, gid).await;
    fs::log::end_op().await;
    result
}

async fn create_inside_op(
    dir: &Arc<crate::fs::inode::Inode>,
    name: &str,
    typ: u16,
    major: u16,
    minor: u16,
    mode: u16,
    uid: u16,
    gid: u16,
) -> Option<Arc<crate::fs::inode::Inode>> {
    let mut dir_li = fs::inode::ilock(dir).await;

    // If the entry already exists and we're opening a regular file,
    // return the existing inode (matches xv6's `create` for O_CREATE
    // without O_EXCL). mkdir/mknod of an existing entry fails.
    if let Some((existing, _)) =
        crate::fs::dir::dirlookup_full(&dir_li, name).await
    {
        if typ != xv6_fs_layout::T_FILE {
            return None;
        }
        let exist_li = fs::inode::ilock(&existing).await;
        let existing_typ = exist_li.state().typ;
        drop(exist_li);
        if existing_typ == xv6_fs_layout::T_FILE
            || existing_typ == xv6_fs_layout::T_DEVICE
        {
            return Some(existing);
        }
        return None;
    }

    let dev = dir_li.dev();
    let child = fs::inode::ialloc(dev, typ, mode).await?;
    {
        let mut child_li = fs::inode::ilock(&child).await;
        child_li.state_mut().major = major;
        child_li.state_mut().minor = minor;
        child_li.state_mut().nlink = 1;
        child_li.state_mut().uid = uid;
        child_li.state_mut().gid = gid;
        fs::inode::iupdate(&child_li).await;

        if typ == xv6_fs_layout::T_DIR {
            let dir_inum = dir_li.inum() as u16;
            let child_inum = child_li.inum() as u16;
            if !crate::fs::dir::dirlink(&mut child_li, ".", child_inum).await
                || !crate::fs::dir::dirlink(&mut child_li, "..", dir_inum).await
            {
                return None;
            }
        }
    }
    let child_inum = child.inum.load(Ordering::Acquire) as u16;
    if !crate::fs::dir::dirlink(&mut dir_li, name, child_inum).await {
        return None;
    }
    if typ == xv6_fs_layout::T_DIR {
        dir_li.state_mut().nlink += 1;
        fs::inode::iupdate(&dir_li).await;
    }
    Some(child)
}

async fn sys_fstat(proc: &Arc<Proc>, fd: i32, stat_va: usize) -> i64 {
    let Some(file) = proc.get_file(fd) else {
        return -1;
    };
    let File::Inode { ip, .. } = &*file else {
        return -1;
    };
    stat_inode_into_user(proc, ip, stat_va).await
}

/// Discretionary-access check. Returns true if `proc` may open the
/// inode with the requested read/write bits.
///
/// Standard 3-tier POSIX: owner → group → world. The kernel uses
/// uid 0 (root) as the universal bypass. We have no group membership
/// table yet, so a user is in a group only if their proc.gid matches
/// the inode's gid.
fn check_access(
    proc: &Arc<Proc>,
    inode_uid: u16,
    inode_gid: u16,
    inode_mode: u16,
    want_read: bool,
    want_write: bool,
) -> bool {
    let uid = proc.uid.load(Ordering::Acquire);
    if uid == 0 {
        return true;
    }
    let gid = proc.gid.load(Ordering::Acquire);
    let bits = if uid == inode_uid as u32 {
        (inode_mode >> 6) & 0o7
    } else if gid == inode_gid as u32 {
        (inode_mode >> 3) & 0o7
    } else {
        inode_mode & 0o7
    };
    if want_read && (bits & 0o4) == 0 {
        return false;
    }
    if want_write && (bits & 0o2) == 0 {
        return false;
    }
    true
}

/// POSIX `chmod(path, mode)` — change a file's permission bits.
/// Only the owner (or root) may chmod.
async fn sys_chmod(proc: &Arc<Proc>, path_va: usize, mode: u16) -> i64 {
    let Some(path) = read_user_cstring(proc, path_va, 128) else {
        return -1;
    };
    let Some(ip) = resolve_path(proc, &path).await else {
        return -1;
    };
    let caller_uid = proc.uid.load(Ordering::Acquire);
    fs::log::begin_op().await;
    let result: i64 = {
        let mut li = fs::inode::ilock(&ip).await;
        if caller_uid != 0 && (li.state().uid as u32) != caller_uid {
            -1
        } else {
            let s = li.state_mut();
            s.mode = mode;
            s.ctime = fs::inode::now_secs();
            fs::inode::iupdate(&li).await;
            0
        }
    };
    fs::log::end_op().await;
    result
}

/// POSIX `chown(path, uid, gid)` — change owner/group. `uid==-1` or
/// `gid==-1` (sentinel u16::MAX here) leaves that field untouched.
/// Only root may chown to an arbitrary uid; the owner may change gid
/// to a group they're a member of, but we don't track group lists yet
/// so we restrict gid-only changes to root too.
async fn sys_chown(proc: &Arc<Proc>, path_va: usize, uid: u16, gid: u16) -> i64 {
    if proc.uid.load(Ordering::Acquire) != 0 {
        return -1;
    }
    let Some(path) = read_user_cstring(proc, path_va, 128) else {
        return -1;
    };
    let Some(ip) = resolve_path(proc, &path).await else {
        return -1;
    };
    fs::log::begin_op().await;
    {
        let mut li = fs::inode::ilock(&ip).await;
        let now = fs::inode::now_secs();
        if uid != u16::MAX {
            li.state_mut().uid = uid;
        }
        if gid != u16::MAX {
            li.state_mut().gid = gid;
        }
        li.state_mut().ctime = now;
        fs::inode::iupdate(&li).await;
    }
    fs::log::end_op().await;
    0
}

/// POSIX `stat(path, &stat)` — like `fstat` but takes a path string
/// instead of an open fd. We have no symlinks yet, so this is also
/// the kernel-side implementation of `lstat` (which would only
/// diverge on a symlink — `lstat` reports the link, `stat` chases
/// it).
/// POSIX `ftruncate(fd, length)` — resize the file backing `fd` to
/// `length` bytes. Shrink frees data blocks past `length`; grow
/// bumps the size and leaves a sparse hole. Requires `fd` to have
/// been opened with write access.
async fn sys_ftruncate(proc: &Arc<Proc>, fd: i32, length: i64) -> i64 {
    if length < 0 || length > u32::MAX as i64 {
        return -1;
    }
    let Some(file) = proc.get_file(fd) else {
        return -1;
    };
    let File::Inode {
        ip, writable, ..
    } = &*file
    else {
        return -1;
    };
    if !*writable {
        return -1;
    }
    fs::log::begin_op().await;
    {
        let mut li = fs::inode::ilock(ip).await;
        fs::inode::itrunc_to(&mut li, length as u32).await;
    }
    fs::log::end_op().await;
    0
}

/// POSIX `truncate(path, length)` — path-based variant. Requires
/// write permission on the file.
/// POSIX `sigaction(signum, handler, restorer, mask)` — install or
/// query a handler. The kernel keeps handler+restorer+mask in
/// `Proc::sig_actions[signum]`. SIGKILL and SIGSTOP can't be caught.
///
/// Caller convention (matches our slim 4-arg syscall):
///   handler  = user-VA function pointer, SIG_DFL, or SIG_IGN
///   restorer = user-VA "sigreturn" trampoline (ulib's `_sigret`)
///   mask     = signals to block while handler runs
///
/// Returns the previous handler VA, or -1 on error. Slim — we don't
/// support querying restorer/mask back (Slice 3 if needed).
fn sys_sigaction(
    proc: &Arc<Proc>,
    signum: i32,
    handler: usize,
    restorer: usize,
    mask: u32,
) -> i64 {
    use crate::uapi::{SigAction, SIGKILL, SIGSTOP, SIG_DFL};
    if signum <= 0 || signum >= 32 {
        return -1;
    }
    if signum == SIGKILL || signum == SIGSTOP {
        // POSIX: SIGKILL/SIGSTOP can never be caught, blocked, or
        // ignored. Installing a handler on these is an error.
        return -1;
    }
    let mut tbl = proc.sig_actions.lock();
    let prev = tbl[signum as usize].handler;
    tbl[signum as usize] = SigAction { handler, restorer, mask };
    // Caller can detect "was previously default" by checking
    // returned value against SIG_DFL.
    let _ = SIG_DFL;
    prev as i64
}

/// POSIX `sigreturn()` — restore the trapframe the kernel saved
/// when it dispatched the current handler. The user-space restorer
/// stub (`_sigret` in ulib) calls this with no args; we just swap
/// the saved frame back into `proc.trapframe()` and let the normal
/// return-to-user path resume the original PC/SP/etc.
fn sys_sigreturn(proc: &Arc<Proc>) -> i64 {
    let Some(saved) = proc.sig_saved_frame.lock().take() else {
        // Called without a pending dispatch — kill the process; it's
        // either confused or attacking us.
        return -1;
    };
    let tf = proc.trapframe();
    *tf = saved;
    // Restore the blocked mask that was in force before delivery.
    let prev = proc
        .sig_saved_blocked
        .load(core::sync::atomic::Ordering::Acquire);
    proc.sig_blocked
        .store(prev, core::sync::atomic::Ordering::Release);
    // sys_sigreturn must NOT clobber the restored a0/x0. The
    // syscall dispatch wraps our return value into the trapframe's
    // arg 0 slot, so we have to make sure we return what was in a0
    // before signal dispatch. The simplest correct thing is to
    // return the freshly-restored a0 — proc_main will write it
    // back, no-op.
    tf.arg(0) as i64
}

/// POSIX `sigprocmask(how, &set, &oldset)`. `how`:
///   * SIG_BLOCK   — `blocked |= set`
///   * SIG_UNBLOCK — `blocked &= ~set`
///   * SIG_SETMASK — `blocked = set`
///
/// Writes the previous mask into `oldset` if non-null. SIGKILL and
/// SIGSTOP can never be blocked — strip them from any incoming set.
fn sys_sigprocmask(
    proc: &Arc<Proc>,
    how: i32,
    set: u32,
    oldset_va: usize,
) -> i64 {
    use crate::uapi::{SIG_BLOCK, SIG_SETMASK, SIG_UNBLOCK, SIGKILL, SIGSTOP};
    // Mask off the unblockable signals.
    let set = set & !((1u32 << SIGKILL) | (1u32 << SIGSTOP));
    let prev = proc.sig_blocked.load(core::sync::atomic::Ordering::Acquire);
    let new = match how {
        x if x == SIG_BLOCK => prev | set,
        x if x == SIG_UNBLOCK => prev & !set,
        x if x == SIG_SETMASK => set,
        _ => return -1,
    };
    proc.sig_blocked
        .store(new, core::sync::atomic::Ordering::Release);
    if oldset_va != 0 {
        // Write the 4-byte u32 to the user buffer.
        let bytes = prev.to_le_bytes();
        for (i, b) in bytes.iter().enumerate() {
            let Some(kva) = proc.translate_user_write(oldset_va + i) else {
                return -1;
            };
            unsafe { *(kva as *mut u8) = *b };
        }
    }
    0
}

/// POSIX `getcwd(buf, len)` — write the absolute path of the current
/// working directory into `buf[0..len]` (NUL-terminated). Returns
/// the length written (not counting NUL) or -1 if `buf` is too small,
/// `buf` isn't writable, or the cwd is unreachable from root.
async fn sys_getcwd(proc: &Arc<Proc>, buf_va: usize, len: usize) -> i64 {
    use alloc::string::String;
    if len == 0 {
        return -1;
    }
    let cwd = match proc.cwd.lock().clone() {
        Some(c) => c,
        None => return -1,
    };
    // Walk leaf-to-root, prepending "/<name>" each step. Root is
    // inum 1 by convention (xv6 mkfs). When current inum == 1 we
    // stop.
    let mut current = cwd;
    let mut parts: alloc::vec::Vec<String> = alloc::vec::Vec::new();
    // Guard against infinite loops (corrupt fs).
    for _ in 0..64 {
        let (cur_inum, cur_is_root) = {
            let li = fs::inode::ilock(&current).await;
            (li.inum() as u16, li.inum() == 1)
        };
        if cur_is_root {
            break;
        }
        // Open ".." from current. ".." is always present in dirs.
        let parent = {
            let li = fs::inode::ilock(&current).await;
            crate::fs::dir::dirlookup(&li, "..").await
        };
        let Some(parent) = parent else {
            return -1;
        };
        // Find current's name in parent's dirents.
        let name = {
            let li = fs::inode::ilock(&parent).await;
            crate::fs::dir::dirlookup_by_inum(&li, cur_inum).await
        };
        let Some(name) = name else { return -1 };
        parts.push(name);
        current = parent;
    }
    // Build the absolute path. parts is leaf-first; reverse.
    let path = if parts.is_empty() {
        String::from("/")
    } else {
        let mut s = String::new();
        for p in parts.iter().rev() {
            s.push('/');
            s.push_str(p);
        }
        s
    };
    let bytes = path.as_bytes();
    if bytes.len() + 1 > len {
        return -1;
    }
    for (i, b) in bytes.iter().enumerate() {
        let Some(kva) = proc.translate_user_write(buf_va + i) else {
            return -1;
        };
        unsafe { *(kva as *mut u8) = *b };
    }
    let Some(kva) = proc.translate_user_write(buf_va + bytes.len()) else {
        return -1;
    };
    unsafe { *(kva as *mut u8) = 0 };
    bytes.len() as i64
}

/// POSIX `rename(old, new)` — atomic-ish rename within the same
/// directory; cross-directory rename does link+unlink under one log
/// op so it's all-or-nothing (matches POSIX atomicity requirements).
async fn sys_rename(proc: &Arc<Proc>, old_va: usize, new_va: usize) -> i64 {
    let Some(old_path) = read_user_cstring(proc, old_va, 128) else {
        return -1;
    };
    let Some(new_path) = read_user_cstring(proc, new_va, 128) else {
        return -1;
    };
    // Resolve the source up front; if it doesn't exist, fail before
    // taking the log lock.
    let src = match resolve_path(proc, &old_path).await {
        Some(s) => s,
        None => return -1,
    };
    let Some((old_dir, old_name)) = nameiparent_via_cwd(proc, &old_path).await else {
        return -1;
    };
    let Some((new_dir, new_name)) = nameiparent_via_cwd(proc, &new_path).await else {
        return -1;
    };
    if old_name == "." || old_name == ".." || new_name == "." || new_name == ".." {
        return -1;
    }
    // Same-name no-op: just succeed. Compare inums via Arc identity
    // — locking the same inode twice would deadlock (xv6's ilock
    // spins on an AtomicBool).
    let same_dir = Arc::as_ptr(&old_dir) == Arc::as_ptr(&new_dir);
    if same_dir && old_name == new_name {
        return 0;
    }
    let src_inum = {
        let li = fs::inode::ilock(&src).await;
        li.inum() as u16
    };
    fs::log::begin_op().await;
    let result: i64 = {
        // If `new` already exists, unlink it first (POSIX allows
        // overwriting a regular file; refuses to overwrite a dir).
        let existing_new = {
            let li = fs::inode::ilock(&new_dir).await;
            crate::fs::dir::dirlookup(&li, &new_name).await
        };
        if let Some(ex) = existing_new {
            let ex_typ = {
                let li = fs::inode::ilock(&ex).await;
                li.state().typ
            };
            if ex_typ == xv6_fs_layout::T_DIR {
                fs::log::end_op().await;
                return -1;
            }
            // Drop the dirent for the existing target.
            if !unlink_dirent_inside_op(&new_dir, &new_name).await {
                fs::log::end_op().await;
                return -1;
            }
            // Decrement nlink on the old target inode. (Borrow from
            // sys_unlink's logic — simplified.)
            let mut ex_li = fs::inode::ilock(&ex).await;
            let n = ex_li.state().nlink;
            ex_li.state_mut().nlink = n.saturating_sub(1);
            fs::inode::iupdate(&ex_li).await;
        }
        // Link src into new_dir under new_name.
        let linked = {
            let mut new_li = fs::inode::ilock(&new_dir).await;
            crate::fs::dir::dirlink(&mut new_li, &new_name, src_inum).await
        };
        if !linked {
            -1
        } else if !unlink_dirent_inside_op(&old_dir, &old_name).await {
            // Rare — we successfully linked under new name but the
            // old dirent removal failed. Leave both names pointing
            // at the inode (nlink reflects two refs); user can clean
            // up manually.
            -1
        } else {
            0
        }
    };
    fs::log::end_op().await;
    result
}

/// Remove a single dirent from `dir` by name (no recursion, no
/// inode bookkeeping — caller handles nlink). Returns true on
/// success. Used by rename's overwrite path.
async fn unlink_dirent_inside_op(
    dir: &Arc<crate::fs::inode::Inode>,
    name: &str,
) -> bool {
    use xv6_fs_layout::{Dirent, DIRSIZ};
    let mut dir_li = fs::inode::ilock(dir).await;
    let entry_size = core::mem::size_of::<Dirent>() as u32;
    let mut off: u32 = 0;
    let dir_size = dir_li.state().size;
    while off < dir_size {
        let mut entry = Dirent::default();
        let bytes = unsafe {
            core::slice::from_raw_parts_mut(
                &mut entry as *mut _ as *mut u8,
                entry_size as usize,
            )
        };
        let n = fs::inode::readi(&dir_li, bytes, off).await;
        if n != entry_size as usize {
            return false;
        }
        // Match (entry.name == name, trimmed).
        let trimmed = entry
            .name
            .iter()
            .position(|&b| b == 0)
            .unwrap_or(DIRSIZ);
        if entry.inum != 0 && &entry.name[..trimmed] == name.as_bytes() {
            let zero = Dirent::default();
            let zb = unsafe {
                core::slice::from_raw_parts(
                    &zero as *const _ as *const u8,
                    entry_size as usize,
                )
            };
            let w = fs::inode::writei(&mut dir_li, zb, off).await;
            return w == entry_size as usize;
        }
        off += entry_size;
    }
    false
}

async fn sys_truncate(proc: &Arc<Proc>, path_va: usize, length: i64) -> i64 {
    if length < 0 || length > u32::MAX as i64 {
        return -1;
    }
    let Some(path) = read_user_cstring(proc, path_va, 128) else {
        return -1;
    };
    let Some(ip) = resolve_path(proc, &path).await else {
        return -1;
    };
    // Check write permission (without actually opening).
    {
        let li = fs::inode::ilock(&ip).await;
        let s = li.state();
        if !check_access(proc, s.uid, s.gid, s.mode, /*r=*/false, /*w=*/true) {
            return -1;
        }
        if s.typ == xv6_fs_layout::T_DIR {
            return -1;
        }
    }
    fs::log::begin_op().await;
    {
        let mut li = fs::inode::ilock(&ip).await;
        fs::inode::itrunc_to(&mut li, length as u32).await;
    }
    fs::log::end_op().await;
    0
}

async fn sys_stat(proc: &Arc<Proc>, path_va: usize, stat_va: usize) -> i64 {
    let Some(path) = read_user_cstring(proc, path_va, 128) else {
        return -1;
    };
    let Some(ip) = resolve_path(proc, &path).await else {
        return -1;
    };
    stat_inode_into_user(proc, &ip, stat_va).await
}

/// Shared implementation: stat the locked inode and copy out the
/// `Stat` struct to the user buffer at `stat_va`.
async fn stat_inode_into_user(
    proc: &Arc<Proc>,
    ip: &Arc<crate::fs::inode::Inode>,
    stat_va: usize,
) -> i64 {
    let (typ, nlink, size, inum, dev, mode_bits, uid, gid, atime, mtime, ctime);
    {
        let li = fs::inode::ilock(ip).await;
        let s = li.state();
        typ = s.typ as i16;
        nlink = s.nlink as i16;
        size = s.size as u64;
        inum = li.inum();
        dev = li.dev() as i32;
        mode_bits = s.mode;
        uid = s.uid;
        gid = s.gid;
        atime = s.atime;
        mtime = s.mtime;
        ctime = s.ctime;
    }
    let st = Stat {
        dev,
        ino: inum,
        typ,
        nlink,
        _pad: 0,
        size,
        mode: crate::uapi::stat_mode(typ as u16, mode_bits),
        uid,
        gid,
        atime,
        mtime,
        ctime,
    };
    let bytes = unsafe {
        core::slice::from_raw_parts(
            &st as *const _ as *const u8,
            core::mem::size_of::<Stat>(),
        )
    };
    for (i, b) in bytes.iter().enumerate() {
        let Some(kva) = proc.translate_user_write(stat_va + i) else {
            return -1;
        };
        unsafe { *(kva as *mut u8) = *b };
    }
    0
}

// ---------- sys_exec --------------------------------------------------------

async fn sys_exec(proc: &Arc<Proc>, path_va: usize, argv_va: usize) -> i64 {
    let Some(path) = read_user_cstring(proc, path_va, 128) else {
        return -1;
    };
    let Some(argv) = read_user_argv(proc, argv_va, MAX_ARGS, MAX_ARG_LEN) else {
        return -1;
    };

    // Load the ELF bytes from the on-disk file pointed to by `path`.
    let bin = match read_file_fully(proc, &path).await {
        Some(b) => b,
        None => {
            crate::println!("exec: no such file: {}", path);
            return -1;
        }
    };

    let image = match crate::user_vm::build_image_from_elf(&bin, proc.trapframe_pa) {
        Ok(img) => img,
        Err(e) => {
            crate::println!("exec: image build failed: {:?}", e);
            return -1;
        }
    };
    let (sp_va, argv_array_va) =
        crate::user_vm::place_argv_on_stack(image.stack_pa, &argv);
    let argc = argv.len() as i64;
    let entry = image.entry;
    let code_end = image.code_end;

    proc.replace_image(image.pagetable, code_end);
    // POSIX: fds marked FD_CLOEXEC close at exec time.
    proc.close_on_exec();
    let tf = proc.trapframe();
    *tf = TrapFrame::default();
    tf.set_epc(entry as u64);
    tf.set_sp(sp_va as u64);
    tf.set_arg(1, argv_array_va as u64);
    // `proc_main` writes our return value into `tf.a0` → becomes the
    // new image's `a0` (i.e., `argc`) when sret to user mode.
    argc
}

/// Resolve `path` against `proc`'s cwd (or `/` if no cwd is set).
async fn resolve_path(proc: &Arc<Proc>, path: &str) -> Option<Arc<crate::fs::inode::Inode>> {
    let cwd = proc.cwd.lock().clone();
    match cwd {
        Some(c) => fs::path::namei_from(c, path).await,
        None => fs::namei(path).await,
    }
}

async fn nameiparent_via_cwd(
    proc: &Arc<Proc>,
    path: &str,
) -> Option<(Arc<crate::fs::inode::Inode>, alloc::string::String)> {
    let cwd = proc.cwd.lock().clone();
    match cwd {
        Some(c) => fs::path::nameiparent_from(c, path).await,
        None => fs::nameiparent(path).await,
    }
}

async fn sys_chdir(proc: &Arc<Proc>, path_va: usize) -> i64 {
    let Some(path) = read_user_cstring(proc, path_va, 128) else {
        return -1;
    };
    let Some(ip) = resolve_path(proc, &path).await else {
        return -1;
    };
    {
        let li = fs::inode::ilock(&ip).await;
        if li.state().typ != xv6_fs_layout::T_DIR {
            return -1;
        }
    }
    *proc.cwd.lock() = Some(ip);
    0
}

/// Hard-link `new` to point at the same inode as `old`. Both paths
/// resolve through the proc's cwd. Refuses to link directories.
async fn sys_link(proc: &Arc<Proc>, old_va: usize, new_va: usize) -> i64 {
    let Some(old) = read_user_cstring(proc, old_va, 128) else {
        return -1;
    };
    let Some(new) = read_user_cstring(proc, new_va, 128) else {
        return -1;
    };
    let Some(ip) = resolve_path(proc, &old).await else {
        return -1;
    };
    fs::log::begin_op().await;
    let result = link_inside_op(proc, ip, &new).await;
    fs::log::end_op().await;
    result
}

async fn link_inside_op(
    proc: &Arc<Proc>,
    ip: Arc<crate::fs::inode::Inode>,
    new_path: &str,
) -> i64 {
    {
        let mut li = fs::inode::ilock(&ip).await;
        if li.state().typ == xv6_fs_layout::T_DIR {
            return -1;
        }
        li.state_mut().nlink += 1;
        fs::inode::iupdate(&li).await;
    }
    let Some((dir, name)) = nameiparent_via_cwd(proc, new_path).await else {
        // Undo the nlink bump we just did.
        let mut li = fs::inode::ilock(&ip).await;
        li.state_mut().nlink -= 1;
        fs::inode::iupdate(&li).await;
        return -1;
    };
    // Names must be in the same device (single device today, but the
    // check costs nothing).
    let dir_dev;
    let ip_dev;
    {
        let dli = fs::inode::ilock(&dir).await;
        dir_dev = dli.dev();
    }
    {
        let ili = fs::inode::ilock(&ip).await;
        ip_dev = ili.dev();
    }
    if dir_dev != ip_dev {
        let mut li = fs::inode::ilock(&ip).await;
        li.state_mut().nlink -= 1;
        fs::inode::iupdate(&li).await;
        return -1;
    }
    let inum = ip.inum.load(Ordering::Acquire) as u16;
    let ok = {
        let mut dli = fs::inode::ilock(&dir).await;
        crate::fs::dir::dirlink(&mut dli, &name, inum).await
    };
    if !ok {
        let mut li = fs::inode::ilock(&ip).await;
        li.state_mut().nlink -= 1;
        fs::inode::iupdate(&li).await;
        return -1;
    }
    0
}

async fn read_file_fully(proc: &Arc<Proc>, path: &str) -> Option<Vec<u8>> {
    let ip = resolve_path(proc, path).await?;
    let li = fs::inode::ilock(&ip).await;
    let size = li.state().size as usize;
    let mut buf = alloc::vec![0u8; size];
    let mut off: u32 = 0;
    while (off as usize) < size {
        let n = fs::inode::readi(&li, &mut buf[off as usize..], off).await;
        if n == 0 {
            break;
        }
        off += n as u32;
    }
    Some(buf)
}

const MAX_ARGS: usize = 16;
const MAX_ARG_LEN: usize = 128;

fn read_user_argv(
    proc: &Proc,
    argv_va: usize,
    max_args: usize,
    max_len: usize,
) -> Option<alloc::vec::Vec<String>> {
    if argv_va == 0 {
        return Some(alloc::vec::Vec::new());
    }
    let mut argv = alloc::vec::Vec::new();
    for i in 0..max_args {
        let kva = proc.translate_user(argv_va + i * 8)?;
        let ptr = unsafe { core::ptr::read_unaligned(kva as *const u64) } as usize;
        if ptr == 0 {
            return Some(argv);
        }
        let arg = read_user_cstring(proc, ptr, max_len)?;
        argv.push(arg);
    }
    None
}

// argv stack layout moved to `crate::user_vm::place_argv_on_stack`.

fn read_user_cstring(proc: &Proc, va: usize, max: usize) -> Option<String> {
    let mut s = String::new();
    for i in 0..max {
        let kva = proc.translate_user(va + i)?;
        let byte = unsafe { *(kva as *const u8) };
        if byte == 0 {
            return Some(s);
        }
        s.push(byte as char);
    }
    None
}

// User pagetable construction moved to `crate::user_vm::build_image_from_elf`.
