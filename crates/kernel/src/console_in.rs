//! Console input ring buffer + reader waker. Pushed to by the UART RX
//! IRQ handler, drained by `sys_read` on fd 0.

use alloc::collections::VecDeque;
use core::task::Waker;

use crate::sync::SpinLock;
use crate::wait::WakerCell;

const CAP: usize = 256;

static BUF: SpinLock<VecDeque<u8>> = SpinLock::new(VecDeque::new());
static READER: WakerCell = WakerCell::new();

/// Push a byte from the IRQ handler. Drops bytes silently if the ring
/// is full (Phase 5d: we never expect that with shell-paced typing).
pub fn push(c: u8) {
    {
        let mut b = BUF.lock();
        if b.len() < CAP {
            b.push_back(c);
        }
    }
    READER.wake();
}

pub fn try_pop() -> Option<u8> {
    BUF.lock().pop_front()
}

pub fn register_waker(w: &Waker) {
    READER.register(w);
}
