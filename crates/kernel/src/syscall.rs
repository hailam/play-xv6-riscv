//! Async syscall dispatch.

use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::future::Future;
use core::pin::Pin;
use core::sync::atomic::{AtomicU32, Ordering};
use core::task::{Context, Poll};

use hal::{FrameAllocator, Hal, PageTableOps, PtePerm};

use crate::arch::Arch;
use crate::cpu;
use crate::executor;
use crate::file::{File, PipeInner};
use crate::fs;
use crate::kalloc::KFRAMES;
use crate::proc::{Proc, ProcState};
use crate::user_vm::STACK_VA_BASE;
use crate::uapi::{
    Stat, O_CREATE, O_RDONLY, O_RDWR, O_TRUNC, O_WRONLY, SYS_CHDIR, SYS_CLOSE, SYS_DUP,
    SYS_EXEC, SYS_EXIT, SYS_FORK, SYS_FSTAT, SYS_GETPID, SYS_KILL, SYS_LINK, SYS_MKDIR,
    SYS_MKNOD, SYS_OPEN, SYS_PIPE, SYS_READ, SYS_SBRK, SYS_SLEEP, SYS_UNLINK, SYS_UPTIME,
    SYS_WAIT, SYS_WRITE,
};

#[cfg(target_arch = "riscv64")]
use hal_riscv64::{memlayout::PGSIZE, TrapFrame, TIMER_INTERVAL};

pub async fn dispatch(proc: &Arc<Proc>, nr: usize) -> i64 {
    match nr {
        SYS_FORK => sys_fork(proc).await,
        SYS_EXIT => {
            let code = proc.trapframe().a0 as i32;
            sys_exit_inner(proc, code).await
        }
        SYS_WAIT => sys_wait(proc).await,
        SYS_WRITE => {
            let tf = proc.trapframe();
            let fd = tf.a0 as i32;
            let buf_va = tf.a1 as usize;
            let len = tf.a2 as usize;
            sys_write(proc, fd, buf_va, len).await
        }
        SYS_READ => {
            let tf = proc.trapframe();
            let fd = tf.a0 as i32;
            let buf_va = tf.a1 as usize;
            let len = tf.a2 as usize;
            sys_read(proc, fd, buf_va, len).await
        }
        SYS_SLEEP => {
            let ticks = proc.trapframe().a0;
            sys_sleep(proc, ticks).await
        }
        SYS_EXEC => {
            let tf = proc.trapframe();
            let path_va = tf.a0 as usize;
            let argv_va = tf.a1 as usize;
            sys_exec(proc, path_va, argv_va).await
        }
        SYS_PIPE => {
            let pipefd_va = proc.trapframe().a0 as usize;
            sys_pipe(proc, pipefd_va).await
        }
        SYS_CLOSE => {
            let fd = proc.trapframe().a0 as i32;
            sys_close(proc, fd).await
        }
        SYS_DUP => {
            let fd = proc.trapframe().a0 as i32;
            sys_dup(proc, fd).await
        }
        SYS_OPEN => {
            let tf = proc.trapframe();
            let path_va = tf.a0 as usize;
            let flags = tf.a1 as u32;
            sys_open(proc, path_va, flags).await
        }
        SYS_FSTAT => {
            let tf = proc.trapframe();
            let fd = tf.a0 as i32;
            let stat_va = tf.a1 as usize;
            sys_fstat(proc, fd, stat_va).await
        }
        SYS_GETPID => proc.pid as i64,
        SYS_UPTIME => (Arch::now_ticks() / TIMER_INTERVAL) as i64,
        SYS_KILL => {
            let pid = proc.trapframe().a0 as i32;
            sys_kill(proc, pid).await
        }
        SYS_SBRK => {
            let n = proc.trapframe().a0 as i64;
            sys_sbrk(proc, n).await
        }
        SYS_MKDIR => {
            let path_va = proc.trapframe().a0 as usize;
            sys_mkdir(proc, path_va).await
        }
        SYS_MKNOD => {
            let tf = proc.trapframe();
            let path_va = tf.a0 as usize;
            let major = tf.a1 as u16;
            let minor = tf.a2 as u16;
            sys_mknod(proc, path_va, major, minor).await
        }
        SYS_UNLINK => {
            let path_va = proc.trapframe().a0 as usize;
            sys_unlink(proc, path_va).await
        }
        SYS_LINK | SYS_CHDIR => {
            // Both touched in a follow-up — `link` needs cross-dir
            // unlink semantics; `chdir` needs per-proc cwd.
            -1
        }
        _ => {
            crate::println!("syscall: unknown nr {}", nr);
            -1
        }
    }
}

