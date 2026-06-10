//! File abstraction backing the per-proc fd table.
//!
//! `Arc<File>` is the *open file description*: `fork`, `dup`, `dup2`
//! and `fcntl(F_DUPFD)` all share the same `Arc`, so the seek offset
//! is shared per POSIX. Per-fd state (`cloexec`, `nonblock`) lives in
//! `FdEntry`. The `Arc`'s strong count plays xv6's `struct file`
//! refcount role: `Drop for File` runs when the last fd anywhere
//! referencing the description closes — for pipes that's what
//! decrements the reader/writer count and lets the peer see
//! EOF / EPIPE.

use alloc::collections::VecDeque;
use alloc::sync::{Arc, Weak};
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, AtomicUsize, Ordering};

use crate::fs::inode::Inode;
use crate::sync::SpinLock;
use crate::wait::WakerList;

const PIPE_CAP: usize = 512;

pub struct PipeInner {
    pub buf: SpinLock<VecDeque<u8>>,
    /// Multi-waiter lists: after fork, several procs can hold fds on
    /// one pipe end and block simultaneously — a single-slot cell
    /// would drop all parked wakers but the last.
    pub read_waker: WakerList,
    pub write_waker: WakerList,
    pub readers: AtomicUsize,
    pub writers: AtomicUsize,
}

impl PipeInner {
    pub fn new() -> Self {
        Self {
            buf: SpinLock::new(VecDeque::with_capacity(PIPE_CAP)),
            read_waker: WakerList::new(),
            write_waker: WakerList::new(),
            readers: AtomicUsize::new(1),
            writers: AtomicUsize::new(1),
        }
    }

    pub fn cap(&self) -> usize {
        PIPE_CAP
    }
}

/// One entry in a `Proc`'s fd table. Bundles the underlying `File`
/// with per-fd flags: `cloexec` (O_CLOEXEC / FD_CLOEXEC — sys_exec
/// closes this entry) and `nonblock` (POSIX O_NONBLOCK — read/write
/// paths return -1 instead of awaiting when no progress can be made
/// immediately).
///
/// Flags live on the fd, not on the `File`, because `dup` and `fork`
/// produce fds that point to the same `File` but may carry
/// different flags.
pub struct FdEntry {
    pub file: Arc<File>,
    pub cloexec: bool,
    pub nonblock: bool,
}

impl FdEntry {
    pub fn new(file: Arc<File>) -> Self {
        Self { file, cloexec: false, nonblock: false }
    }
}

impl Clone for FdEntry {
    fn clone(&self) -> Self {
        // Shares the underlying File — one open file description,
        // shared offset (this is what fork/dup/F_DUPFD want). Per-fd
        // flags are copied.
        Self {
            file: Arc::clone(&self.file),
            cloexec: self.cloexec,
            nonblock: self.nonblock,
        }
    }
}

// ---------- AF_UNIX stream sockets ----------------------------------------
//
// A connected socket is just two pipes, one per direction; all the
// blocking, waker, EOF and EPIPE machinery is PipeInner's, verbatim.
// Each endpoint is the READER of `rx` and the WRITER of `tx`, so the
// per-direction reader/writer counts start at PipeInner::new()'s 1/1
// and the endpoint's Drop releases exactly its own side.

pub struct SockEnd {
    pub rx: Arc<PipeInner>,
    pub tx: Arc<PipeInner>,
}

impl Drop for SockEnd {
    fn drop(&mut self) {
        self.rx.readers.fetch_sub(1, Ordering::AcqRel);
        self.rx.write_waker.wake_all();
        self.tx.writers.fetch_sub(1, Ordering::AcqRel);
        self.tx.read_waker.wake_all();
    }
}

/// Build a connected pair of endpoints (the guts of socketpair /
/// connect+accept).
pub fn socket_conn_pair() -> (SockEnd, SockEnd) {
    let ab = Arc::new(PipeInner::new());
    let ba = Arc::new(PipeInner::new());
    (
        SockEnd { rx: Arc::clone(&ba), tx: Arc::clone(&ab) },
        SockEnd { rx: ab, tx: ba },
    )
}

pub const SOCK_BACKLOG_CAP: usize = 16;

pub struct Listener {
    /// Inum of the T_SOCK fs node this listener is bound to.
    pub inum: u32,
    /// Server-side endpoints queued by `connect`, waiting for accept.
    pub backlog: SpinLock<VecDeque<SockEnd>>,
    pub accept_waker: crate::wait::WakerList,
}

pub enum SockState {
    /// socket() done, neither bound nor connected yet.
    Fresh,
    Listening(Arc<Listener>),
    Connected(Arc<SockEnd>),
}

pub struct Socket {
    pub state: SpinLock<SockState>,
}

/// inum → listener bindings. Weak so a closed listener vanishes;
/// dead entries are purged on lookup.
static SOCK_REGISTRY: SpinLock<Vec<(u32, Weak<Listener>)>> =
    SpinLock::new(Vec::new());

pub fn sock_register(inum: u32, l: &Arc<Listener>) {
    SOCK_REGISTRY.lock().push((inum, Arc::downgrade(l)));
}

pub fn sock_lookup(inum: u32) -> Option<Arc<Listener>> {
    let mut reg = SOCK_REGISTRY.lock();
    let mut found = None;
    reg.retain(|(i, w)| match w.upgrade() {
        Some(l) => {
            if *i == inum && found.is_none() {
                found = Some(l);
            }
            true
        }
        None => false,
    });
    found
}

pub enum File {
    Console,
    PipeRead(Arc<PipeInner>),
    PipeWrite(Arc<PipeInner>),
    /// AF_UNIX stream socket (any state). Teardown is fully automatic:
    /// dropping the last Arc<Socket> drops the state — a Connected
    /// endpoint's SockEnd::Drop signals the peer; a dropped Listener
    /// drops its backlog, EOF-ing every un-accepted client.
    Socket(Arc<Socket>),
    /// On-disk file. The offset belongs to the open file description
    /// (this `File`) and is shared by every fd that fork/dup produced
    /// from the same `open`. Offset reads/updates happen inside the
    /// inode lock in `inode_read`/`inode_write` so concurrent sharers
    /// serialize.
    ///
    /// `append`: O_APPEND semantics — every write seeks to current
    /// end-of-file under the inode lock, so concurrent appenders
    /// never overwrite each other.
    Inode {
        ip: Arc<Inode>,
        off: AtomicU32,
        readable: bool,
        writable: bool,
        append: bool,
    },
}

// NOTE: `File` deliberately does NOT implement `Clone`. Cloning a
// `File` would mint a second open file description (private offset,
// double-counted pipe ends) — every duplication path must share the
// `Arc<File>` instead (see `FdEntry::clone`).

impl Drop for File {
    fn drop(&mut self) {
        match self {
            File::PipeRead(p) => {
                p.readers.fetch_sub(1, Ordering::AcqRel);
                p.write_waker.wake_all();
            }
            File::PipeWrite(p) => {
                p.writers.fetch_sub(1, Ordering::AcqRel);
                p.read_waker.wake_all();
            }
            File::Inode { ip, .. } => {
                // The last fd on this open file description just
                // closed. If the file was also unlinked, the disk
                // inode + data blocks must be freed (xv6's iput) —
                // defer to the reaper task, since Drop can't run a
                // log transaction. False positives are fine.
                crate::fs::inode::iput_deferred(Arc::clone(ip));
            }
            File::Console | File::Socket(_) => {}
        }
    }
}
