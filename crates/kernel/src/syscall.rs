//! Async syscall dispatch. Each `sys_*` is an `async fn` so it can `.await`
//! on I/O wakers (pipes, console, timers) once those land in later phases.

use alloc::sync::Arc;
use core::sync::atomic::Ordering;

use hal::Hal;

use crate::arch::Arch;
use crate::proc::{Proc, ProcState};
use crate::uapi::{SYS_EXIT, SYS_FORK, SYS_WRITE};

pub async fn dispatch(proc: &Arc<Proc>, nr: usize) -> i64 {
    match nr {
        SYS_FORK => sys_fork(proc).await,
        SYS_EXIT => {
            let code = proc.trapframe().a0 as i32;
            sys_exit(proc, code).await
        }
        SYS_WRITE => {
            let tf = proc.trapframe();
            let fd = tf.a0 as i32;
            let buf_va = tf.a1 as usize;
            let len = tf.a2 as usize;
            sys_write(proc, fd, buf_va, len).await
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
    crate::println!("pid {} exit({code})", proc.pid);
    0
}
