//! Async syscall dispatch.

use alloc::string::String;
use alloc::sync::Arc;
use core::future::Future;
use core::pin::Pin;
use core::sync::atomic::Ordering;
use core::task::{Context, Poll};

use hal::{FrameAllocator, Hal, PageTableOps, PtePerm};

use crate::arch::Arch;
use crate::kalloc::KFRAMES;
use crate::proc::{Proc, ProcState};
use crate::uapi::{SYS_EXEC, SYS_EXIT, SYS_FORK, SYS_READ, SYS_SLEEP, SYS_WAIT, SYS_WRITE};

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

async fn sys_write(proc: &Arc<Proc>, fd: i32, buf_va: usize, len: usize) -> i64 {
    if fd != 1 && fd != 2 {
        return -1;
    }
    let mut written: usize = 0;
    let mut va = buf_va;
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
    if fd != 0 || len == 0 {
        return -1;
    }
    let mut n: usize = 0;
    while n < len {
        let b = ConsoleRead.await;
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

struct ConsoleRead;

impl Future for ConsoleRead {
    type Output = u8;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<u8> {
        crate::console_in::register_waker(cx.waker());
        if let Some(b) = crate::console_in::try_pop() {
            return Poll::Ready(b);
        }
        Poll::Pending
    }
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
    assert!(bin.len() < PGSIZE, "exec: binary too big for Phase 5d");
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
