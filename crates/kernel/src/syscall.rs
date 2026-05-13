//! Async syscall dispatch.

use alloc::sync::Arc;
use core::future::Future;
use core::pin::Pin;
use core::sync::atomic::Ordering;
use core::task::{Context, Poll};

use hal::Hal;

use crate::arch::Arch;
use crate::proc::{Proc, ProcState};
use crate::uapi::{SYS_EXIT, SYS_FORK, SYS_SLEEP, SYS_WAIT, SYS_WRITE};

#[cfg(target_arch = "riscv64")]
use hal_riscv64::TIMER_INTERVAL;

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
        SYS_SLEEP => {
            let ticks = proc.trapframe().a0;
            sys_sleep(proc, ticks).await
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

async fn sys_exit(proc: &Arc<Proc>, code: i32) -> i64 {
    proc.exit_code.store(code, Ordering::Relaxed);
    proc.state.store(ProcState::Zombie as i32, Ordering::Release);
    // Notify parent (if any) so a waiting parent gets re-polled.
    let parent_weak = proc.parent.lock().clone();
    if let Some(p) = parent_weak.and_then(|w| w.upgrade()) {
        p.wait_waker.wake();
    }
    crate::println!("pid {} exit({code})", proc.pid);
    0
}

// ---------- async I/O futures ------------------------------------------------

async fn sys_wait(proc: &Arc<Proc>) -> i64 {
    Wait { proc }.await
}

struct Wait<'a> {
    proc: &'a Arc<Proc>,
}

impl Future for Wait<'_> {
    type Output = i64;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<i64> {
        // Park first to avoid losing wakes that arrive between scanning
        // children and the next poll.
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
