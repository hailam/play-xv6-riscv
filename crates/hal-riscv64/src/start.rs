//! M-mode bootstrap. `_entry` (asm) calls `mstart` once per hart. We do
//! the minimum to delegate everything to S-mode, then `mret` to `kmain`.

use core::arch::asm;

use crate::csr;

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

    csr::write_mepc(kmain as *const () as usize);
    asm!("mret", options(noreturn));
}
