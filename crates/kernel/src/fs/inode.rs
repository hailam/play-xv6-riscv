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
    /// Mutable access to the inode's in-memory state. Caller must
    /// remember to `iupdate` (or `writei` which calls it) before
    /// releasing the lock if disk needs to see the change.
    pub fn state_mut(&mut self) -> &mut InodeState {
        // Safety: `locked == true` and `&mut self` proves no aliasing
        // `&InodeState` is live through `state()`.
        unsafe { &mut *self.inode.state.get() }
    }
    #[allow(dead_code)] // exposed for completeness; current callers use `inum`/`dev`
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
///
/// Key invariant — matches xv6's `iget`: a "match by (dev, inum)" only
/// counts if **someone other than the cache is still holding it**
/// (`strong_count > 1`). If only the cache holds it, the slot is
/// considered free for reuse, even though `dev`/`inum` haven't been
/// cleared — and reuse re-stamps `valid = false`, forcing the next
/// `ilock` to reload from disk.
///
/// Without this, an inode that was unlinked + freed by `sys_unlink`,
/// then re-claimed by `ialloc` for a different file, would be
/// returned with stale in-memory state (`typ`, `addrs`, `size` from
/// the previous file). A subsequent `iupdate` would then write the
/// stale state back to disk, clobbering the new file's metadata —
/// which manifested as `usertests::copyin`'s `open(copyin1)` failing
/// on the second iteration of its open/unlink loop.
pub fn iget(dev: u32, inum: u32) -> Arc<Inode> {
    let cache = CACHE.lock();
    // Live entry — someone other than the cache holds it.
    for ip in cache.bufs.iter() {
        if Arc::strong_count(ip) > 1
            && ip.inum.load(Ordering::Acquire) == inum
            && ip.dev.load(Ordering::Acquire) == dev
        {
            return ip.clone();
        }
    }
    // No live holder. Reuse the first idle slot — re-stamping
    // valid=false so the next `ilock` re-reads disk state.
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

/// Like `bmap` but allocates the block (and an indirect block if
/// needed) when it isn't present. Returns `None` on disk-full.
/// Caller must hold an open log transaction.
async fn bmap_or_alloc(li: &mut LockedInode<'_>, bn: u32) -> Option<u32> {
    let dev = li.dev();
    if (bn as usize) < NDIRECT {
        let cur = li.state().addrs[bn as usize];
        if cur != 0 {
            return Some(cur);
        }
        let new_blk = crate::fs::bmap::balloc(dev).await?;
        li.state_mut().addrs[bn as usize] = new_blk;
        return Some(new_blk);
    }
    let idx = (bn as usize) - NDIRECT;
    assert!(idx < NINDIRECT, "bmap_or_alloc: file too big");
    let ind_blkno = li.state().addrs[NDIRECT];
    let ind_blkno = if ind_blkno != 0 {
        ind_blkno
    } else {
        let new_ind = crate::fs::bmap::balloc(dev).await?;
        li.state_mut().addrs[NDIRECT] = new_ind;
        new_ind
    };
    let ind_buf = bio::bread(ind_blkno).await;
    let o = idx * 4;
    let cur = u32::from_le_bytes(ind_buf.data()[o..o + 4].try_into().unwrap());
    if cur != 0 {
        return Some(cur);
    }
    let new_blk = crate::fs::bmap::balloc(dev).await?;
    unsafe {
        ind_buf.data_mut()[o..o + 4].copy_from_slice(&new_blk.to_le_bytes());
    }
    crate::fs::log::log_write(&ind_buf);
    Some(new_blk)
}

/// Write `src` into the inode starting at offset `off`, extending the
/// file if needed. Returns the number of bytes written (always
/// `src.len()` unless we hit disk-full or the per-file size cap).
/// Caller must hold an open log transaction.
pub async fn writei(li: &mut LockedInode<'_>, src: &[u8], off: u32) -> usize {
    let max_bytes = (xv6_fs_layout::MAXFILE as usize) * BSIZE;
    if off as usize >= max_bytes {
        return 0;
    }
    let want = src.len().min(max_bytes - off as usize);
    let mut tot = 0usize;
    let mut cur_off = off;
    while tot < want {
        let bn = cur_off / BSIZE as u32;
        let Some(blkno) = bmap_or_alloc(li, bn).await else {
            break;
        };
        let block_off = (cur_off as usize) % BSIZE;
        let chunk = (BSIZE - block_off).min(want - tot);
        let buf = bio::bread(blkno).await;
        unsafe {
            buf.data_mut()[block_off..block_off + chunk]
                .copy_from_slice(&src[tot..tot + chunk]);
        }
        crate::fs::log::log_write(&buf);
        tot += chunk;
        cur_off += chunk as u32;
    }
    if cur_off > li.state().size {
        li.state_mut().size = cur_off;
        iupdate(li).await;
    }
    tot
}

/// Flush the in-memory inode back to disk through the log.
/// Caller must hold an open log transaction.
pub async fn iupdate(li: &LockedInode<'_>) {
    let sb = superblock::get();
    let inum = li.inum();
    let blkno = inum / IPB + sb.inodestart;
    let off = (inum % IPB) as usize * core::mem::size_of::<DInode>();
    let buf = bio::bread(blkno).await;
    let s = li.state();
    let dino = DInode {
        typ: s.typ,
        major: s.major,
        minor: s.minor,
        nlink: s.nlink,
        size: s.size,
        addrs: s.addrs,
    };
    let bytes = unsafe {
        core::slice::from_raw_parts(
            &dino as *const _ as *const u8,
            core::mem::size_of::<DInode>(),
        )
    };
    unsafe {
        buf.data_mut()[off..off + bytes.len()].copy_from_slice(bytes);
    }
    crate::fs::log::log_write(&buf);
}

/// Free all data blocks owned by the file. Doesn't change typ/nlink —
/// caller decides whether to zero those. Caller must hold an open log
/// transaction. After this returns, the inode's in-memory `addrs` are
/// all zero and `size == 0`, and `iupdate` has been called to push
/// that to disk.
pub async fn itrunc(li: &mut LockedInode<'_>) {
    let dev = li.dev();
    let addrs = li.state().addrs;
    for i in 0..NDIRECT {
        if addrs[i] != 0 {
            crate::fs::bmap::bfree(dev, addrs[i]).await;
        }
    }
    if addrs[NDIRECT] != 0 {
        let ind_blkno = addrs[NDIRECT];
        let ind_buf = bio::bread(ind_blkno).await;
        for j in 0..NINDIRECT {
            let o = j * 4;
            let b = u32::from_le_bytes(ind_buf.data()[o..o + 4].try_into().unwrap());
            if b != 0 {
                crate::fs::bmap::bfree(dev, b).await;
            }
        }
        crate::fs::bmap::bfree(dev, ind_blkno).await;
    }
    let state = li.state_mut();
    state.addrs = [0; NDIRECT + 1];
    state.size = 0;
    iupdate(li).await;
}

/// Find a free on-disk inode slot, claim it with `typ`, and return an
/// `Arc<Inode>` for it. Caller must hold an open log transaction.
pub async fn ialloc(dev: u32, typ: u16) -> Option<Arc<Inode>> {
    let sb = superblock::get();
    for inum in 1..sb.ninodes {
        let blkno = inum / IPB + sb.inodestart;
        let off = (inum % IPB) as usize * core::mem::size_of::<DInode>();
        let buf = bio::bread(blkno).await;
        let mut dino = unsafe {
            core::ptr::read_unaligned(buf.data()[off..].as_ptr() as *const DInode)
        };
        if dino.typ == 0 {
            dino = DInode {
                typ,
                major: 0,
                minor: 0,
                nlink: 1,
                size: 0,
                addrs: [0; NDIRECT + 1],
            };
            let bytes = unsafe {
                core::slice::from_raw_parts(
                    &dino as *const _ as *const u8,
                    core::mem::size_of::<DInode>(),
                )
            };
            unsafe {
                buf.data_mut()[off..off + bytes.len()].copy_from_slice(bytes);
            }
            crate::fs::log::log_write(&buf);
            return Some(iget(dev, inum));
        }
    }
    None
}
