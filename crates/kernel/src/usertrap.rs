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

    // POSIX signal dispatch hook. If there's a pending unblocked
    // signal with a user handler installed, divert the upcoming
    // user-mode return: save the live trapframe to `proc.sig_saved_frame`,
    // rewrite epc/sp/a0/ra so eret lands in the handler.
    maybe_deliver_signal(proc);

    let hartid = Arch::hartid();
    let tf = proc.trapframe();
    tf.set_kernel_satp(crate::vm::kernel_satp() as u64);
    tf.set_kernel_sp(kernel_stack_top(hartid) as u64);
    tf.set_kernel_trap(rust_usertrap as *const () as u64);
    tf.set_kernel_hartid(hartid as u64);

    let user_satp = proc.satp();
    unsafe { Arch::return_to_user(tf, user_satp) }
}

/// Pick the lowest-numbered pending, unblocked signal whose handler
/// is non-default/non-ignored. Atomically dequeue it (clear the
/// pending bit), snapshot the trapframe into `proc.sig_saved_frame`,
/// then rewrite the trapframe so the next eret lands in the
/// handler with:
///   * arg0 = signum
///   * pc   = handler VA
///   * ra   = restorer VA  (so `ret` from the handler hits sigreturn)
///
/// If a signal is already in flight (`sig_saved_frame.is_some()`) we
/// skip — no nesting.
fn maybe_deliver_signal(proc: &Proc) {
    use crate::uapi::{SIG_DFL, SIG_IGN};
    if proc.sig_saved_frame.lock().is_some() {
        return;
    }
    let blocked = proc.sig_blocked.load(core::sync::atomic::Ordering::Acquire);
    let pending = proc.sig_pending.load(core::sync::atomic::Ordering::Acquire);
    let runnable = pending & !blocked;
    if runnable == 0 {
        return;
    }
    let sig = runnable.trailing_zeros() as i32;
    let action = proc.sig_actions.lock()[sig as usize];
    if action.handler == SIG_DFL || action.handler == SIG_IGN {
        // Disposition changed between sys_kill queuing and now —
        // just clear the pending bit and continue (no delivery).
        proc.sig_pending.fetch_and(!(1u32 << sig), core::sync::atomic::Ordering::AcqRel);
        return;
    }
    if action.restorer == 0 {
        // No restorer installed — user code would never return
        // from the handler. Refuse to deliver; this keeps the proc
        // from disappearing into a never-returning handler.
        return;
    }
    // Dequeue the bit before any side effect that might wake the
    // proc again (so we don't double-deliver).
    proc.sig_pending.fetch_and(!(1u32 << sig), core::sync::atomic::Ordering::AcqRel);
    let tf = proc.trapframe();
    *proc.sig_saved_frame.lock() = Some(*tf);
    // Rewrite the trapframe for the handler invocation. arg0 = sig,
    // pc = handler, return address = restorer. sp untouched — the
    // handler runs on the same user stack (no separate signal stack).
    tf.set_arg(0, sig as u64);
    tf.set_epc(action.handler as u64);
    set_return_addr(tf, action.restorer);
}

#[cfg(target_arch = "riscv64")]
fn set_return_addr(tf: &mut <Arch as Hal>::TrapFrame, va: usize) {
    // riscv: link register is ra (x1) — there's no
    // `TrapFrameAccess::set_ra` so we go through the concrete type.
    use hal_riscv64::trapframe::TrapFrame;
    let rv_tf: &mut TrapFrame = unsafe { &mut *(tf as *mut _ as *mut TrapFrame) };
    rv_tf.ra = va as u64;
}

#[cfg(target_arch = "aarch64")]
fn set_return_addr(tf: &mut <Arch as Hal>::TrapFrame, va: usize) {
    // aarch64: link register is x30. The TrapFrame stores all GPRs
    // in a `[u64; 31]` (x0..x30), so x30 is index 30.
    use hal_aarch64::trapframe::TrapFrame;
    let a64_tf: &mut TrapFrame = unsafe { &mut *(tf as *mut _ as *mut TrapFrame) };
    a64_tf.x[30] = va as u64;
}
