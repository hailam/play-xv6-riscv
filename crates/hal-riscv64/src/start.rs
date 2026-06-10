//! M-mode bootstrap. `_entry` (asm) calls `mstart` once per hart. We do
//! the minimum to delegate everything to S-mode, then `mret` to `kmain`.

use core::arch::asm;

use crate::csr;
use crate::memlayout::CLINT;

const MIE_MSIE: usize = 1 << 3; // machine software interrupt enable

/// Per-hart M-mode scratch for the IPI trampoline:
///   [0] saved a1, [1] unused, [2] this hart's CLINT MSIP address.
#[repr(C, align(16))]
struct MScratch([u64; 4]);
static mut MSCRATCH: [MScratch; 8] = [const { MScratch([0; 4]) }; 8];

// M-mode trap vector. The ONLY machine interrupt we leave enabled is
// MSIP (the cross-hart IPI doorbell): clear it, convert it into a
// supervisor software interrupt (SSIP — delegated to S-mode), mret.
// S-mode sees scause=1 and treats it as a pure wakeup kick.
core::arch::global_asm!(
    r#"
.section .text
.balign 4
.global m_ipivec
m_ipivec:
        csrrw   a0, mscratch, a0
        sd      a1, 0(a0)
        ld      a1, 16(a0)        # this hart's CLINT MSIP address
        sw      zero, 0(a1)       # clear the doorbell
        li      a1, 2
        csrs    mip, a1           # raise SSIP for S-mode
        ld      a1, 0(a0)
        csrrw   a0, mscratch, a0
        mret
"#
);

extern "C" {
    fn m_ipivec();
}

extern "C" {
    fn kmain() -> !;
}

const MSTATUS_MPP_MASK: usize = 3 << 11;
const MSTATUS_MPP_S: usize = 1 << 11;
const SIE_SEIE: usize = 1 << 9; // external
const SIE_STIE: usize = 1 << 5; // timer
const SIE_SSIE: usize = 1 << 1; // software

const MENVCFG_STCE: usize = 1 << 63; // Sstc — enables S-mode `stimecmp`.

#[no_mangle]
pub unsafe extern "C" fn mstart() -> ! {
    let mstatus = (csr::read_mstatus() & !MSTATUS_MPP_MASK) | MSTATUS_MPP_S;
    csr::write_mstatus(mstatus);
    csr::write_satp(0);

    csr::write_medeleg(0xffff);
    csr::write_mideleg(0xffff);
    csr::write_sie(csr::read_sie() | SIE_SEIE | SIE_STIE | SIE_SSIE);

    // Enable the Sstc extension so S-mode can program `stimecmp` directly.
    csr::write_menvcfg(csr::read_menvcfg() | MENVCFG_STCE);
    // Let S-mode read `time` and friends.
    csr::write_mcounteren(0x7);

    csr::write_pmpaddr0(0x3fff_ffff_ffff_ffff);
    csr::write_pmpcfg0(0xf);

    csr::write_tp(csr::read_mhartid());

    // Cross-hart IPI doorbell: another hart writes our CLINT MSIP;
    // the M-mode trampoline above converts it to an SSIP kick.
    let hart = csr::read_mhartid();
    let sc = core::ptr::addr_of_mut!(MSCRATCH[hart]) as *mut u64;
    unsafe { *sc.add(2) = (CLINT + 4 * hart) as u64 };
    csr::write_mscratch(sc as usize);
    csr::write_mtvec(m_ipivec as *const () as usize);
    csr::write_mie(csr::read_mie() | MIE_MSIE);

    csr::write_mepc(kmain as *const () as usize);
    asm!("mret", options(noreturn));
}
