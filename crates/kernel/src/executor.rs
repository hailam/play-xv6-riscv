//! Single global async executor — Phase 4b/5a scope. Per-CPU sticky
//! executors land in Phase 7 when SMP user procs come online.
//!
//! Model:
//!   * Each `Task` owns an `Arc<Proc>` and a heap-pinned `dyn Future`.
//!   * `run()` is the kernel's main loop. It pops ready task ids, polls,
//!     and after each poll checks `cpu::take_user_target()`. If set, it
//!     transfers control to user mode (noreturn).
//!   * `UserMode::poll` is the only place that sets `user_target`. It
//!     returns `Pending` so the executor can `return_to_user` *outside*
//!     of `poll` — leaving the task safely back in its slot.

use alloc::boxed::Box;
use alloc::collections::VecDeque;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::future::Future;
use core::pin::Pin;
use core::sync::atomic::{AtomicU32, Ordering};
use core::task::{Context, RawWaker, RawWakerVTable, Waker};

use hal::Hal;

use crate::arch::Arch;
use crate::cpu;
use crate::proc::Proc;
use crate::sync::SpinLock;

pub type TaskId = u32;
pub type FutureBox = Pin<Box<dyn Future<Output = ()> + Send + 'static>>;

struct Task {
    proc: Arc<Proc>,
    future: FutureBox,
}

struct Executor {
    tasks: SpinLock<Vec<Option<Task>>>,
    ready: SpinLock<VecDeque<TaskId>>,
    next_id: AtomicU32,
}

static EXECUTOR: Executor = Executor {
    tasks: SpinLock::new(Vec::new()),
    ready: SpinLock::new(VecDeque::new()),
    next_id: AtomicU32::new(1),
};

pub fn spawn<F>(proc: Arc<Proc>, future_fn: F) -> TaskId
where
    F: FnOnce(Arc<Proc>) -> FutureBox,
{
    let id = EXECUTOR.next_id.fetch_add(1, Ordering::Relaxed);
    proc.task_id.store(id, Ordering::Relaxed);
    let future = future_fn(proc.clone());
    {
        let mut tasks = EXECUTOR.tasks.lock();
        while tasks.len() <= id as usize {
            tasks.push(None);
        }
        tasks[id as usize] = Some(Task { proc, future });
    }
    EXECUTOR.ready.lock().push_back(id);
    id
}

pub fn wake(id: TaskId) {
    EXECUTOR.ready.lock().push_back(id);
}

pub fn run() -> ! {
    // We enter `run` at the top of a kernel stack with no locks held —
    // either from kmain (where intr_on was already called) or from
    // `rust_usertrap` (where the hardware cleared sstatus.SIE on trap
    // entry). Force interrupts on so a wfi here can be woken by the
    // timer.
    unsafe { Arch::intr_on() };
    loop {
        let tid = EXECUTOR.ready.lock().pop_front();
        let Some(tid) = tid else {
            unsafe { Arch::wfi() };
            continue;
        };

        // Take the task out of the slot for exclusive ownership during poll
        // (so e.g. sys_fork can safely re-enter `spawn`, which locks tasks).
        let mut task = match EXECUTOR.tasks.lock().get_mut(tid as usize).and_then(|o| o.take()) {
            Some(t) => t,
            None => continue,
        };

        cpu::set_current_proc(Arc::as_ptr(&task.proc) as *mut _);

        let waker = task_waker(tid);
        let mut cx = Context::from_waker(&waker);
        let _ = task.future.as_mut().poll(&mut cx);

        // Put the task back BEFORE potentially noreturning into user mode.
        EXECUTOR.tasks.lock()[tid as usize] = Some(task);

        if let Some(target) = cpu::take_user_target() {
            // SAFETY: target points into a `Proc` owned by an `Arc` in
            // `EXECUTOR.tasks`. Procs live forever in Phase 4b/5a.
            unsafe {
                crate::usertrap::return_to_user(&*target);
            }
        }
    }
}

fn task_waker(tid: TaskId) -> Waker {
    let raw = RawWaker::new(tid as usize as *const (), &VTABLE);
    // SAFETY: VTABLE matches RawWaker contract (clone/wake idempotent,
    // wake_by_ref does not free, drop is a no-op since data is an integer).
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
