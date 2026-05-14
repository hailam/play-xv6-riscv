//! Write-ahead log.
//!
//! Ported from xv6's `kernel/log.c`. A transaction is opened with
//! `begin_op().await`; each modified buffer is recorded via
//! `log_write(&buf)`. `end_op().await` either accumulates with other
//! in-flight ops, or — if it's the last one — runs `commit()`:
//!
//! 1. Copy every pinned buffer's contents into the corresponding log
//!    slot on disk (`log_start + 1 + i`).
//! 2. Write the log header to `log_start`. **This is the commit point.**
//!    Anything past this is durable.
//! 3. `bwrite` every pinned buffer to its home block. (Cached buffer
//!    already has the modifications.)
//! 4. Zero the on-disk log header.
//!
//! Recovery on boot: if the header on disk shows `n > 0`, that means
//! we crashed between steps 2 and 4 of a previous commit. Step 3 may
//! or may not have completed; replay it in full. If `n == 0` there's
//! nothing to do (we either never committed or finished cleanly).

use alloc::sync::Arc;
use alloc::vec::Vec;
use core::future::Future;
use core::pin::Pin;
use core::sync::atomic::Ordering;
use core::task::{Context, Poll};

use crate::driver::bio::{self, Buffer, BSIZE};
use crate::sync::SpinLock;
use crate::wait::WakerCell;

pub const LOGSIZE: usize = 30;
/// Max blocks any one fs operation may write. `begin_op` refuses to
/// proceed if there isn't headroom for one more op's worth.
pub const MAXOPBLOCKS: u32 = 10;

#[derive(Clone)]
struct LogHeader {
    n: u32,
    block: [u32; LOGSIZE],
}

impl LogHeader {
    const fn zero() -> Self {
        Self {
            n: 0,
            block: [0; LOGSIZE],
        }
    }
}

struct LogState {
    start: u32,
    #[allow(dead_code)]
    size: u32,
    outstanding: u32,
    committing: bool,
    initialized: bool,
    lh: LogHeader,
    /// Buffers pinned by `log_write` calls in the current transaction.
    /// Their `Arc` here keeps them in the bio cache (refcount >= 2) so
    /// they can't be evicted while the commit is in flight.
    pinned: Vec<Arc<Buffer>>,
}

impl LogState {
    const fn zero() -> Self {
        Self {
            start: 0,
            size: 0,
            outstanding: 0,
            committing: false,
            initialized: false,
            lh: LogHeader::zero(),
            pinned: Vec::new(),
        }
    }
}

static LOG: SpinLock<LogState> = SpinLock::new(LogState::zero());
/// One-shot waker. Phase 6.5: only one task should be parked here at a
/// time (`WakerCell` overwrites on second registration). Multi-waiter
/// queueing is a follow-up.
static COMMIT_WAKER: WakerCell = WakerCell::new();

// ---------- init + recovery ------------------------------------------------

/// Initialize the log at the given disk location, run recovery once.
/// Call from a kernel task at boot.
pub async fn init(log_start: u32, log_size: u32) {
    {
        let mut log = LOG.lock();
        log.start = log_start;
        log.size = log_size;
        log.initialized = true;
    }
    recover().await;
}

async fn recover() {
    let lh = read_log_header().await;
    if lh.n == 0 {
        return;
    }
    let log_start = LOG.lock().start;
    crate::println!("log: replaying {} blocks", lh.n);
    install_log_to_home(log_start, &lh).await;
    clear_log_header().await;
}

async fn install_log_to_home(log_start: u32, lh: &LogHeader) {
    for i in 0..lh.n as usize {
        let log_buf = bio::bread(log_start + 1 + i as u32).await;
        let dst_buf = bio::bread(lh.block[i]).await;
        // Safety: we hold the only outstanding `Arc` to each via the
        // local binding above; no concurrent reader.
        unsafe {
            dst_buf.data_mut().copy_from_slice(log_buf.data());
        }
        bio::bwrite(&dst_buf).await.expect("recover: write home");
    }
}

// ---------- begin_op / log_write / end_op ----------------------------------

pub async fn begin_op() {
    loop {
        let ready = {
            let mut log = LOG.lock();
            assert!(log.initialized, "log used before init");
            if log.committing {
                false
            } else if log.lh.n + (log.outstanding + 1) * MAXOPBLOCKS > LOGSIZE as u32 {
                // Not enough log space for another op of MAXOPBLOCKS
                // size. Wait for the next commit to free space.
                false
            } else {
                log.outstanding += 1;
                true
            }
        };
        if ready {
            return;
        }
        WaitCommit.await;
    }
}