/// Grow (or shrink) the user data segment by `n` bytes. Returns the
/// OLD break (matches xv6 / classic `sbrk` semantics). On error
/// returns `-1`.
///
/// Growth allocates fresh zero-filled frames and maps them URW
/// starting at `ceil(old_break / PGSIZE)`. Shrink is currently a
/// metadata-only update — pages aren't unmapped or freed. Reclaiming
/// pages on shrink is tracked in `pending/09-vm-reaping`.
async fn sys_sbrk(proc: &Arc<Proc>, n: i64) -> i64 {
    let old = proc.size.load(Ordering::Acquire);
    if n == 0 {
        return old as i64;
    }
    if n < 0 {
        let shrink = (-n) as usize;
        if shrink > old {
            return -1;
        }
        proc.size.store(old - shrink, Ordering::Release);
        return old as i64;
    }
    let new = match old.checked_add(n as usize) {
        Some(x) => x,
        None => return -1,
    };
    // Reserve one page of guard between the heap top and the user
    // stack so an overflowing malloc can't silently scribble on
    // argv / the trapframe.
    let heap_top = STACK_VA_BASE.saturating_sub(PGSIZE);
    if new > heap_top {
        return -1;
    }
    let start = (old + PGSIZE - 1) & !(PGSIZE - 1);
    let end = (new + PGSIZE - 1) & !(PGSIZE - 1);
    {
        let mut pt = proc.pagetable.lock();
        let mut va = start;
        while va < end {
            let Some(pa) = KFRAMES.alloc_zeroed() else {
                // Partial allocation leaks until vm-reaping lands.
                return -1;
            };
            if pt.map(va, pa, PGSIZE, PtePerm::URW, &KFRAMES).is_err() {
                return -1;
            }
            va += PGSIZE;
        }
    }
    proc.size.store(new, Ordering::Release);
    old as i64
}

async fn sys_kill(_proc: &Arc<Proc>, pid: i32) -> i64 {
    if pid <= 0 {
        return -1;
    }
    let Some(target) = executor::find_proc_by_pid(pid as usize) else {
        return -1;
    };
    target.killed.store(true, Ordering::Release);
    // Boot every blocking future this proc may be parked on. Each
    // poll checks `killed` and returns a cancellation sentinel.
    target.wait_waker.wake();
    crate::console_in::wake();
    executor::wake(target.task_id.load(Ordering::Relaxed));
    0
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

async fn sys_wait(proc: &Arc<Proc>) -> i64 {
    Wait { proc }.await
}

struct Wait<'a> {
    proc: &'a Arc<Proc>,
}

