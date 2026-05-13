//! File abstraction backing the per-proc fd table.
//!
//! Phase 5e variants:
//!   * `Console` — fd 0/1/2 by default; read/write go through the
//!     UART-driven `console_in` ring (read) and `Arch::console_putc`
//!     (write).
//!   * `PipeRead` / `PipeWrite` — the two endpoints of `sys_pipe`,
//!     sharing one `PipeInner` ring buffer plus two wakers.

use alloc::collections::VecDeque;
use alloc::sync::Arc;
use core::sync::atomic::AtomicUsize;

use crate::sync::SpinLock;
use crate::wait::WakerCell;

const PIPE_CAP: usize = 512;

pub struct PipeInner {
    pub buf: SpinLock<VecDeque<u8>>,
    pub read_waker: WakerCell,
    pub write_waker: WakerCell,
    /// Tracked for future EOF semantics; not consulted yet in Phase 5e.
    #[allow(dead_code)]
    pub readers: AtomicUsize,
    #[allow(dead_code)]
    pub writers: AtomicUsize,
}

impl PipeInner {
    pub fn new() -> Self {
        Self {
            buf: SpinLock::new(VecDeque::with_capacity(PIPE_CAP)),
            read_waker: WakerCell::new(),
            write_waker: WakerCell::new(),
            readers: AtomicUsize::new(1),
            writers: AtomicUsize::new(1),
        }
    }

    pub fn cap(&self) -> usize {
        PIPE_CAP
    }
}

pub enum File {
    Console,
    PipeRead(Arc<PipeInner>),
    PipeWrite(Arc<PipeInner>),
}
