//! Process abstraction.

use alloc::boxed::Box;
use alloc::sync::{Arc, Weak};
use alloc::vec::Vec;
use core::future::Future;
use core::pin::Pin;
use core::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, AtomicUsize, Ordering};
use core::task::{Context, Poll};

use hal::{FrameAllocator, PageTableOps, PtePerm};
// (FrameAllocator brought in above so `Drop for Proc` can free
// `trapframe_pa` via the same KFRAMES handle the rest of the code uses.)

use crate::arch::{Arch, Hal};
use crate::cpu;
use crate::executor;
use crate::file::File;
use crate::kalloc::KFRAMES;
use crate::sync::SpinLock;
use crate::syscall;
use crate::user_vm::{self, STACK_VA_BASE};
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
    /// Set by `sys_kill`. Every blocking future's `poll` checks this
    /// and bails (returning a sentinel value); `proc_main` then routes
    /// the killed proc into `sys_exit(-1)`.
    pub killed: AtomicBool,
    /// Per-proc file descriptor table.
    pub files: SpinLock<Vec<Option<Arc<File>>>>,
    /// Current working directory. `None` until the fs is up; the
    /// first `bringup_then_init` sets it to inode 1 (root) on the
    /// init proc, and every `fork` clones the parent's cwd.
    pub cwd: SpinLock<Option<Arc<crate::fs::inode::Inode>>>,
}

impl Proc {
    pub fn new_initcode(initcode_elf: &[u8]) -> Self {
        let tf_pa = KFRAMES.alloc_zeroed().expect("kalloc TRAPFRAME");
        let image = user_vm::build_image_from_elf(initcode_elf, tf_pa)
            .expect("build initcode image");
        // Empty argv for the first proc.
        let (sp_va, argv_array_va) =
            user_vm::place_argv_on_stack(image.stack_pa, &[]);

        let tf = unsafe { &mut *(tf_pa as *mut TrapFrame) };
        tf.epc = image.entry as u64;
        tf.sp = sp_va as u64;
        tf.a0 = 0;
        tf.a1 = argv_array_va as u64;

        Self::with_layout(image.pagetable, tf_pa, image.code_end, default_files())
    }

    pub fn fork_from(parent: &Arc<Proc>) -> Option<Self> {
        let code_end = parent.size.load(Ordering::Relaxed);

        let tf_pa = KFRAMES.alloc_zeroed()?;
        let mut pt = <Arch as Hal>::PageTable::new(&KFRAMES).ok()?;
        pt.map(TRAMPOLINE, trampoline_pa(), PGSIZE, PtePerm::RX, &KFRAMES)
            .ok()?;
        pt.map(TRAPFRAME, tf_pa, PGSIZE, PtePerm::RW, &KFRAMES)
            .ok()?;

        let parent_pt = parent.pagetable.lock();

        // Code/data pages.
        let mut va = 0;
        while va < code_end {
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

        // Stack page (fixed VA just below TRAPFRAME).
        let (parent_stack_pa, stack_perm) = parent_pt.translate(STACK_VA_BASE)?;
        let child_stack_pa = KFRAMES.alloc_zeroed()?;
        unsafe {
            core::ptr::copy_nonoverlapping(
                parent_stack_pa as *const u8,
                child_stack_pa as *mut u8,
                PGSIZE,
            );
        }
        pt.map(STACK_VA_BASE, child_stack_pa, PGSIZE, stack_perm, &KFRAMES)
            .ok()?;

        drop(parent_pt);

        let parent_tf = parent.trapframe();
        let child_tf = unsafe { &mut *(tf_pa as *mut TrapFrame) };
        *child_tf = *parent_tf;
        child_tf.a0 = 0;

        let child_files: Vec<Option<Arc<File>>> = parent
            .files
            .lock()
            .iter()
            .map(|f| f.as_ref().map(|a| Arc::new((**a).clone())))
            .collect();

        let child = Self::with_layout(pt, tf_pa, code_end, child_files);
        // Child inherits the parent's cwd (Arc clone — same inode).
        *child.cwd.lock() = parent.cwd.lock().clone();
        Some(child)
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
            killed: AtomicBool::new(false),
            files: SpinLock::new(files),
            cwd: SpinLock::new(None),
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

impl Drop for Proc {
    fn drop(&mut self) {
        // The pagetable's own Drop already frees user pages +
        // intermediate tables; the trapframe was allocated separately
        // (it's mapped into the pagetable without PTE_U so the
        // reaper leaves it alone).
        if self.trapframe_pa != 0 {
            unsafe { KFRAMES.free(self.trapframe_pa) };
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
            }
            TrapEvent::Timer | TrapEvent::Devintr => {}
        }
        if proc.killed.load(Ordering::Acquire) && !proc.is_zombie() {
            let _ = syscall::sys_exit(&proc, -1).await;
        }
        if proc.is_zombie() {
            // Return cleanly. The executor sees `Poll::Ready(())` and
            // leaves the task slot empty. Dropping this future also
            // drops its captured `Arc<Proc>` — once the parent's
            // `wait` reaps the child from `parent.children`, the
            // refcount falls to zero and `Drop for Proc` returns the
            // trapframe page and the (now-empty) pagetable root.
            return;
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
