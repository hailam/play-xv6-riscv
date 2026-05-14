//! Buffer cache. Phase 6c: a fixed-size pool of `Arc<Buffer>`, with
//! `async fn bread` that returns a buffer ready for use (cached or
//! freshly read via virtio). Concurrent `bread` calls for the same
//! block coalesce on the buffer's `io_waker` so only one disk I/O is
//! issued.
//!
//! No eviction yet — NBUF slots are populated lazily and never
//! reclaimed. Real eviction lands when the inode layer starts
//! pressuring the cache.

use alloc::sync::Arc;
use alloc::vec::Vec;
use core::cell::UnsafeCell;
use core::future::Future;
use core::pin::Pin;
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use core::task::{Context, Poll};

use crate::driver::virtio_blk;
use crate::sync::SpinLock;
use crate::wait::WakerCell;

pub const BSIZE: usize = 512;
const NBUF: usize = 32;
const BLOCK_INVALID: u32 = u32::MAX;

pub struct Buffer {
    pub block_no: AtomicU32,
    /// Raw data backing. Access via `data()` / `data_mut()`. Safe to
    /// read after `valid == true`; safe to write only by the I/O loader
    /// (under `loading == true`).
    data: UnsafeCell<[u8; BSIZE]>,
    pub valid: AtomicBool,
    pub loading: AtomicBool,
    /// Woken whenever `loading` transitions to false.
    pub io_waker: WakerCell,
}

unsafe impl Send for Buffer {}
unsafe impl Sync for Buffer {}

impl Buffer {
    const fn new() -> Self {
        Self {
            block_no: AtomicU32::new(BLOCK_INVALID),
            data: UnsafeCell::new([0; BSIZE]),
            valid: AtomicBool::new(false),
            loading: AtomicBool::new(false),
            io_waker: WakerCell::new(),
        }
    }

    pub fn data(&self) -> &[u8; BSIZE] {
        // Safety: caller is expected to have observed `valid == true`.
        unsafe { &*self.data.get() }
    }

    fn data_addr(&self) -> usize {
        self.data.get() as usize
    }
}

struct Cache {
    bufs: Vec<Arc<Buffer>>,
}

static CACHE: SpinLock<Cache> = SpinLock::new(Cache { bufs: Vec::new() });

pub fn init() {
    let mut cache = CACHE.lock();
    if cache.bufs.is_empty() {
        cache.bufs.reserve(NBUF);
        for _ in 0..NBUF {
            cache.bufs.push(Arc::new(Buffer::new()));
        }
    }
}

enum Role {
    Hit,
    Loader,
    Waiter,
}

/// Look up `block_no` in the cache. On hit, returns the buffer. On
/// miss, awaits an I/O (issuing one if no other task is already
/// loading the same block).
pub async fn bread(block_no: u32) -> Arc<Buffer> {
    loop {
        let (buf, role) = {
            let cache = CACHE.lock();
            // First: search for a valid hit.
            let mut hit = None;
            for (i, b) in cache.bufs.iter().enumerate() {
                if b.block_no.load(Ordering::Acquire) == block_no
                    && b.valid.load(Ordering::Acquire)
                {
                    hit = Some(i);
                    break;
                }
            }
            if let Some(i) = hit {
                (cache.bufs[i].clone(), Role::Hit)
            } else {
                // Look for an in-progress loader of the same block.
                let mut loading = None;
                for (i, b) in cache.bufs.iter().enumerate() {
                    if b.block_no.load(Ordering::Acquire) == block_no
                        && b.loading.load(Ordering::Acquire)
                    {
                        loading = Some(i);
                        break;
                    }
                }
                if let Some(i) = loading {
                    (cache.bufs[i].clone(), Role::Waiter)
                } else {
                    // Claim an idle slot.
                    let mut slot = None;
                    for (i, b) in cache.bufs.iter().enumerate() {
                        if !b.valid.load(Ordering::Acquire)
                            && !b.loading.load(Ordering::Acquire)
                        {
                            slot = Some(i);
                            break;
                        }
                    }
                    let i = slot.expect("bio: buffer cache full (no eviction yet)");
                    let buf = cache.bufs[i].clone();
                    buf.block_no.store(block_no, Ordering::Release);
                    buf.valid.store(false, Ordering::Release);
                    buf.loading.store(true, Ordering::Release);
                    (buf, Role::Loader)
                }
            }
        };

        match role {
            Role::Hit => return buf,
            Role::Loader => {
                let addr = buf.data_addr();
                virtio_blk::read_block_async(block_no as u64, addr)
                    .await
                    .expect("disk read failed");
                buf.valid.store(true, Ordering::Release);
                buf.loading.store(false, Ordering::Release);
                buf.io_waker.wake();
                return buf;
            }
            Role::Waiter => {
                LoadWait { buf: &buf }.await;
                // The loader marked valid=true (or failed and marked
                // loading=false). Loop and re-resolve.
                continue;
            }
        }
    }
}

struct LoadWait<'a> {
    buf: &'a Buffer,
}

impl Future for LoadWait<'_> {
    type Output = ();
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        // Register first, then check — closes the wake-loss race.
        self.buf.io_waker.register(cx.waker());
        if !self.buf.loading.load(Ordering::Acquire) {
            return Poll::Ready(());
        }
        Poll::Pending
    }
}
