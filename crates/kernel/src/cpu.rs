//! Per-CPU state.

use core::ptr::null_mut;
use core::sync::atomic::{AtomicBool, AtomicPtr, AtomicU64, AtomicUsize, Ordering};

use crate::arch::{Arch, Hal, MAX_CPUS};
use crate::proc::Proc;

/// Bitmask of harts that have called [`init_this_hart`]. Used by
/// `executor::pick_home_cpu` so we never assign a task to a hart
/// that QEMU didn't actually spin up (the static `MAX_CPUS` is just
/// the upper bound).
static ACTIVE_CPUS: AtomicU64 = AtomicU64::new(0);

pub fn active_cpu_mask() -> u64 {
    ACTIVE_CPUS.load(Ordering::Acquire)
}

#[repr(align(64))]
pub struct Cpu {
    pub hartid: AtomicUsize,
    pub noff: AtomicUsize,
    pub intena: AtomicBool,
    /// Proc currently running on this hart in user mode (set just before
    /// returning to user). Read by the trap handler to identify the
    /// trapping proc.
    pub current_proc: AtomicPtr<Proc>,
    /// One-shot slot set by `UserMode::poll`. The executor pops this
    /// after each `poll` to decide whether to noreturn into user mode.
    pub user_target: AtomicPtr<Proc>,
}

impl Cpu {
    const fn new() -> Self {
        Self {
            hartid: AtomicUsize::new(usize::MAX),
            noff: AtomicUsize::new(0),
            intena: AtomicBool::new(false),
            current_proc: AtomicPtr::new(null_mut()),
            user_target: AtomicPtr::new(null_mut()),
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
    ACTIVE_CPUS.fetch_or(1u64 << id, Ordering::Release);
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
        // Safety: caller responsible — procs live forever in Phase 4b/5a.
        Some(unsafe { &*p })
    }
}

pub fn set_user_target(p: *const Proc) {
    current().user_target.store(p as *mut _, Ordering::Release);
}

pub fn take_user_target() -> Option<*const Proc> {
    let p = current().user_target.swap(null_mut(), Ordering::Acquire);
    if p.is_null() {
        None
    } else {
        Some(p as *const Proc)
    }
}
