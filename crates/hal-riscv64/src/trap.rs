//! S-mode trap plumbing (kernel side).

use crate::csr;

extern "C" {
    fn kernelvec();
}

const SCAUSE_INTERRUPT: usize = 1usize << 63;
const SCAUSE_TIMER: usize = 5;
const SCAUSE_EXTERNAL: usize = 9;

pub const TIMER_INTERVAL: u64 = 1_000_000;

pub unsafe fn init_kernel_trap_vec() {
    csr::write_stvec(kernelvec as *const () as usize);
}

pub fn arm_timer() {
    let now = csr::read_time() as u64;
    unsafe { csr::write_stimecmp((now + TIMER_INTERVAL) as usize) };
}

#[no_mangle]
pub extern "C" fn rust_kerneltrap() {
    let sepc = csr::read_sepc();
    let sstatus = csr::read_sstatus();
    let scause = csr::read_scause();

    if !decode_and_handle(scause) {
        let stval = csr::read_stval();
        panic!(
            "kerneltrap: scause={:#x} sepc={:#x} stval={:#x}",
            scause, sepc, stval
        );
    }

    unsafe {
        csr::write_sepc(sepc);
        csr::write_sstatus(sstatus);
    }
}

fn decode_and_handle(scause: usize) -> bool {
    if scause & SCAUSE_INTERRUPT == 0 {
        return false;
    }
    let code = scause & !SCAUSE_INTERRUPT;
    match code {
        SCAUSE_TIMER => {
            unsafe { csr::write_stimecmp(usize::MAX) };
            crate::trap_hook::on_timer();
            true
        }
        SCAUSE_EXTERNAL => {
            handle_external();
            true
        }
        _ => false,
    }
}

fn handle_external() {
    let src = crate::plic::claim();
    if src != 0 {
        crate::trap_hook::on_external(src);
        crate::plic::complete(src);
    }
}

/// Same logic as the kernel-trap path; exposed so the user-trap path
/// can reuse it when scause is SCAUSE_EXTERNAL.
pub fn handle_external_irq() {
    handle_external();
}
