//! Console input ring buffer + reader wakers. Pushed to by the UART RX
//! IRQ handler, drained by `sys_read` on fd 0.

use alloc::collections::VecDeque;
use core::task::Waker;

use crate::sync::SpinLock;
use crate::wait::WakerList;

const CAP: usize = 256;

static BUF: SpinLock<VecDeque<u8>> = SpinLock::new(VecDeque::new());
/// Multi-waiter: more than one proc can block reading the console
/// (e.g. parent and child both on fd 0) — wake them all and let the
/// losers re-park.
static READER: WakerList = WakerList::new();

/// Push a byte from the IRQ handler. Drops bytes silently if the ring
/// is full (Phase 5d: we never expect that with shell-paced typing).
pub fn push(c: u8) {
    {
        let mut b = BUF.lock();
        if b.len() < CAP {
            b.push_back(c);
        }
    }
    READER.wake_all();
}

pub fn try_pop() -> Option<u8> {
    BUF.lock().pop_front()
}

/// Number of unread bytes in the console queue (for FIONREAD).
pub fn pending() -> usize {
    BUF.lock().len()
}

pub fn register_waker(w: &Waker) {
    READER.register(w);
}

/// Boot the currently-parked readers (if any). Called by `sys_kill`
/// so a killed proc blocked in console read returns promptly.
pub fn wake() {
    READER.wake_all();
}
