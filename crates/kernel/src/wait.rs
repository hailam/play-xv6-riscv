//! One-shot waker slot. Holds at most one parked `Waker` for a resource
//! (a proc's exit notification, a pipe's reader, etc.). Writer to the
//! slot is the resource owner; reader is the `Future::poll` of the
//! parking task.

use core::task::Waker;

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
