//! RISC-V CSR access primitives.

use core::arch::asm;

macro_rules! read_csr {
    ($fn:ident, $csr:ident) => {
        #[inline(always)]
        #[allow(dead_code)]
        pub fn $fn() -> usize {
            let v: usize;
            unsafe {
                asm!(concat!("csrr {}, ", stringify!($csr)),
                     out(reg) v,
                     options(nomem, nostack, preserves_flags));
            }
            v
        }
    };
}

macro_rules! write_csr {
    ($fn:ident, $csr:ident) => {
        #[inline(always)]
        #[allow(dead_code)]
        pub unsafe fn $fn(v: usize) {
            asm!(concat!("csrw ", stringify!($csr), ", {}"),
                 in(reg) v,
                 options(nomem, nostack, preserves_flags));
        }
    };
}

// Numeric-CSR variants for non-standard / new CSRs that the assembler
// may not know by name.
macro_rules! read_csr_num {
    ($fn:ident, $csr_num:literal) => {
        #[inline(always)]
        #[allow(dead_code)]
        pub fn $fn() -> usize {
            let v: usize;
            unsafe {
                asm!(concat!("csrr {}, ", stringify!($csr_num)),
                     out(reg) v,
                     options(nomem, nostack, preserves_flags));
            }
            v
        }
    };
}

macro_rules! write_csr_num {
    ($fn:ident, $csr_num:literal) => {
        #[inline(always)]
        #[allow(dead_code)]
        pub unsafe fn $fn(v: usize) {
            asm!(concat!("csrw ", stringify!($csr_num), ", {}"),
                 in(reg) v,
                 options(nomem, nostack, preserves_flags));
        }
    };
}

read_csr!(read_mhartid, mhartid);
read_csr!(read_mstatus, mstatus);
write_csr!(write_mstatus, mstatus);
write_csr!(write_mepc, mepc);
write_csr!(write_mtvec, mtvec);
read_csr!(read_mie, mie);
write_csr!(write_mie, mie);
read_csr!(read_satp, satp);
write_csr!(write_satp, satp);
write_csr!(write_medeleg, medeleg);
write_csr!(write_mideleg, mideleg);
read_csr!(read_sie, sie);
write_csr!(write_sie, sie);
read_csr!(read_sstatus, sstatus);
write_csr!(write_sstatus, sstatus);
read_csr!(read_sip, sip);
write_csr!(write_sip, sip);
write_csr!(write_stvec, stvec);
read_csr!(read_scause, scause);
read_csr!(read_sepc, sepc);
write_csr!(write_sepc, sepc);
read_csr!(read_stval, stval);
read_csr!(read_time, time);
write_csr!(write_pmpaddr0, pmpaddr0);
write_csr!(write_pmpcfg0, pmpcfg0);

// New / non-standard CSRs: menvcfg=0x30A, mcounteren=0x306, stimecmp=0x14D.
read_csr_num!(read_menvcfg, 0x30a);
write_csr_num!(write_menvcfg, 0x30a);
read_csr_num!(read_mcounteren, 0x306);
write_csr_num!(write_mcounteren, 0x306);
read_csr_num!(read_stimecmp, 0x14d);
write_csr_num!(write_stimecmp, 0x14d);

#[inline(always)]
pub fn read_tp() -> usize {
    let v: usize;
    unsafe {
        asm!("mv {}, tp", out(reg) v, options(nomem, nostack, preserves_flags));
    }
    v
}

#[inline(always)]
pub unsafe fn write_tp(v: usize) {
    asm!("mv tp, {}", in(reg) v, options(nomem, nostack, preserves_flags));
}

#[inline(always)]
pub unsafe fn sfence_vma() {
    asm!("sfence.vma zero, zero", options(nostack, preserves_flags));
}

#[inline(always)]
pub unsafe fn wfi() {
    asm!("wfi", options(nomem, nostack, preserves_flags));
}

pub const SSTATUS_SIE: usize = 1 << 1;

#[inline(always)]
pub unsafe fn intr_off() {
    let s = read_sstatus();
    write_sstatus(s & !SSTATUS_SIE);
}

#[inline(always)]
pub unsafe fn intr_on() {
    let s = read_sstatus();
    write_sstatus(s | SSTATUS_SIE);
}

#[inline(always)]
pub fn intr_get() -> bool {
    (read_sstatus() & SSTATUS_SIE) != 0
}
