//! Process abstraction.

use alloc::boxed::Box;
use alloc::sync::{Arc, Weak};
use alloc::vec::Vec;
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
use crate::wait::WakerCell;

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
    /// Page table protected by a lock so `sys_exec` can replace it.
    pub pagetable: SpinLock<<Arch as Hal>::PageTable>,
    pub trapframe_pa: usize,
    pub size: AtomicUsize,
    pub task_id: AtomicU32,
    pub pending_trap: SpinLock<Option<TrapEvent>>,
    pub parent: SpinLock<Option<Weak<Proc>>>,
    pub children: SpinLock<Vec<Arc<Proc>>>,
    pub wait_waker: WakerCell,
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

        Self::with_layout(pt, tf_pa, PGSIZE)
    }

    pub fn fork_from(parent: &Arc<Proc>) -> Option<Self> {
        let size = parent.size.load(Ordering::Relaxed);

        let tf_pa = KFRAMES.alloc_zeroed()?;
        let mut pt = <Arch as Hal>::PageTable::new(&KFRAMES).ok()?;
        pt.map(TRAMPOLINE, trampoline_pa(), PGSIZE, PtePerm::RX, &KFRAMES)
            .ok()?;
        pt.map(TRAPFRAME, tf_pa, PGSIZE, PtePerm::RW, &KFRAMES)
            .ok()?;

        // Lock parent's pagetable across the whole copy.
        let parent_pt = parent.pagetable.lock();
        let mut va = 0;
        while va < size {
            let (parent_pa, perm) = parent_pt.translate(va)?;
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
        drop(parent_pt);

        let parent_tf = parent.trapframe();
        let child_tf = unsafe { &mut *(tf_pa as *mut TrapFrame) };
        *child_tf = *parent_tf;
        child_tf.a0 = 0;

        Some(Self::with_layout(pt, tf_pa, size))
    }

    fn with_layout(pt: <Arch as Hal>::PageTable, trapframe_pa: usize, size: usize) -> Self {
        Self {
            pid: next_pid(),
            state: AtomicI32::new(ProcState::Runnable as i32),
            exit_code: AtomicI32::new(0),
            pagetable: SpinLock::new(pt),
            trapframe_pa,
            size: AtomicUsize::new(size),
            task_id: AtomicU32::new(0),
            pending_trap: SpinLock::new(None),
            parent: SpinLock::new(None),
            children: SpinLock::new(Vec::new()),
            wait_waker: WakerCell::new(),
        }
    }

    pub fn trapframe(&self) -> &mut TrapFrame {
        unsafe { &mut *(self.trapframe_pa as *mut TrapFrame) }
    }

    pub fn satp(&self) -> usize {
        <Arch as Hal>::pagetable_satp(&self.pagetable.lock())
    }

    pub fn translate_user(&self, va: usize) -> Option<usize> {
        let page = va & !(PGSIZE - 1);
        let off = va & (PGSIZE - 1);
        let pt = self.pagetable.lock();
        let (pa, perm) = pt.translate(page)?;
        if perm.0 & PtePerm::USER == 0 {
            return None;
        }
        Some(pa + off)
    }

    /// Replace this proc's user pagetable + size. Caller has prepared
    /// the new pagetable already (with TRAMPOLINE/TRAPFRAME re-mapped).
    /// Old pagetable is dropped (frames leak in Phase 5c — TODO: real
    /// vm-reap pass).
    pub fn replace_image(&self, new_pt: <Arch as Hal>::PageTable, new_size: usize) {
        *self.pagetable.lock() = new_pt;
        self.size.store(new_size, Ordering::Release);
    }

    pub fn is_zombie(&self) -> bool {
        self.state.load(Ordering::Acquire) == ProcState::Zombie as i32
    }
}

fn next_pid() -> usize {
    static NEXT: AtomicU32 = AtomicU32::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed) as usize
}

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
                    core::future::pending::<()>().await;
                }
            }
            TrapEvent::Timer | TrapEvent::Devintr => {}
        }
    }
}

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
