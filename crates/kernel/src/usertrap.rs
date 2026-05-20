//! Trap-from-user dispatch + the noreturn user-mode entry path.

use hal::{Hal, TrapFrameAccess};

use crate::arch::Arch;
use crate::cpu;
use crate::executor;
use crate::proc::{Proc, TrapEvent};

use crate::arch::{userret_offset, uservec_offset, TRAMPOLINE};

// CSR / SSTATUS helpers are RISC-V-specific.
#[cfg(target_arch = "riscv64")]
use hal_riscv64::csr_api::{
    read_scause, read_sepc, read_sstatus, read_stval, write_sepc, write_sstatus,
    write_stvec, SSTATUS_SPIE, SSTATUS_SPP,
};

const SCAUSE_ECALL_FROM_U: usize = 8;
const SCAUSE_LOAD_PAGE_FAULT: usize = 13;
const SCAUSE_STORE_PAGE_FAULT: usize = 15;
const SCAUSE_INTERRUPT: usize = 1usize << 63;
const SCAUSE_TIMER: usize = 5;
const SCAUSE_EXTERNAL: usize = 9;

extern "C" {
    static _stack0: u8;
    fn kernelvec();
}

const STACK_PER_HART: usize = 16 * 1024;

fn kernel_stack_top(hartid: usize) -> usize {
    let base = unsafe { &_stack0 as *const u8 as usize };
    base + (hartid + 1) * STACK_PER_HART
}

#[no_mangle]
pub extern "C" fn rust_usertrap() -> ! {
    unsafe { write_stvec(kernelvec as *const () as usize) };

    let proc = cpu::current_proc().expect("rust_usertrap: no current proc");
    let tf = proc.trapframe();
    tf.set_epc(read_sepc() as u64);

    let scause = read_scause();
    let event = if scause == SCAUSE_ECALL_FROM_U {
        tf.set_epc(tf.epc() + 4);
        TrapEvent::Syscall {
            nr: tf.syscall_nr() as usize,
        }
    } else if scause & SCAUSE_INTERRUPT != 0 {
        let code = scause & !SCAUSE_INTERRUPT;
        match code {
            SCAUSE_TIMER => {
                crate::trap::TICKS
                    .fetch_add(1, core::sync::atomic::Ordering::Relaxed);
                crate::time::drain_expired();
                hal_riscv64::arm_timer();
                TrapEvent::Timer
            }
            SCAUSE_EXTERNAL => {
                hal_riscv64::handle_external_irq();
                TrapEvent::Devintr
            }
            _ => panic!("usertrap: unknown intr code {code}"),
        }
    } else {
        let stval = read_stval();

        // First: is this a lazy-sbrk-allocated page? If so, map it
        // and resume the trapping instruction by NOT advancing epc.
        if (scause == SCAUSE_LOAD_PAGE_FAULT || scause == SCAUSE_STORE_PAGE_FAULT)
            && {
                // Reconstruct an Arc so `lazy_map_page` gets the same
                // `&Arc<Proc>` shape the syscall layer uses.
                let proc_arc = unsafe {
                    let raw = proc as *const Proc;
                    alloc::sync::Arc::increment_strong_count(raw);
                    alloc::sync::Arc::from_raw(raw)
                };
                let ok = crate::syscall::lazy_map_page(&proc_arc, stval as usize);
                // `proc_arc` drops here, restoring the strong count.
                ok
            }
        {
            TrapEvent::Timer
        } else {
            // Unknown / illegal fault. xv6 prints a diagnostic and
            // calls `setkilled(p)`; match that, no panic.
            crate::println!(
                "usertrap: pid {} scause={:#x} sepc={:#x} stval={:#x} -> killed",
                proc.pid,
                scause,
                tf.epc(),
                stval,
            );
            proc.killed.store(true, core::sync::atomic::Ordering::Release);
            TrapEvent::Timer
        }
    };

    *proc.pending_trap.lock() = Some(event);
    let tid = proc.task_id.load(core::sync::atomic::Ordering::Relaxed);
    executor::wake(tid);

    executor::run()
}

pub fn return_to_user(proc: &Proc) -> ! {
    let hartid = Arch::hartid();

    let tf = proc.trapframe();
    tf.set_kernel_satp(crate::vm::kernel_satp() as u64);
    tf.set_kernel_sp(kernel_stack_top(hartid) as u64);
    tf.set_kernel_trap(rust_usertrap as *const () as u64);
    tf.set_kernel_hartid(hartid as u64);

    let mut sstatus = read_sstatus();
    sstatus &= !SSTATUS_SPP;
    sstatus |= SSTATUS_SPIE;
    unsafe { write_sstatus(sstatus) };
    unsafe { write_sepc(tf.epc() as usize) };

    let uservec_va = TRAMPOLINE + uservec_offset();
    unsafe { write_stvec(uservec_va) };

    let user_satp = proc.satp();
    let userret_va = TRAMPOLINE + userret_offset();
    let userret_fn: extern "C" fn(usize) -> ! =
        unsafe { core::mem::transmute(userret_va) };
    userret_fn(user_satp);
}
