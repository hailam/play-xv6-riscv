//! Phase 4 syscall surface. Sync handlers — the async refactor lands in
//! Phase 4b together with the executor.

use crate::arch::{Arch, Hal};
use crate::proc::Proc;
use crate::uapi::{SYS_EXIT, SYS_WRITE};

/// Dispatch a syscall. Returns the value to put in `a0`.
pub fn dispatch(proc: &Proc, nr: usize) -> i64 {
    let tf = proc.trapframe();
    match nr {
        SYS_WRITE => {
            let fd = tf.a0 as i32;
            let buf_va = tf.a1 as usize;
            let len = tf.a2 as usize;
            sys_write(proc, fd, buf_va, len)
        }
        SYS_EXIT => {
            let code = tf.a0 as i32;
            sys_exit(proc, code)
        }
        _ => {
            crate::println!("syscall: unknown nr {}", nr);
            -1
        }
    }
}

fn sys_write(proc: &Proc, fd: i32, buf_va: usize, len: usize) -> i64 {
    if fd != 1 && fd != 2 {
        return -1;
    }
    let mut written: usize = 0;
    let mut va = buf_va;
    while written < len {
        let Some(kva) = proc.translate_user(va) else {
            return -1;
        };
        Arch::console_putc(unsafe { *(kva as *const u8) });
        va += 1;
        written += 1;
    }
    written as i64
}

fn sys_exit(_proc: &Proc, code: i32) -> i64 {
    crate::println!("init: exit({code})");
    // Phase 4: no scheduler / wait yet — just halt this hart's user loop
    // by tail-jumping to the "done" sink. We do that by returning a
    // sentinel; the caller checks for it.
    crate::usertrap::request_exit(code);
    0
}
