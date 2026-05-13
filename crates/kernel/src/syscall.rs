//! Async syscall dispatch.

use alloc::string::String;
use alloc::sync::Arc;
use core::future::Future;
use core::pin::Pin;
use core::sync::atomic::Ordering;
use core::task::{Context, Poll};

use hal::{FrameAllocator, Hal, PageTableOps, PtePerm};

use crate::arch::Arch;
use crate::file::{File, PipeInner};
use crate::kalloc::KFRAMES;
use crate::proc::{Proc, ProcState};
use crate::uapi::{
    SYS_CLOSE, SYS_EXEC, SYS_EXIT, SYS_FORK, SYS_PIPE, SYS_READ, SYS_SLEEP, SYS_WAIT, SYS_WRITE,
};

#[cfg(target_arch = "riscv64")]
use hal_riscv64::{
    memlayout::{PGSIZE, TRAMPOLINE, TRAPFRAME},
    trampoline_pa, TrapFrame, TIMER_INTERVAL,
};

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
            let path_va = proc.trapframe().a0 as usize;
            sys_exec(proc, path_va).await
        }
        SYS_PIPE => {
            let pipefd_va = proc.trapframe().a0 as usize;
            sys_pipe(proc, pipefd_va).await
        }
        SYS_CLOSE => {
            let fd = proc.trapframe().a0 as i32;
            sys_close(proc, fd).await
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
    crate::proc::spawn_proc_main(child_arc);
    child_pid
}

async fn sys_exit(proc: &Arc<Proc>, code: i32) -> i64 {
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
    }
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

    // Write rfd, wfd back to user `pipefd[2]` (2 i32s = 8 bytes).
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
        let b = PipeReadByte { pipe: pipe.clone() }.await;
        let Some(kva) = proc.translate_user(buf_va + n) else {
            return -1;
        };
        unsafe { *(kva as *mut u8) = b };
        n += 1;
        // Drain whatever is available without blocking again — but if
        // the ring is empty, that's enough; let the caller call us
        // again for more bytes.
        if pipe.buf.lock().is_empty() {
            break;
        }
    }
    n as i64
}

struct PipeReadByte {
    pipe: Arc<PipeInner>,
}

impl Future for PipeReadByte {
    type Output = u8;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<u8> {
        self.pipe.read_waker.register(cx.waker());
        let popped = self.pipe.buf.lock().pop_front();
        if let Some(b) = popped {
            self.pipe.write_waker.wake();
            return Poll::Ready(b);
        }
        Poll::Pending
    }
}

// ---------- sys_exec --------------------------------------------------------

async fn sys_exec(proc: &Arc<Proc>, path_va: usize) -> i64 {
    let Some(path) = read_user_cstring(proc, path_va, 64) else {
        return -1;
    };
    let Some(bin) = crate::embed::find(&path) else {
        crate::println!("exec: no such program: {}", path);
        return -1;
    };
    let Ok(new_pt) = build_user_pagetable(bin, proc.trapframe_pa) else {
        return -1;
    };
    proc.replace_image(new_pt, PGSIZE);
    let tf = proc.trapframe();
    *tf = TrapFrame::default();
    tf.epc = 0;
    tf.sp = PGSIZE as u64;
    0
}

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

fn build_user_pagetable(
    bin: &[u8],
    trapframe_pa: usize,
) -> Result<<Arch as Hal>::PageTable, ()> {
    assert!(bin.len() < PGSIZE, "exec: binary too big for Phase 5e");
    let mut pt = <Arch as Hal>::PageTable::new(&KFRAMES).map_err(|_| ())?;
    pt.map(TRAMPOLINE, trampoline_pa(), PGSIZE, PtePerm::RX, &KFRAMES)
        .map_err(|_| ())?;
    pt.map(TRAPFRAME, trapframe_pa, PGSIZE, PtePerm::RW, &KFRAMES)
        .map_err(|_| ())?;
    let upage_pa = KFRAMES.alloc_zeroed().ok_or(())?;
    unsafe {
        core::ptr::copy_nonoverlapping(bin.as_ptr(), upage_pa as *mut u8, bin.len());
    }
    pt.map(0, upage_pa, PGSIZE, PtePerm::URWX, &KFRAMES)
        .map_err(|_| ())?;
    Ok(pt)
}
