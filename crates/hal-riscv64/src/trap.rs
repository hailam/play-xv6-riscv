//! S-mode trap plumbing (kernel side). Phase 3 wires up `kernelvec`,
//! handles supervisor timer interrupts via Sstc, and panics on
//! everything else.

use crate::csr;

extern "C" {
    fn kernelvec();
}

const SCAUSE_INTERRUPT: usize = 1usize << 63;
const SCAUSE_TIMER: usize = 5;
#[allow(dead_code)]
const SCAUSE_EXTERNAL: usize = 9;
#[allow(dead_code)]
const SCAUSE_SOFTWARE: usize = 1;

/// Default tick interval, in `time` ticks. On QEMU virt this clock runs
/// at ~10 MHz, so 1_000_000 ticks ≈ 100 ms.
pub const TIMER_INTERVAL: u64 = 1_000_000;

/// Cause categories the kernel cares about; the Rust handler returns one
/// of these so the caller (kernel) can decide what to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrapCause {
    Timer,
    External,
    Software,
    Unknown(usize),
    Exception(usize),
}

/// Install the kernel trap vector on this hart and enable supervisor
/// interrupts. Safe wrapper.
pub unsafe fn init_kernel_trap_vec() {
    csr::write_stvec(kernelvec as *const () as usize);
}

/// Arm the next supervisor timer interrupt at `now + TIMER_INTERVAL`.
pub fn arm_timer() {
    let now = csr::read_time() as u64;
    unsafe { csr::write_stimecmp((now + TIMER_INTERVAL) as usize) };
}

/// Called by `kernelvec` (asm) for every S-mode trap. Reads cause, takes
/// action, restores sepc/sstatus so the asm shim can `sret` cleanly.
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
    if scause & SCAUSE_INTERRUPT != 0 {
        let code = scause & !SCAUSE_INTERRUPT;
        match code {
            SCAUSE_TIMER => {
                // Disarm by pushing stimecmp far into the future; the
                // kernel decides when (and whether) to re-arm.
                unsafe { csr::write_stimecmp(usize::MAX) };
                crate::trap_hook::on_timer();
                true
            }
            _ => false,
        }
    } else {
        false
    }
}
