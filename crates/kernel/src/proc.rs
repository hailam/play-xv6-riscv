//! Process abstraction. Phase 4: one `Proc` per running user program,
//! holding its pagetable and trapframe physical address.

use core::ptr::NonNull;
use core::sync::atomic::{AtomicI32, AtomicUsize, Ordering};

use hal::{FrameAllocator, PageTableOps, PtePerm};

use crate::arch::{Arch, Hal};
use crate::kalloc::KFRAMES;

#[cfg(target_arch = "riscv64")]
use hal_riscv64::{
    memlayout::{PGSIZE, TRAMPOLINE, TRAPFRAME},
    trampoline_pa, TrapFrame,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcState {
    Runnable,
    Running,
    Zombie,
}

pub struct Proc {
    pub pid: usize,
    pub state: AtomicI32, // ProcState encoded as i32
    pub exit_code: AtomicI32,
    /// Owned user pagetable. Never dropped while the proc is alive.
    pub pagetable: <Arch as Hal>::PageTable,
    /// Physical address of the proc's trapframe page (mapped at TRAPFRAME
    /// in the user pagetable; identity-mapped in the kernel pagetable
    /// for direct kernel access).
    pub trapframe_pa: usize,
    /// Bytes of user address space currently mapped (text + stack).
    pub size: AtomicUsize,
}

impl Proc {
    /// Construct an `init`-style process: lay out a fresh user
    /// pagetable, copy `initcode` to user VA 0, set up the trapframe
    /// for the first return-to-user.
    pub fn new_initcode(initcode: &[u8]) -> Self {
        assert!(initcode.len() < PGSIZE, "initcode must fit in one page");

        let tf_pa = KFRAMES
            .alloc_zeroed()
            .expect("kalloc TRAPFRAME frame");

        let mut pt = <Arch as Hal>::PageTable::new(&KFRAMES)
            .expect("alloc user pagetable root");

        // Trampoline (kernel <-> user transition page).
        pt.map(TRAMPOLINE, trampoline_pa(), PGSIZE, PtePerm::RX, &KFRAMES)
            .expect("map TRAMPOLINE");

        // Trapframe — kernel writes regs here on trap, user can never see it.
        pt.map(TRAPFRAME, tf_pa, PGSIZE, PtePerm::RW, &KFRAMES)
            .expect("map TRAPFRAME");

        // User text + stack: one page at VA 0 with RWX|U. Initcode at the
        // bottom; sp starts at the top of the page.
        let upage_pa = KFRAMES.alloc_zeroed().expect("kalloc initcode page");
        // Copy initcode bytes into the user page (identity-mapped in kernel).
        unsafe {
            core::ptr::copy_nonoverlapping(
                initcode.as_ptr(),
                upage_pa as *mut u8,
                initcode.len(),
            );
        }
        pt.map(0, upage_pa, PGSIZE, PtePerm::URWX, &KFRAMES)
            .expect("map user text+stack");

        // Initialize trapframe for first userret.
        let tf = unsafe { &mut *(tf_pa as *mut TrapFrame) };
        tf.epc = 0; // user PC = _start at VA 0
        tf.sp = PGSIZE as u64; // user sp = top of the single user page
        // kernel_satp/kernel_sp/kernel_trap/kernel_hartid are set just
        // before each return-to-user.

        Self {
            pid: next_pid(),
            state: AtomicI32::new(ProcState::Runnable as i32),
            exit_code: AtomicI32::new(0),
            pagetable: pt,
            trapframe_pa: tf_pa,
            size: AtomicUsize::new(PGSIZE),
        }
    }

    pub fn trapframe(&self) -> &mut TrapFrame {
        // Safety: trapframe_pa points to a frame we own, identity-mapped
        // in the kernel pagetable. Only this proc's task (single-CPU in
        // Phase 4) accesses it.
        unsafe { &mut *(self.trapframe_pa as *mut TrapFrame) }
    }

    /// Walk user pagetable to translate a user VA. Returns the kernel
    /// VA (== PA) the user VA refers to.
    pub fn translate_user(&self, va: usize) -> Option<usize> {
        let page_va = va & !(PGSIZE - 1);
        let off = va & (PGSIZE - 1);
        let (pa, perm) = self.pagetable.translate(page_va)?;
        if perm.0 & PtePerm::USER == 0 {
            return None;
        }
        Some(pa + off)
    }
}

impl Drop for Proc {
    fn drop(&mut self) {
        // Phase 4: procs live forever. Drop should free the pagetable
        // tree and the trapframe page, but we never reach this path yet.
        // Leaving as no-op until Phase 5 introduces exit/wait reaping.
    }
}

fn next_pid() -> usize {
    static NEXT: AtomicUsize = AtomicUsize::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

/// Convenience: return a never-null `Proc` pointer suitable for cpu.current_proc.
pub fn as_raw(p: &Proc) -> NonNull<Proc> {
    NonNull::from(p)
}
