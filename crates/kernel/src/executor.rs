//! Per-CPU async executor with sticky `home_cpu`.
//!
//! Model:
//!   * Each CPU owns a `PerCpuExec` (tasks Vec + ready VecDeque).
//!   * `TaskId` encodes `(cpu, slot)` as one `u32`. The high 8 bits
//!     are the home CPU, the low 24 bits index into that CPU's
//!     `tasks` Vec. The encoding lets `wake(id)` and the RawWaker
//!     find the right per-CPU queue without a global task table.
//!   * `run()` is the kernel's main loop, one instance per hart. It
//!     drains the local ready queue, polls each task, and after each
//!     poll checks `cpu::take_user_target()` — if set, transfers
//!     control to user mode (noreturn) on this hart.
//!   * Cross-CPU wake: `wake(id)` decodes the home CPU and pushes
//!     to that CPU's ready queue. If the home is a remote hart, the
//!     remote hart picks it up on its next timer tick. (Proper IPI
//!     plumbing is a follow-on — see [[ipi-plumbing]].)

use alloc::boxed::Box;
use alloc::collections::VecDeque;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::future::Future;
use core::pin::Pin;
use core::sync::atomic::{AtomicU32, Ordering};
use core::task::{Context, RawWaker, RawWakerVTable, Waker};

use hal::Hal;

use crate::arch::{Arch, MAX_CPUS};
use crate::cpu;
use crate::proc::Proc;
use crate::sync::SpinLock;

pub type TaskId = u32;
pub type FutureBox = Pin<Box<dyn Future<Output = ()> + Send + 'static>>;

const CPU_BITS: u32 = 8;
const SLOT_MASK: u32 = (1u32 << (32 - CPU_BITS)) - 1;

#[inline]
fn make_tid(cpu: usize, slot: u32) -> TaskId {
    debug_assert!(cpu < MAX_CPUS);
    debug_assert!(slot <= SLOT_MASK);
    ((cpu as u32) << (32 - CPU_BITS)) | slot
}

#[inline]
fn tid_cpu(tid: TaskId) -> usize {
    (tid >> (32 - CPU_BITS)) as usize
}

#[inline]
fn tid_slot(tid: TaskId) -> usize {
    (tid & SLOT_MASK) as usize
}

struct Task {
    /// `None` for kernel-only tasks (no return-to-user, no current_proc).
    proc: Option<Arc<Proc>>,
    future: FutureBox,
}

struct PerCpuExec {
    tasks: SpinLock<Vec<Option<Task>>>,
    ready: SpinLock<VecDeque<TaskId>>,
    next_slot: AtomicU32,
}

impl PerCpuExec {
    const fn new() -> Self {
        Self {
            tasks: SpinLock::new(Vec::new()),
            ready: SpinLock::new(VecDeque::new()),
            next_slot: AtomicU32::new(0),
        }
    }
}

static EXECUTORS: [PerCpuExec; MAX_CPUS] = [const { PerCpuExec::new() }; MAX_CPUS];

fn current_exec() -> &'static PerCpuExec {
    &EXECUTORS[Arch::hartid()]
}

/// Pick the home CPU for a new task: the active hart with the
/// shortest ready queue. "Active" means it has called
/// `cpu::init_this_hart` — so we never spawn onto a hart QEMU
/// didn't actually start.
fn pick_home_cpu() -> usize {
    let active = cpu::active_cpu_mask();
    if active == 0 {
        // Boot path — nobody's marked active yet (we're called from
        // hart 0 before its init). Default to hart 0.
        return 0;
    }
    let mut best = active.trailing_zeros() as usize; // first set bit
    let mut best_len = EXECUTORS[best].ready.lock().len();
    for c in (best + 1)..MAX_CPUS {
        if active & (1u64 << c) == 0 {
            continue;
        }
        let l = EXECUTORS[c].ready.lock().len();
        if l < best_len {
            best_len = l;
            best = c;
        }
    }
    best
}

pub fn spawn<F>(proc: Arc<Proc>, future_fn: F) -> TaskId
where
    F: FnOnce(Arc<Proc>) -> FutureBox,
{
    let home = pick_home_cpu();
    let future = future_fn(proc.clone());
    let tid = insert_task(home, Task { proc: Some(proc.clone()), future });
    proc.task_id.store(tid, Ordering::Relaxed);
    tid
}

