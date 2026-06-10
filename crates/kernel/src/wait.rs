//! One-shot waker slot. Holds at most one parked `Waker` for a resource
//! (a proc's exit notification, a pipe's reader, etc.). Writer to the
//! slot is the resource owner; reader is the `Future::poll` of the
//! parking task.

use core::task::Waker;

use alloc::vec::Vec;

use crate::sync::SpinLock;

pub struct WakerCell {
    inner: SpinLock<Option<Waker>>,
}

impl WakerCell {
    pub const fn new() -> Self {
        Self { inner: SpinLock::new(None) }
    }

    /// Park a waker on this slot, replacing any prior one (last writer
    /// wins). Returns immediately.
    pub fn register(&self, w: &Waker) {
        let mut g = self.inner.lock();
        // Avoid clone churn if the same waker is already here.
        if g.as_ref().map_or(false, |existing| existing.will_wake(w)) {
            return;
        }
        *g = Some(w.clone());
    }

    /// Wake any parked waker. No-op if empty. Subsequent calls before
    /// a re-register also no-op (slot is one-shot).
    pub fn wake(&self) {
        let w = self.inner.lock().take();
        if let Some(w) = w {
            w.wake();
        }
    }
}

/// Multi-waiter version of [`WakerCell`]: parks any number of distinct
/// wakers and wakes them all. Needed where several tasks can block on a
/// single shared condition at once — e.g. the fs log's commit, where
/// multiple procs may be parked in `begin_op` waiting for log space.
/// (`WakerCell` overwrites on the second registration, so it would drop
/// all but the last waiter and hang the rest.)
pub struct WakerList {
    inner: SpinLock<Vec<Waker>>,
}

impl WakerList {
    pub const fn new() -> Self {
        Self { inner: SpinLock::new(Vec::new()) }
    }

    /// Park `w`. Idempotent for an already-parked waker (deduped via
    /// `will_wake`), so re-polling the same future doesn't grow the list.
    pub fn register(&self, w: &Waker) {
        let mut g = self.inner.lock();
        if g.iter().any(|existing| existing.will_wake(w)) {
            return;
        }
        g.push(w.clone());
    }

    /// Wake every parked waker and clear the list. Woken tasks re-park on
    /// their next poll if their condition is still unmet.
    pub fn wake_all(&self) {
        let wakers = core::mem::take(&mut *self.inner.lock());
        for w in wakers {
            w.wake();
        }
    }
}

/// Yield to the executor once: re-queues the current task and returns
/// `Pending` a single time. Use for retry loops on conditions that have
/// no waker hook (e.g. waiting for some other task to drop an
/// `Arc<Buffer>` — releases happen at await points, so one round of the
/// ready queue is enough to make progress).
pub async fn yield_now() {
    struct YieldNow(bool);
    impl core::future::Future for YieldNow {
        type Output = ();
        fn poll(
            mut self: core::pin::Pin<&mut Self>,
            cx: &mut core::task::Context<'_>,
        ) -> core::task::Poll<()> {
            if self.0 {
                core::task::Poll::Ready(())
            } else {
                self.0 = true;
                cx.waker().wake_by_ref();
                core::task::Poll::Pending
            }
        }
    }
    YieldNow(false).await
}
