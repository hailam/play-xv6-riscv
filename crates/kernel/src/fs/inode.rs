//! Inode cache + ilock + readi (read path; writei is a follow-on).
//!
//! Each entry is an `Arc<Inode>`. Lookups are linear (the pool is
//! ~50 entries; xv6 does the same). The "sleeplock" that protects
//! mutable inode state is implemented with `locked: AtomicBool` +
//! `WakerCell` — `ilock(&ip).await` parks until the lock is free,
//! loads from disk if `!valid`, then hands back a `LockedInode<'a>`
//! RAII guard. Drop releases the lock and wakes any waiter.

use alloc::sync::Arc;
use alloc::vec::Vec;
use core::cell::UnsafeCell;
use core::future::Future;
use core::pin::Pin;
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use core::task::{Context, Poll};

use xv6_fs_layout::{DInode, BSIZE, IPB, NDIRECT, NINDIRECT};

use crate::driver::bio;
use crate::fs::superblock;
use crate::sync::SpinLock;
use crate::wait::WakerCell;

const NINODE_CACHE: usize = 50;
const INUM_FREE: u32 = u32::MAX;

#[derive(Clone, Copy, Default)]
pub struct InodeState {
    pub typ: u16,
    pub major: u16,
    pub minor: u16,
    pub nlink: u16,
    pub size: u32,
    pub addrs: [u32; NDIRECT + 1],
}

pub struct Inode {
    pub dev: AtomicU32,
    pub inum: AtomicU32,
    pub valid: AtomicBool,
    pub locked: AtomicBool,
    pub lock_waker: WakerCell,
    state: UnsafeCell<InodeState>,
}

// `state` is only accessed while `locked == true`, and `Inode` is
// otherwise just atomics. The aliasing rules are enforced by the
// `LockedInode` guard.
unsafe impl Send for Inode {}
unsafe impl Sync for Inode {}

impl Inode {
    const fn new_empty() -> Self {
        Self {
            dev: AtomicU32::new(0),
            inum: AtomicU32::new(INUM_FREE),
            valid: AtomicBool::new(false),
            locked: AtomicBool::new(false),
            lock_waker: WakerCell::new(),
            state: UnsafeCell::new(InodeState {
                typ: 0,
                major: 0,
                minor: 0,
                nlink: 0,
                size: 0,
                addrs: [0; NDIRECT + 1],
            }),
        }
    }
}

/// RAII guard returned by `ilock`. Read access via `state()`.
pub struct LockedInode<'a> {
    inode: &'a Arc<Inode>,
}

impl<'a> LockedInode<'a> {
    pub fn state(&self) -> &InodeState {
        // Safety: `locked == true` for the duration of self.
        unsafe { &*self.inode.state.get() }
    }
    pub fn inode(&self) -> &Arc<Inode> {
        self.inode
    }
    pub fn inum(&self) -> u32 {
        self.inode.inum.load(Ordering::Acquire)
    }
    pub fn dev(&self) -> u32 {
        self.inode.dev.load(Ordering::Acquire)
    }
}

impl<'a> Drop for LockedInode<'a> {
    fn drop(&mut self) {
        self.inode.locked.store(false, Ordering::Release);
        self.inode.lock_waker.wake();
    }
}

struct Cache {
    bufs: Vec<Arc<Inode>>,
}

static CACHE: SpinLock<Cache> = SpinLock::new(Cache { bufs: Vec::new() });

pub fn init_cache() {
    let mut cache = CACHE.lock();
    if cache.bufs.is_empty() {
        cache.bufs.reserve(NINODE_CACHE);
        for _ in 0..NINODE_CACHE {
            cache.bufs.push(Arc::new(Inode::new_empty()));
        }
    }
}