/// Spawn a kernel-only task on a specific CPU — no Proc, no
/// return-to-user. Used at boot (bringup pinned to hart 0).
pub fn spawn_kernel_on<F>(cpu: usize, future_fn: F) -> TaskId
where
    F: FnOnce() -> FutureBox,
{
    insert_task(cpu, Task { proc: None, future: future_fn() })
}

/// Spawn a kernel-only task; picks the least-loaded CPU.
pub fn spawn_kernel<F>(future_fn: F) -> TaskId
where
    F: FnOnce() -> FutureBox,
{
    spawn_kernel_on(pick_home_cpu(), future_fn)
}

fn insert_task(home: usize, task: Task) -> TaskId {
    let exec = &EXECUTORS[home];
    let slot = exec.next_slot.fetch_add(1, Ordering::Relaxed);
    let tid = make_tid(home, slot);
    {
        let mut tasks = exec.tasks.lock();
        while tasks.len() <= slot as usize {
            tasks.push(None);
        }
        tasks[slot as usize] = Some(task);
    }
    exec.ready.lock().push_back(tid);
    tid
}

pub fn wake(id: TaskId) {
    EXECUTORS[tid_cpu(id)].ready.lock().push_back(id);
    // No cross-hart IPI yet — a remote hart picks it up on its
    // next timer tick. [[ipi-plumbing]]
}

/// Linear scan over all per-CPU task tables for a proc with `pid`.
pub fn find_proc_by_pid(pid: usize) -> Option<Arc<Proc>> {
    for c in 0..MAX_CPUS {
        let tasks = EXECUTORS[c].tasks.lock();
        for t in tasks.iter().flatten() {
            if let Some(p) = t.proc.as_ref() {
                if p.pid == pid {
                    return Some(Arc::clone(p));
                }
            }
        }
    }
    None
}

pub fn run() -> ! {
    unsafe { Arch::intr_on() };
    let exec = current_exec();
    loop {
        let tid = exec.ready.lock().pop_front();
        let Some(tid) = tid else {
            unsafe { Arch::wfi() };
            continue;
        };
        // A foreign hart may have enqueued a tid whose home is us,
        // or — defensively — a stale tid for us. Drop tids that
        // aren't ours.
        if tid_cpu(tid) != Arch::hartid() {
            // Forward to the rightful owner.
            EXECUTORS[tid_cpu(tid)].ready.lock().push_back(tid);
            continue;
        }

        let slot = tid_slot(tid);
        let mut task = match exec.tasks.lock().get_mut(slot).and_then(|o| o.take()) {
            Some(t) => t,
            None => continue,
        };

        let proc_ptr: *mut Proc = match &task.proc {
            Some(p) => Arc::as_ptr(p) as *mut _,
            None => core::ptr::null_mut(),
        };
        cpu::set_current_proc(proc_ptr);

        let waker = task_waker(tid);
        let mut cx = Context::from_waker(&waker);
        let poll = task.future.as_mut().poll(&mut cx);

        if poll.is_pending() {
            exec.tasks.lock()[slot] = Some(task);
        }

        if let Some(target) = cpu::take_user_target() {
            // SAFETY: target points into a `Proc` owned by an `Arc` in
            // the per-CPU `tasks` table; Procs live until parent reaps.
            unsafe {
                crate::usertrap::return_to_user(&*target);
            }
        }
    }
}

fn task_waker(tid: TaskId) -> Waker {
    let raw = RawWaker::new(tid as usize as *const (), &VTABLE);
    unsafe { Waker::from_raw(raw) }
}

unsafe fn waker_clone(p: *const ()) -> RawWaker {
    RawWaker::new(p, &VTABLE)
}
unsafe fn waker_wake(p: *const ()) {
    wake(p as usize as TaskId);
}
unsafe fn waker_wake_by_ref(p: *const ()) {
    wake(p as usize as TaskId);
}
unsafe fn waker_drop(_p: *const ()) {}

static VTABLE: RawWakerVTable =
    RawWakerVTable::new(waker_clone, waker_wake, waker_wake_by_ref, waker_drop);
