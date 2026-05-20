//! Trap-from-user dispatch + the noreturn user-mode entry path.
//!
//! Arch-independent: every CSR / cause-register read goes through
//! the `Hal::decode_user_trap` and `Hal::return_to_user` surface.

use hal::{Hal, TrapFrameAccess, UserTrapCause};

use crate::arch::Arch;
use crate::cpu;
use crate::executor;
use crate::proc::{Proc, TrapEvent};

extern "C" {
    static _stack0: u8;
}

const STACK_PER_HART: usize = 16 * 1024;

fn kernel_stack_top(hartid: usize) -> usize {
    let base = unsafe { &_stack0 as *const u8 as usize };
    base + (hartid + 1) * STACK_PER_HART
}

#[no_mangle]
pub extern "C" fn rust_usertrap() -> ! {
    unsafe { Arch::on_user_trap_entry() };

    let proc = cpu::current_proc().expect("rust_usertrap: no current proc");
    let tf = proc.trapframe();

    let cause = Arch::decode_user_trap(tf);

    let event = match cause {
        UserTrapCause::Syscall => TrapEvent::Syscall {
            nr: tf.syscall_nr() as usize,
        },
        UserTrapCause::Timer => {
            crate::trap::TICKS
                .fetch_add(1, core::sync::atomic::Ordering::Relaxed);
            crate::time::drain_expired();
            Arch::arm_timer();
            TrapEvent::Timer
        }
        UserTrapCause::Devintr => {
            Arch::handle_external_irq();
            TrapEvent::Devintr
        }
        UserTrapCause::PageFault { va, write: _ } => {
            // Lazy-sbrk region? Map a fresh zero frame and re-run
            // the trapping instruction (don't advance epc).
            let proc_arc = unsafe {
                let raw = proc as *const Proc;
                alloc::sync::Arc::increment_strong_count(raw);
                alloc::sync::Arc::from_raw(raw)
            };
            let mapped = crate::syscall::lazy_map_page(&proc_arc, va);
            if mapped {
                TrapEvent::Timer
            } else {
                crate::println!(
                    "usertrap: pid {} page fault va={:#x} epc={:#x} -> killed",
                    proc.pid,
                    va,
                    tf.epc(),
                );
                proc.killed
                    .store(true, core::sync::atomic::Ordering::Release);
                TrapEvent::Timer
            }
        }
        UserTrapCause::Unknown { code, va } => {
            crate::println!(
                "usertrap: pid {} unknown cause={:#x} va={:#x} epc={:#x} -> killed",
                proc.pid,
                code,
                va,
                tf.epc(),
            );
            proc.killed
                .store(true, core::sync::atomic::Ordering::Release);
            TrapEvent::Timer
        }
    };

    *proc.pending_trap.lock() = Some(event);
    let tid = proc.task_id.load(core::sync::atomic::Ordering::Relaxed);
    executor::wake(tid);

    executor::run()
}

/// Set up the proc's trapframe for a return-to-user and jump
/// through the trampoline. Noreturn.
pub fn return_to_user(proc: &Proc) -> ! {
    // CRITICAL: interrupts must be off across the trap-vector swap
    // until `sret`/`eret` re-enables them. Without this, a timer
    // firing in the window between vector swap and the actual
    // return would dispatch via the user-trap path from kernel
    // mode, corrupting the trapframe.
    unsafe { Arch::intr_off() };

    let hartid = Arch::hartid();
    let tf = proc.trapframe();
    tf.set_kernel_satp(crate::vm::kernel_satp() as u64);
    tf.set_kernel_sp(kernel_stack_top(hartid) as u64);
    tf.set_kernel_trap(rust_usertrap as *const () as u64);
    tf.set_kernel_hartid(hartid as u64);

    let user_satp = proc.satp();
    unsafe { Arch::return_to_user(tf, user_satp) }
}
