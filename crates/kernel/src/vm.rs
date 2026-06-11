//! Kernel virtual memory: build the kernel pagetable and install it on
//! each hart.

use core::sync::atomic::{AtomicUsize, Ordering};

use hal::{PageTableOps, PtePerm};

use crate::arch::{Arch, Hal};
use crate::kalloc::KFRAMES;

use crate::arch::{
    trampoline_pa, INTC_BASE, INTC_SIZE, KERNBASE, PGSIZE, PHYSTOP, TRAMPOLINE, UART0,
    UART0_SIZE, VIRTIO0, VIRTIO0_SIZE,
};

const VIRTIO1: usize = <Arch as Hal>::VIRTIO1;
const VIRTIO1_SIZE: usize = <Arch as Hal>::VIRTIO1_SIZE;

extern "C" {
    static _etext: u8;
}

static KERNEL_SATP: AtomicUsize = AtomicUsize::new(0);

/// Build the kernel pagetable, stash its `satp` value globally, and
/// install it on this hart. Call once from hart 0.
pub fn init_and_install() {
    let mut pt =
        <Arch as Hal>::PageTable::new(&KFRAMES).expect("kvm: pagetable alloc");

    // MMIO regions, identity-mapped.
    pt.map(UART0, UART0, UART0_SIZE, PtePerm::RW, &KFRAMES)
        .expect("map UART0");
    pt.map(VIRTIO0, VIRTIO0, VIRTIO0_SIZE, PtePerm::RW, &KFRAMES)
        .expect("map VIRTIO0");
    // virtio-net slot — on aarch64 it shares VIRTIO0's page (0x200
    // stride), so skip the duplicate mapping there.
    if VIRTIO1 & !(PGSIZE - 1) != VIRTIO0 & !(PGSIZE - 1) {
        pt.map(
            VIRTIO1 & !(PGSIZE - 1),
            VIRTIO1 & !(PGSIZE - 1),
            (VIRTIO1_SIZE + PGSIZE - 1) & !(PGSIZE - 1),
            PtePerm::RW,
            &KFRAMES,
        )
        .expect("map VIRTIO1");
    }
    pt.map(INTC_BASE, INTC_BASE, INTC_SIZE, PtePerm::RW, &KFRAMES)
        .expect("map interrupt controller");
    // Arch-specific extras (riscv: the CLINT page, so S-mode can ring
    // MSIP doorbells for cross-hart IPIs).
    for &(base, size) in <Arch as Hal>::EXTRA_MMIO {
        pt.map(base, base, size, PtePerm::RW, &KFRAMES)
            .expect("map extra MMIO");
    }

    // Kernel text RX (includes trampoline page since it's inside .text),
    // kernel data + free physmem RW.
    let etext = unsafe { &_etext as *const u8 as usize };
    pt.map(KERNBASE, KERNBASE, etext - KERNBASE, PtePerm::RX, &KFRAMES)
        .expect("map kernel text");
    pt.map(etext, etext, PHYSTOP - etext, PtePerm::RW, &KFRAMES)
        .expect("map kernel data");

    // Trampoline page mapped a second time at TRAMPOLINE (the same VA
    // both kernel and user pagetables use during trap entry/exit).
    pt.map(TRAMPOLINE, trampoline_pa(), PGSIZE, PtePerm::RX, &KFRAMES)
        .expect("map TRAMPOLINE");

    let satp = <Arch as Hal>::pagetable_satp(&pt);
    KERNEL_SATP.store(satp, Ordering::Release);
    core::mem::forget(pt);

    install_on_this_hart();
}

pub fn install_on_this_hart() {
    let satp = KERNEL_SATP.load(Ordering::Acquire);
    assert!(satp != 0, "kvm not initialized");
    unsafe { <Arch as Hal>::write_satp(satp) };
}

pub fn kernel_satp() -> usize {
    KERNEL_SATP.load(Ordering::Acquire)
}
