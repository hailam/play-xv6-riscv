//! Async syscall dispatch.

use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::future::Future;
use core::pin::Pin;
use core::sync::atomic::{AtomicU32, Ordering};
use core::task::{Context, Poll};

use hal::Hal;

use crate::arch::Arch;
use crate::file::{File, PipeInner};
use crate::fs;
use crate::proc::{Proc, ProcState};
use crate::uapi::{
    Stat, O_RDONLY, O_RDWR, O_WRONLY, SYS_CHDIR, SYS_CLOSE, SYS_DUP, SYS_EXEC, SYS_EXIT,
    SYS_FORK, SYS_FSTAT, SYS_GETPID, SYS_KILL, SYS_LINK, SYS_MKDIR, SYS_MKNOD, SYS_OPEN,
    SYS_PIPE, SYS_READ, SYS_SBRK, SYS_SLEEP, SYS_UNLINK, SYS_UPTIME, SYS_WAIT, SYS_WRITE,
};

#[cfg(target_arch = "riscv64")]
use hal_riscv64::{TrapFrame, TIMER_INTERVAL};

pub async fn dispatch(proc: &Arc<Proc>, nr: usize) -> i64 {
    match nr {
        SYS_FORK => sys_fork(proc).await,
        SYS_EXIT => {
            let code = proc.trapframe().a0 as i32;
            sys_exit(proc, code).await
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
        SYS_KILL => -1, // pending/07-sys-kill-cancellation
        SYS_SBRK => -1, // pending/08-sbrk-and-malloc
        SYS_CHDIR | SYS_MKDIR | SYS_MKNOD | SYS_UNLINK | SYS_LINK => {
            // Writes through the log not implemented yet; tracked in
            // pending/13-fs-writes.
            -1
        }
        _ => {
            crate::println!("syscall: unknown nr {}", nr);
            -1
        }
    }
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

async fn sys_exit(proc: &Arc<Proc>, code: i32) -> i64 {
    // Drop all fds so pipe end counts decrement now (rather than
    // waiting for the Proc itself to be reclaimed, which never happens
    // in Phase 5x).
    proc.files.lock().clear();

    proc.exit_code.store(code, Ordering::Relaxed);
    proc.state.store(ProcState::Zombie as i32, Ordering::Release);
    let parent_weak = proc.parent.lock().clone();
    if let Some(p) = parent_weak.and_then(|w| w.upgrade()) {
        p.wait_waker.wake();
    }
    crate::println!("pid {} exit({code})", proc.pid);
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
        // Disk writes are not implemented yet (writei lives in
        // pending/13-fs-writes). Reject so user code sees an error
        // instead of a silent no-op.
        File::Inode { .. } => -1,
    }
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
        let b = ConsoleByte.await;
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
    type Output = u8;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<u8> {
        crate::console_in::register_waker(cx.waker());
        if let Some(b) = crate::console_in::try_pop() {
            return Poll::Ready(b);
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
        let Some(kva) = proc.translate_user(buf_va + n) else {
            return -1;
        };
        let byte = unsafe { *(kva as *const u8) };
        PipeWriteByte {
            pipe: pipe.clone(),
            byte,
        }
        .await;
        n += 1;
    }
    n as i64
}

struct PipeWriteByte {
    pipe: Arc<PipeInner>,
    byte: u8,
}

impl Future for PipeWriteByte {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        self.pipe.write_waker.register(cx.waker());
        let mut buf = self.pipe.buf.lock();
        if buf.len() < self.pipe.cap() {
            buf.push_back(self.byte);
            drop(buf);
            self.pipe.read_waker.wake();
            return Poll::Ready(());
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
    // O_CREATE / O_TRUNC need writei — not yet supported.
    if (flags & !(O_RDONLY | O_WRONLY | O_RDWR)) != 0 {
        return -1;
    }
    let Some(ip) = fs::namei(&path).await else {
        return -1;
    };
    // Sanity-check the inode by ilocking it once (forces load).
    {
        let li = fs::inode::ilock(&ip).await;
        if li.state().typ == 0 {
            return -1;
        }
    }
    let readable = (flags & O_WRONLY) == 0; // RDONLY (=0) or RDWR
    let writable = (flags & (O_WRONLY | O_RDWR)) != 0;
    let f = Arc::new(File::Inode {
        ip,
        off: AtomicU32::new(0),
        readable,
        writable,
    });
    proc.alloc_fd(f).map(|fd| fd as i64).unwrap_or(-1)
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
