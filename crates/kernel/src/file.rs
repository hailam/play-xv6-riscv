//! File abstraction backing the per-proc fd table.
//!
//! Each fd has its **own** `Arc<File>` (strong_count == 1 per fd).
//! `File::Clone` is the operation `fork` and `dup` use to create a new
//! fd that shares a pipe end with an existing fd; it bumps the pipe's
//! reader/writer count. `File::Drop` (which runs when the last
//! reference to *that fd's* `Arc<File>` is gone, i.e. when the fd
//! closes) decrements the count. With these in lockstep, the count
//! tracks "number of fds currently open on this pipe end" — and
//! readers can spot writer == 0 to return EOF.

use alloc::collections::VecDeque;
use alloc::sync::Arc;
use core::sync::atomic::{AtomicU32, AtomicUsize, Ordering};

use crate::fs::inode::Inode;
use crate::sync::SpinLock;
use crate::wait::WakerCell;

const PIPE_CAP: usize = 512;

pub struct PipeInner {
    pub buf: SpinLock<VecDeque<u8>>,
    pub read_waker: WakerCell,
    pub write_waker: WakerCell,
    pub readers: AtomicUsize,
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
    /// On-disk file. Each fd has its own seek offset; the underlying
    /// inode is shared via `Arc<Inode>` (the inode cache holds another
    /// strong ref).
    Inode {
        ip: Arc<Inode>,
        off: AtomicU32,
        readable: bool,
        writable: bool,
    },
}

impl Clone for File {
    fn clone(&self) -> Self {
        match self {
            File::Console => File::Console,
            File::PipeRead(p) => {
                p.readers.fetch_add(1, Ordering::AcqRel);
                File::PipeRead(Arc::clone(p))
            }
            File::PipeWrite(p) => {
                p.writers.fetch_add(1, Ordering::AcqRel);
                File::PipeWrite(Arc::clone(p))
            }
            File::Inode {
                ip,
                off,
                readable,
                writable,
            } => File::Inode {
                ip: Arc::clone(ip),
                off: AtomicU32::new(off.load(Ordering::Acquire)),
                readable: *readable,
                writable: *writable,
            },
        }
    }
}

impl Drop for File {
    fn drop(&mut self) {
        match self {
            File::PipeRead(p) => {
                p.readers.fetch_sub(1, Ordering::AcqRel);
                p.write_waker.wake();
            }
            File::PipeWrite(p) => {
                p.writers.fetch_sub(1, Ordering::AcqRel);
                p.read_waker.wake();
            }
            File::Console | File::Inode { .. } => {}
        }
    }
}
