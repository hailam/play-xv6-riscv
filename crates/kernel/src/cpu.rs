//! Per-CPU state.

use core::ptr::null_mut;
use core::sync::atomic::{AtomicBool, AtomicPtr, AtomicUsize, Ordering};

use crate::arch::{Arch, Hal, MAX_CPUS};
use crate::proc::Proc;

#[repr(align(64))] // dodge false sharing across harts
pub struct Cpu {
    pub hartid: AtomicUsize,
    pub noff: AtomicUsize,
    pub intena: AtomicBool,
    /// Process currently running on this hart, or null. Set before
    /// returning to user mode; read in trap-from-user dispatch.
    pub current_proc: AtomicPtr<Proc>,
}

impl Cpu {
    const fn new() -> Self {
        Self {
            hartid: AtomicUsize::new(usize::MAX),
            noff: AtomicUsize::new(0),
            intena: AtomicBool::new(false),
            current_proc: AtomicPtr::new(null_mut()),
        }
    }
}

static CPUS: [Cpu; MAX_CPUS] = [const { Cpu::new() }; MAX_CPUS];

pub fn current() -> &'static Cpu {
    &CPUS[Arch::hartid()]
}

pub fn init_this_hart() {
    let id = Arch::hartid();
    CPUS[id].hartid.store(id, Ordering::Relaxed);
}

pub fn push_off() {
    let enabled = Arch::intr_get();
    unsafe { Arch::intr_off() };
    let c = current();
    if c.noff.load(Ordering::Relaxed) == 0 {
        c.intena.store(enabled, Ordering::Relaxed);
    }
    c.noff.fetch_add(1, Ordering::Relaxed);
}

pub fn pop_off() {
    debug_assert!(!Arch::intr_get(), "pop_off with interrupts on");
    let c = current();
    let prev = c.noff.fetch_sub(1, Ordering::Relaxed);
    debug_assert!(prev >= 1, "pop_off: noff underflow");
    if prev == 1 && c.intena.load(Ordering::Relaxed) {
        unsafe { Arch::intr_on() };
    }
}

pub fn set_current_proc(p: *mut Proc) {
    current().current_proc.store(p, Ordering::Release);
}

pub fn current_proc() -> Option<&'static Proc> {
    let p = current().current_proc.load(Ordering::Acquire);
    if p.is_null() {
        None
    } else {
        // Safety: the proc is heap-allocated and lives forever once spawned.
        Some(unsafe { &*p })
    }
}
