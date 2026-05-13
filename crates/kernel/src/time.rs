//! Timer wheel. Phase 5b: a sorted `Vec<TimerEntry>` keyed by the raw
//! `time` CSR cycle deadline. Replaced by a proper heap / per-CPU
//! wheel once we measure it as a bottleneck.

use alloc::vec::Vec;
use core::task::Waker;

use hal::Hal;

use crate::arch::Arch;
use crate::sync::SpinLock;

struct TimerEntry {
    deadline: u64,
    waker: Waker,
}

static TIMERS: SpinLock<Vec<TimerEntry>> = SpinLock::new(Vec::new());

pub fn add_timer(deadline: u64, waker: Waker) {
    let mut timers = TIMERS.lock();
    // Insert in ascending-deadline order so drain can stop early.
    let pos = timers.partition_point(|e| e.deadline <= deadline);
    timers.insert(pos, TimerEntry { deadline, waker });
}

/// Called from timer trap handlers (kernel or user side). Wakes every
/// entry whose deadline has passed.
pub fn drain_expired() {
    let now = Arch::now_ticks();
    let mut timers = TIMERS.lock();
    while let Some(t) = timers.first() {
        if t.deadline > now {
            break;
        }
        let entry = timers.remove(0);
        entry.waker.wake();
    }
}
