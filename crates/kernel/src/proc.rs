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
use crate::file::File;
use crate::kalloc::KFRAMES;
use crate::sync::SpinLock;
use crate::syscall;
use crate::wait::WakerCell;

#[cfg(target_arch = "riscv64")]
use hal_riscv64::{
    memlayout::{PGSIZE, TRAMPOLINE, TRAPFRAME},
    trampoline_pa, TrapFrame,
};

pub const NOFILE: usize = 16;

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
    pub pagetable: SpinLock<<Arch as Hal>::PageTable>,
    pub trapframe_pa: usize,
    pub size: AtomicUsize,
    pub task_id: AtomicU32,
    pub pending_trap: SpinLock<Option<TrapEvent>>,
    pub parent: SpinLock<Option<Weak<Proc>>>,
    pub children: SpinLock<Vec<Arc<Proc>>>,
    pub wait_waker: WakerCell,
    /// Per-proc file descriptor table.
    pub files: SpinLock<Vec<Option<Arc<File>>>>,
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

        Self::with_layout(pt, tf_pa, PGSIZE, default_files())
    }

    pub fn fork_from(parent: &Arc<Proc>) -> Option<Self> {
        let size = parent.size.load(Ordering::Relaxed);

        let tf_pa = KFRAMES.alloc_zeroed()?;
        let mut pt = <Arch as Hal>::PageTable::new(&KFRAMES).ok()?;
        pt.map(TRAMPOLINE, trampoline_pa(), PGSIZE, PtePerm::RX, &KFRAMES)
            .ok()?;
        pt.map(TRAPFRAME, tf_pa, PGSIZE, PtePerm::RW, &KFRAMES)
            .ok()?;

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

        // Clone parent's fd table — but give each child fd its own
        // `Arc<File>`. `File::Clone` bumps pipe reader/writer counts so
        // child's eventual close decrements them independently of the
        // parent's lifetime.
        let child_files: Vec<Option<Arc<File>>> = parent
            .files
            .lock()
            .iter()
            .map(|f| f.as_ref().map(|a| Arc::new((**a).clone())))
            .collect();

        Some(Self::with_layout(pt, tf_pa, size, child_files))
    }

    fn with_layout(
        pt: <Arch as Hal>::PageTable,
        trapframe_pa: usize,
        size: usize,
        files: Vec<Option<Arc<File>>>,
    ) -> Self {
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
            files: SpinLock::new(files),
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

    pub fn replace_image(&self, new_pt: <Arch as Hal>::PageTable, new_size: usize) {
        *self.pagetable.lock() = new_pt;
        self.size.store(new_size, Ordering::Release);
    }

    pub fn is_zombie(&self) -> bool {
        self.state.load(Ordering::Acquire) == ProcState::Zombie as i32
    }

    /// Look up an fd's File; clones the Arc.
    pub fn get_file(&self, fd: i32) -> Option<Arc<File>> {
        if fd < 0 {
            return None;
        }
        self.files.lock().get(fd as usize).cloned().flatten()
    }

    /// Find the lowest free fd and install `file` there. Returns the fd
    /// number or `None` if the table is full.
    pub fn alloc_fd(&self, file: Arc<File>) -> Option<i32> {
        let mut files = self.files.lock();
        for (i, slot) in files.iter_mut().enumerate() {
            if slot.is_none() {
                *slot = Some(file);
                return Some(i as i32);
            }
        }
        None
    }

    /// Drop the file in `fd`. Returns 0 on success, -1 if `fd` was
    /// already empty / out of range.
    pub fn close_fd(&self, fd: i32) -> i64 {
        if fd < 0 {
            return -1;
        }
        let mut files = self.files.lock();
        let Some(slot) = files.get_mut(fd as usize) else {
            return -1;
        };
        if slot.take().is_some() {
            0
        } else {
            -1
        }
    }
}

fn default_files() -> Vec<Option<Arc<File>>> {
    let mut files: Vec<Option<Arc<File>>> = (0..NOFILE).map(|_| None).collect();
    let console = Arc::new(File::Console);
    files[0] = Some(console.clone());
    files[1] = Some(console.clone());
    files[2] = Some(console);
    files
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