struct WaitCommit;
impl Future for WaitCommit {
    type Output = ();
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        // Register first, then re-check — closes the wake-loss race.
        COMMIT_WAKER.register(cx.waker());
        let log = LOG.lock();
        let busy = log.committing
            || log.lh.n + MAXOPBLOCKS > LOGSIZE as u32;
        if !busy {
            return Poll::Ready(());
        }
        Poll::Pending
    }
}

/// Record a modified buffer as part of the current transaction.
/// Absorbs duplicates (same block_no recorded twice in one op).
pub fn log_write(buf: &Arc<Buffer>) {
    let block_no = buf.block_no.load(Ordering::Acquire);
    let mut log = LOG.lock();
    assert!(log.outstanding > 0, "log_write outside begin_op/end_op");
    assert!((log.lh.n as usize) < LOGSIZE, "log overflow");
    // Already recorded?
    for i in 0..log.lh.n as usize {
        if log.lh.block[i] == block_no {
            return;
        }
    }
    let i = log.lh.n as usize;
    log.lh.block[i] = block_no;
    log.lh.n += 1;
    log.pinned.push(buf.clone());
}

pub async fn end_op() {
    let do_commit = {
        let mut log = LOG.lock();
        log.outstanding -= 1;
        assert!(!log.committing, "end_op while committing");
        if log.outstanding == 0 {
            log.committing = true;
            true
        } else {
            // Some headroom may have just become available (no, only
            // commit frees the log). But wake waiters defensively.
            COMMIT_WAKER.wake();
            false
        }
    };
    if do_commit {
        commit().await;
        {
            let mut log = LOG.lock();
            log.committing = false;
        }
        COMMIT_WAKER.wake();
    }
}

// ---------- commit ---------------------------------------------------------

async fn commit() {
    let (lh, pinned, log_start) = {
        let log = LOG.lock();
        (log.lh.clone(), log.pinned.clone(), log.start)
    };
    if lh.n == 0 {
        return;
    }

    // 1. Copy each pinned buf into the corresponding log slot.
    for i in 0..lh.n as usize {
        let log_buf = bio::bread(log_start + 1 + i as u32).await;
        // Safety: we own the only outstanding non-cache Arc to log_buf
        // here, and pinned[i] is read-only via &.
        unsafe {
            log_buf.data_mut().copy_from_slice(pinned[i].data());
        }
        bio::bwrite(&log_buf).await.expect("commit: write log block");
    }

    // 2. Write the log header. *** COMMIT POINT ***
    write_log_header(&lh).await;

    // 3. Copy log entries to home blocks. The pinned bufs already hold
    // the modifications in the bio cache — just flush them.
    for buf in pinned.iter() {
        bio::bwrite(buf).await.expect("commit: write home block");
    }

    // 4. Zero the on-disk header — marks the log empty.
    clear_log_header().await;

    {
        let mut log = LOG.lock();
        log.lh = LogHeader::zero();
        log.pinned.clear();
    }
}

// ---------- header (de)serialization --------------------------------------

async fn read_log_header() -> LogHeader {
    let log_start = LOG.lock().start;
    let buf = bio::bread(log_start).await;
    let data = buf.data();
    let n_raw = u32::from_le_bytes(data[..4].try_into().unwrap());
    let mut lh = LogHeader::zero();
    lh.n = n_raw.min(LOGSIZE as u32);
    for i in 0..lh.n as usize {
        let off = 4 + i * 4;
        lh.block[i] = u32::from_le_bytes(data[off..off + 4].try_into().unwrap());
    }
    lh
}

async fn write_log_header(lh: &LogHeader) {
    let log_start = LOG.lock().start;
    let buf = bio::bread(log_start).await;
    unsafe {
        let dst = buf.data_mut();
        dst.iter_mut().for_each(|b| *b = 0);
        dst[..4].copy_from_slice(&lh.n.to_le_bytes());
        for i in 0..lh.n as usize {
            let off = 4 + i * 4;
            dst[off..off + 4].copy_from_slice(&lh.block[i].to_le_bytes());
        }
    }
    bio::bwrite(&buf).await.expect("write log header");
}

async fn clear_log_header() {
    write_log_header(&LogHeader::zero()).await;
}

// Compile-time check that the header fits in one block.
const _: () = {
    let header_bytes = 4 + 4 * LOGSIZE;
    assert!(header_bytes <= BSIZE, "log header exceeds block size");
};
