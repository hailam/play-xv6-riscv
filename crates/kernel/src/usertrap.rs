//! Trap-from-user handling and the user-mode return path. Sync for now;
//! becomes async in Phase 4b.

use core::sync::atomic::{AtomicBool, AtomicI32, Ordering};

use hal::Hal;

use crate::arch::Arch;
use crate::cpu;
use crate::proc::Proc;
use crate::syscall;

#[cfg(target_arch = "riscv64")]
use hal_riscv64::{
    csr_api::{
        read_scause, read_sepc, read_sstatus, read_stval, write_sepc, write_sstatus,
        write_stvec, SSTATUS_SPIE, SSTATUS_SPP,
    },
    memlayout::TRAMPOLINE,
    trampoline_pa, userret_offset, uservec_offset,
};

const SCAUSE_ECALL_FROM_U: usize = 8;
const SCAUSE_INTERRUPT: usize = 1usize << 63;
const SCAUSE_TIMER: usize = 5;

extern "C" {
    static _stack0: u8;
    fn kernelvec();
}

const STACK_PER_HART: usize = 16 * 1024;

fn kernel_stack_top(hartid: usize) -> usize {
    let base = unsafe { &_stack0 as *const u8 as usize };
    base + (hartid + 1) * STACK_PER_HART
}

/// Set by `sys_exit`. The user loop polls and unwinds out of the
/// trap-and-resume cycle when set.
static EXIT_REQUESTED: AtomicBool = AtomicBool::new(false);
static EXIT_CODE: AtomicI32 = AtomicI32::new(0);

pub fn request_exit(code: i32) {
    EXIT_CODE.store(code, Ordering::Relaxed);
    EXIT_REQUESTED.store(true, Ordering::Release);
}

/// Run the given proc as the current CPU's user task. Loops:
///   return-to-user -> user runs -> trap -> handler -> back.
/// Returns when `sys_exit` requests it.
pub fn run_user(proc: &'static Proc) -> i32 {
    cpu::set_current_proc(proc as *const _ as *mut _);
    loop {
        if EXIT_REQUESTED.load(Ordering::Acquire) {
            return EXIT_CODE.load(Ordering::Relaxed);
        }
        return_to_user(proc);
    }
}

/// Called from `kernelvec`-equivalent trampoline path via `rust_usertrap`.
/// Handles the trap then re-enters the user loop. Doesn't return — it
/// loops back via `return_to_user`.
#[no_mangle]
pub extern "C" fn rust_usertrap() -> ! {
    // Switch stvec to kernelvec so any further trap from the kernel path
    // goes through the kernel trap entry.
    unsafe { write_stvec(kernelvec as *const () as usize) };

    let proc = cpu::current_proc().expect("rust_usertrap: no current proc");
    let tf = proc.trapframe();

    // Save user PC into trapframe so we can restore it after handling.
    tf.epc = read_sepc() as u64;

    let scause = read_scause();
    if scause == SCAUSE_ECALL_FROM_U {
        // ecall from U-mode: skip past the ecall.
        tf.epc += 4;
        let nr = tf.a7 as usize;
        // Interrupts on during the syscall body — except for spinlock
        // critical sections, which push_off internally.
        unsafe { Arch::intr_on() };
        let ret = syscall::dispatch(proc, nr);
        unsafe { Arch::intr_off() };
        tf.a0 = ret as u64;
    } else if scause & SCAUSE_INTERRUPT != 0 {
        let code = scause & !SCAUSE_INTERRUPT;
        match code {
            SCAUSE_TIMER => {
                // Disarm & invoke kernel timer hook.
                hal_riscv64::arm_timer();
                crate::trap::kernel_on_timer();
            }
            _ => panic!("usertrap: unknown intr code {code}"),
        }
    } else {
        let stval = read_stval();
        panic!(
            "usertrap: unhandled scause={:#x} sepc={:#x} stval={:#x}",
            scause, tf.epc, stval
        );
    }

    if EXIT_REQUESTED.load(Ordering::Acquire) {
        // Tail-return into run_user's loop, which will observe the flag
        // and exit. Restoring sp to the per-hart stack top gives us a
        // clean frame to "return" from.
        let sp_top = kernel_stack_top(Arch::hartid());
        unsafe {
            core::arch::asm!(
                "mv sp, {0}",
                "jal x0, {sink}",
                in(reg) sp_top,
                sink = sym user_exit_sink,
                options(noreturn),
            );
        }
    }

    return_to_user(proc)
}

#[no_mangle]
extern "C" fn user_exit_sink() -> ! {
    // After exit, just park this hart. Hart 0's run_user check happens
    // before the next return_to_user, but we never re-enter it because
    // sp has been reset and we tail-jumped here.
    loop {
        unsafe { Arch::wfi() }
    }
}

fn return_to_user(proc: &Proc) -> ! {
    let hartid = Arch::hartid();

    // Update trapframe with this CPU's kernel context.
    let tf = proc.trapframe();
    tf.kernel_satp = crate::vm::kernel_satp() as u64;
    tf.kernel_sp = kernel_stack_top(hartid) as u64;
    tf.kernel_trap = rust_usertrap as *const () as u64;
    tf.kernel_hartid = hartid as u64;

    // sstatus: SPP=0 (return to U-mode), SPIE=1 (re-enable interrupts after sret).
    let mut sstatus = read_sstatus();
    sstatus &= !SSTATUS_SPP;
    sstatus |= SSTATUS_SPIE;
    unsafe { write_sstatus(sstatus) };

    // sepc = user PC (saved in trapframe).
    unsafe { write_sepc(tf.epc as usize) };

    // Switch stvec to point at uservec (in the trampoline page).
    let uservec_va = TRAMPOLINE + uservec_offset();
    unsafe { write_stvec(uservec_va) };

    // Compute user satp.
    let user_satp = <Arch as Hal>::pagetable_satp(&proc.pagetable);

    // Jump into the trampoline at userret. Tail-call.
    let userret_va = TRAMPOLINE + userret_offset();
    let userret_fn: extern "C" fn(usize) -> ! = unsafe {
        core::mem::transmute(userret_va)
    };
    userret_fn(user_satp);
}