/// Get a cached `Arc<Inode>` for `(dev, inum)`. Doesn't lock or load;
/// follow up with `ilock(&ip).await` before reading state.
pub fn iget(dev: u32, inum: u32) -> Arc<Inode> {
    let cache = CACHE.lock();
    // Existing entry?
    for ip in cache.bufs.iter() {
        if ip.inum.load(Ordering::Acquire) == inum && ip.dev.load(Ordering::Acquire) == dev {
            return ip.clone();
        }
    }
    // Reuse first idle slot.
    for ip in cache.bufs.iter() {
        if Arc::strong_count(ip) == 1 {
            ip.dev.store(dev, Ordering::Release);
            ip.inum.store(inum, Ordering::Release);
            ip.valid.store(false, Ordering::Release);
            return ip.clone();
        }
    }
    panic!("inode cache full");
}

pub async fn ilock<'a>(ip: &'a Arc<Inode>) -> LockedInode<'a> {
    loop {
        if ip
            .locked
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            if !ip.valid.load(Ordering::Acquire) {
                load_from_disk(ip).await;
                ip.valid.store(true, Ordering::Release);
            }
            return LockedInode { inode: ip };
        }
        LockWait { ip }.await;
    }
}

struct LockWait<'a> {
    ip: &'a Inode,
}

impl Future for LockWait<'_> {
    type Output = ();
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        // Register first, then re-check (close the wake-loss race).
        self.ip.lock_waker.register(cx.waker());
        if !self.ip.locked.load(Ordering::Acquire) {
            return Poll::Ready(());
        }
        Poll::Pending
    }
}

async fn load_from_disk(ip: &Arc<Inode>) {
    let inum = ip.inum.load(Ordering::Acquire);
    let sb = superblock::get();
    let blkno = inum / IPB + sb.inodestart;
    let off = (inum % IPB) as usize * core::mem::size_of::<DInode>();
    let buf = bio::bread(blkno).await;
    let dino = unsafe {
        core::ptr::read_unaligned(buf.data()[off..].as_ptr() as *const DInode)
    };
    let new_state = InodeState {
        typ: dino.typ,
        major: dino.major,
        minor: dino.minor,
        nlink: dino.nlink,
        size: dino.size,
        addrs: dino.addrs,
    };
    // Safety: we hold the lock (locked == true) for the duration of
    // this call — exclusive access.
    unsafe { *ip.state.get() = new_state };
    assert!(new_state.typ != 0, "ilock: empty inode {}", inum);
}

/// Read up to `dst.len()` bytes starting at `off` from the inode's
/// data. Returns the number of bytes copied (may be less than
/// `dst.len()` if EOF is hit). Caller must hold the inode lock.
pub async fn readi(li: &LockedInode<'_>, dst: &mut [u8], off: u32) -> usize {
    let size = li.state().size;
    if off >= size {
        return 0;
    }
    let limit = (dst.len() as u32).min(size - off) as usize;
    let mut tot = 0usize;
    let mut cur_off = off;
    while tot < limit {
        let blkno = bmap(li, cur_off / BSIZE as u32).await;
        let buf = bio::bread(blkno).await;
        let block_off = (cur_off as usize) % BSIZE;
        let chunk = (BSIZE - block_off).min(limit - tot);
        dst[tot..tot + chunk].copy_from_slice(&buf.data()[block_off..block_off + chunk]);
        tot += chunk;
        cur_off += chunk as u32;
    }
    tot
}

async fn bmap(li: &LockedInode<'_>, bn: u32) -> u32 {
    let state_addrs = li.state().addrs;
    if (bn as usize) < NDIRECT {
        return state_addrs[bn as usize];
    }
    let idx = (bn as usize) - NDIRECT;
    assert!(idx < NINDIRECT, "bmap: file too big");
    let ind_blkno = state_addrs[NDIRECT];
    assert!(ind_blkno != 0, "bmap: missing indirect block for bn={}", bn);
    let ind_buf = bio::bread(ind_blkno).await;
    let o = idx * 4;
    u32::from_le_bytes(ind_buf.data()[o..o + 4].try_into().unwrap())
}