impl Future for Wait<'_> {
    type Output = i64;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<i64> {
        if self.proc.killed.load(Ordering::Acquire) {
            return Poll::Ready(-1);
        }
        self.proc.wait_waker.register(cx.waker());
        let mut children = self.proc.children.lock();
        let mut zombie_idx = None;
        for (i, c) in children.iter().enumerate() {
            if c.is_zombie() {
                zombie_idx = Some(i);
                break;
            }
        }
        if let Some(i) = zombie_idx {
            let dead = children.remove(i);
            let code = dead.exit_code.load(Ordering::Relaxed) as i64;
            return Poll::Ready(code);
        }
        if children.is_empty() {
            return Poll::Ready(-1);
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
    let Some(file) = proc.get_file(fd) else {
        return -1;
    };
    match &*file {
        File::Console => console_write(proc, buf_va, len),
        File::PipeWrite(p) => pipe_write(p.clone(), proc, buf_va, len).await,
        File::PipeRead(_) => -1,
        File::Inode {
            ip,
            off,
            readable: _,
            writable,
        } => {
            if !*writable {
                return -1;
            }
            inode_write(proc, ip.clone(), off, buf_va, len).await
        }
    }
}

async fn inode_write(
    proc: &Arc<Proc>,
    ip: Arc<crate::fs::inode::Inode>,
    off: &AtomicU32,
    buf_va: usize,
    len: usize,
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

    let cur = off.load(Ordering::Acquire);
    fs::log::begin_op().await;
    let mut li = fs::inode::ilock(&ip).await;
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
    let Some(file) = proc.get_file(fd) else {
        return -1;
    };
    match &*file {
        File::Console => console_read(proc, buf_va, len).await,
        File::PipeRead(p) => pipe_read(p.clone(), proc, buf_va, len).await,
        File::PipeWrite(_) => -1,
        File::Inode {
            ip,
            off,
            readable,
            writable: _,
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
    // Copy the bytes out into the user buffer, page-aware.
    let mut copied = 0usize;
    while copied < n {
        let Some(kva) = proc.translate_user(buf_va + copied) else {
            return -1;
        };
        unsafe { *(kva as *mut u8) = tmp[copied] };
        copied += 1;
    }
    off.store(cur + n as u32, Ordering::Release);
    n as i64
}

async fn console_read(proc: &Proc, buf_va: usize, len: usize) -> i64 {
    let mut n: usize = 0;
    while n < len {
        let Some(b) = ConsoleByte.await else {
            return -1; // killed
        };
        let Some(kva) = proc.translate_user(buf_va + n) else {
            return -1;
        };
        unsafe { *(kva as *mut u8) = b };
        n += 1;
        if b == b'\n' {
            break;
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
        let Some(kva) = proc.translate_user(va) else {
            return -1;
        };
        unsafe { *(kva as *mut i32) = *fd };
    }
    0
}

async fn sys_close(proc: &Arc<Proc>, fd: i32) -> i64 {
    proc.close_fd(fd)
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

async fn pipe_write(
    pipe: Arc<PipeInner>,
    proc: &Arc<Proc>,
    buf_va: usize,
    len: usize,
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
) -> i64 {
    let mut n: usize = 0;
    while n < len {
        match (PipeReadByte { pipe: pipe.clone() }).await {
            Some(b) => {
                let Some(kva) = proc.translate_user(buf_va + n) else {
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
    let allowed = O_RDONLY | O_WRONLY | O_RDWR | O_CREATE | O_TRUNC;
    if (flags & !allowed) != 0 {
        return -1;
    }

    let ip = if (flags & O_CREATE) != 0 {
        match create_at_path(&path, xv6_fs_layout::T_FILE, 0, 0).await {
            Some(i) => i,
            None => return -1,
        }
    } else {
        let Some(i) = fs::namei(&path).await else {
            return -1;
        };
        i
    };

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
        if (flags & O_TRUNC) != 0 && typ == xv6_fs_layout::T_FILE {
            fs::log::begin_op().await;
            fs::inode::itrunc(&mut li).await;
            // itrunc already called iupdate; just close the op.
            drop(li);
            fs::log::end_op().await;
        }
    }

    let readable = (flags & O_WRONLY) == 0;
    let writable = (flags & (O_WRONLY | O_RDWR)) != 0;
    let f = Arc::new(File::Inode {
        ip,
        off: AtomicU32::new(0),
        readable,
        writable,
    });
    proc.alloc_fd(f).map(|fd| fd as i64).unwrap_or(-1)
}

async fn sys_mkdir(proc: &Arc<Proc>, path_va: usize) -> i64 {
    let Some(path) = read_user_cstring(proc, path_va, 128) else {
        return -1;
    };
    match create_at_path(&path, xv6_fs_layout::T_DIR, 0, 0).await {
        Some(_) => 0,
        None => -1,
    }
}

async fn sys_mknod(proc: &Arc<Proc>, path_va: usize, major: u16, minor: u16) -> i64 {
    let Some(path) = read_user_cstring(proc, path_va, 128) else {
        return -1;
    };
    match create_at_path(&path, xv6_fs_layout::T_DEVICE, major, minor).await {
        Some(_) => 0,
        None => -1,
    }
}

async fn sys_unlink(proc: &Arc<Proc>, path_va: usize) -> i64 {
    let Some(path) = read_user_cstring(proc, path_va, 128) else {
        return -1;
    };
    let Some((dir, name)) = fs::nameiparent(&path).await else {
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
    path: &str,
    typ: u16,
    major: u16,
    minor: u16,
) -> Option<Arc<crate::fs::inode::Inode>> {
    let (dir, name) = fs::nameiparent(path).await?;
    if name.is_empty() || name == "." || name == ".." || name.len() > xv6_fs_layout::DIRSIZ
    {
        return None;
    }
    fs::log::begin_op().await;
    let result = create_inside_op(&dir, &name, typ, major, minor).await;
    fs::log::end_op().await;
    result
}

async fn create_inside_op(
    dir: &Arc<crate::fs::inode::Inode>,
    name: &str,
    typ: u16,
    major: u16,
    minor: u16,
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
    let child = fs::inode::ialloc(dev, typ).await?;
    {
        let mut child_li = fs::inode::ilock(&child).await;
        child_li.state_mut().major = major;
        child_li.state_mut().minor = minor;
        child_li.state_mut().nlink = 1;
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
    let (typ, nlink, size, inum, dev);
    {
        let li = fs::inode::ilock(ip).await;
        let s = li.state();
        typ = s.typ as i16;
        nlink = s.nlink as i16;
        size = s.size as u64;
        inum = li.inum();
        dev = li.dev() as i32;
    }
    let st = Stat {
        dev,
        ino: inum,
        typ,
        nlink,
        _pad: 0,
        size,
    };
    let bytes = unsafe {
        core::slice::from_raw_parts(
            &st as *const _ as *const u8,
            core::mem::size_of::<Stat>(),
        )
    };
    for (i, b) in bytes.iter().enumerate() {
        let Some(kva) = proc.translate_user(stat_va + i) else {
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
    let bin = match read_file_fully(&path).await {
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
    let tf = proc.trapframe();
    *tf = TrapFrame::default();
    tf.epc = entry as u64;
    tf.sp = sp_va as u64;
    tf.a1 = argv_array_va as u64;
    // `proc_main` writes our return value into `tf.a0` → becomes the
    // new image's `a0` (i.e., `argc`) when sret to user mode.
    argc
}

async fn read_file_fully(path: &str) -> Option<Vec<u8>> {
    let ip = fs::namei(path).await?;
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
