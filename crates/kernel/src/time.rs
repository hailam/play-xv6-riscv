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
    drop(timers);
    drain_expired_alarms(now);
}

// ---------- POSIX alarm(2) plumbing -----------------------------------

struct AlarmEntry {
    deadline: u64,
    target_pid: usize,
    /// Snapshot of `Proc::alarm_generation` at scheduling time. If
    /// the proc's current generation no longer matches, the alarm
    /// was cancelled / rescheduled — we drop without delivering.
    generation: u32,
}

static ALARMS: SpinLock<Vec<AlarmEntry>> = SpinLock::new(Vec::new());

pub fn add_alarm(deadline: u64, target_pid: usize, generation: u32) {
    let mut alarms = ALARMS.lock();
    let pos = alarms.partition_point(|e| e.deadline <= deadline);
    alarms.insert(pos, AlarmEntry { deadline, target_pid, generation });
}

fn drain_expired_alarms(now: u64) {
    // Pop expired entries from the sorted list, then deliver outside
    // the lock so signal delivery can grab whatever it needs.
    let mut due = Vec::new();
    {
        let mut alarms = ALARMS.lock();
        while let Some(e) = alarms.first() {
            if e.deadline > now {
                break;
            }
            due.push(alarms.remove(0));
        }
    }
    for e in due {
        if let Some(target) = crate::executor::find_proc_by_pid(e.target_pid) {
            // Skip if the alarm was rescheduled / cancelled since
            // we were queued.
            if target
                .alarm_generation
                .load(core::sync::atomic::Ordering::Acquire)
                != e.generation
            {
                continue;
            }
            // Match sys_kill's "user-handler installed → set pending"
            // path. If no handler is installed, SIGALRM's default
            // disposition is to terminate.
            use crate::uapi::{SIGALRM, SIG_DFL, SIG_IGN};
            let action = target.sig_actions.lock()[SIGALRM as usize];
            match action.handler {
                SIG_DFL => {
                    target
                        .killed
                        .store(true, core::sync::atomic::Ordering::Release);
                }
                SIG_IGN => {}
                _ => {
                    let bit = 1u32 << SIGALRM;
                    target
                        .sig_pending
                        .fetch_or(bit, core::sync::atomic::Ordering::AcqRel);
                }
            }
            // Wake whatever the proc is parked on.
            target.wait_waker.wake();
            crate::executor::wake(
                target.task_id.load(core::sync::atomic::Ordering::Relaxed),
            );
        }
    }
}
