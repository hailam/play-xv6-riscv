//! Process abstraction.
//!
//! Phase 4b model: each running user program is an async `Task` whose
//! `Future` is `proc_main`. The future loops `UserMode::run(...).await`
//! to enter user mode and dispatch the next trap event.

use alloc::boxed::Box;
use alloc::sync::Arc;
use core::future::Future;
use core::pin::Pin;
use core::sync::atomic::{AtomicI32, AtomicU32, AtomicUsize, Ordering};
use core::task::{Context, Poll};

use hal::{FrameAllocator, PageTableOps, PtePerm};

use crate::arch::{Arch, Hal};
use crate::cpu;
use crate::executor;
use crate::kalloc::KFRAMES;
use crate::sync::SpinLock;
use crate::syscall;

#[cfg(target_arch = "riscv64")]
use hal_riscv64::{
    memlayout::{PGSIZE, TRAMPOLINE, TRAPFRAME},
    trampoline_pa, TrapFrame,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum ProcState {
    Runnable = 0,
    Running = 1,
    Zombie = 2,
}

#[derive(Clone, Copy, Debug)]
pub enum TrapEvent {
    Syscall { nr: usize },
    Timer,
    Devintr,
}

pub struct Proc {
    pub pid: usize,
    pub state: AtomicI32,
    pub exit_code: AtomicI32,
    pub pagetable: <Arch as Hal>::PageTable,
    pub trapframe_pa: usize,
    pub size: AtomicUsize,
    pub task_id: AtomicU32,
    pub pending_trap: SpinLock<Option<TrapEvent>>,
}

impl Proc {
    pub fn new_initcode(initcode: &[u8]) -> Self {
        assert!(initcode.len() < PGSIZE, "initcode must fit in one page");

        let tf_pa = KFRAMES.alloc_zeroed().expect("kalloc TRAPFRAME");
        let mut pt = <Arch as Hal>::PageTable::new(&KFRAMES).expect("pagetable root");

        pt.map(TRAMPOLINE, trampoline_pa(), PGSIZE, PtePerm::RX, &KFRAMES)
            .expect("map TRAMPOLINE");
        pt.map(TRAPFRAME, tf_pa, PGSIZE, PtePerm::RW, &KFRAMES)
            .expect("map TRAPFRAME");

        let upage_pa = KFRAMES.alloc_zeroed().expect("kalloc initcode page");
        unsafe {
            core::ptr::copy_nonoverlapping(
                initcode.as_ptr(),
                upage_pa as *mut u8,
                initcode.len(),
            );
        }
        pt.map(0, upage_pa, PGSIZE, PtePerm::URWX, &KFRAMES)
            .expect("map user");

        let tf = unsafe { &mut *(tf_pa as *mut TrapFrame) };
        tf.epc = 0;
        tf.sp = PGSIZE as u64;

        Self {
            pid: next_pid(),
            state: AtomicI32::new(ProcState::Runnable as i32),
            exit_code: AtomicI32::new(0),
            pagetable: pt,
            trapframe_pa: tf_pa,
            size: AtomicUsize::new(PGSIZE),
            task_id: AtomicU32::new(0),
            pending_trap: SpinLock::new(None),
        }
    }

    /// Fork: clone parent's user vm into a child. Caller spawns the
    /// resulting `Proc` as a task. Returns `None` on OOM.
    pub fn fork_from(parent: &Arc<Proc>) -> Option<Self> {
        let size = parent.size.load(Ordering::Relaxed);

        let tf_pa = KFRAMES.alloc_zeroed()?;
        let mut pt = <Arch as Hal>::PageTable::new(&KFRAMES).ok()?;
        pt.map(TRAMPOLINE, trampoline_pa(), PGSIZE, PtePerm::RX, &KFRAMES)
            .ok()?;
        pt.map(TRAPFRAME, tf_pa, PGSIZE, PtePerm::RW, &KFRAMES)
            .ok()?;

        // Copy user pages.
        let mut va = 0;
        while va < size {
            let (parent_pa, perm) = parent.pagetable.translate(va)?;
            let child_pa = KFRAMES.alloc_zeroed()?;
            unsafe {
                core::ptr::copy_nonoverlapping(
                    parent_pa as *const u8,
                    child_pa as *mut u8,
                    PGSIZE,
                );
            }
            pt.map(va, child_pa, PGSIZE, perm, &KFRAMES).ok()?;
            va += PGSIZE;
        }

        // Copy trapframe; child's a0 = 0 so fork returns 0 in child.
        let parent_tf = parent.trapframe();
        let child_tf = unsafe { &mut *(tf_pa as *mut TrapFrame) };
        *child_tf = *parent_tf;
        child_tf.a0 = 0;

        Some(Self {
            pid: next_pid(),
            state: AtomicI32::new(ProcState::Runnable as i32),
            exit_code: AtomicI32::new(0),
            pagetable: pt,
            trapframe_pa: tf_pa,
            size: AtomicUsize::new(size),
            task_id: AtomicU32::new(0),
            pending_trap: SpinLock::new(None),
        })
    }

    pub fn trapframe(&self) -> &mut TrapFrame {
        // Safety: trapframe_pa identifies a frame this proc exclusively
        // owns, identity-mapped in kernel space, mutated only by the
        // current-CPU task in Phase 4b/5a.
        unsafe { &mut *(self.trapframe_pa as *mut TrapFrame) }
    }

    pub fn translate_user(&self, va: usize) -> Option<usize> {
        let page = va & !(PGSIZE - 1);
        let off = va & (PGSIZE - 1);
        let (pa, perm) = self.pagetable.translate(page)?;
        if perm.0 & PtePerm::USER == 0 {
            return None;
        }
        Some(pa + off)
    }

    pub fn is_zombie(&self) -> bool {
        self.state.load(Ordering::Acquire) == ProcState::Zombie as i32
    }
}

fn next_pid() -> usize {
    static NEXT: AtomicU32 = AtomicU32::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed) as usize
}

// =============================================================================
// proc_main async fn — the task body for any user proc.
// =============================================================================

pub fn spawn_proc_main(proc: Arc<Proc>) -> executor::TaskId {
    executor::spawn(proc, |p| Box::pin(proc_main(p)))
}

async fn proc_main(proc: Arc<Proc>) {
    loop {
        let event = UserMode::run(&proc).await;
        match event {
            TrapEvent::Syscall { nr } => {
                let ret = syscall::dispatch(&proc, nr).await;
                proc.trapframe().a0 = ret as u64;
                if proc.is_zombie() {
                    // Sys_exit ran — park forever so the task is never polled again.
                    core::future::pending::<()>().await;
                }
            }
            TrapEvent::Timer | TrapEvent::Devintr => {
                // Cooperative: just loop back to user.
            }
        }
    }
}

// =============================================================================
// UserMode future — the only place that signals "executor should
// return-to-user after this poll".
// =============================================================================

pub struct UserMode<'a> {
    proc: &'a Proc,
}

impl<'a> UserMode<'a> {
    pub fn run(proc: &'a Arc<Proc>) -> Self {
        Self { proc }
    }
}

impl Future for UserMode<'_> {
    type Output = TrapEvent;

    fn poll(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<TrapEvent> {
        if let Some(ev) = self.proc.pending_trap.lock().take() {
            return Poll::Ready(ev);
        }
        cpu::set_user_target(self.proc as *const Proc);
        Poll::Pending
    }
}
