//! Free-data-block bitmap (the `bmap` blocks in xv6's fs layout).
//!
//! Each bit in a bmap block represents one disk block:
//!   * bit = 0 → free
//!   * bit = 1 → in use
//!
//! `balloc` finds and claims the first free bit; `bfree` clears one.
//! Both must run inside an open log transaction (`begin_op` /
//! `end_op`) — they call `log::log_write` on the modified bmap block.

use xv6_fs_layout::{BPB, BSIZE};

use crate::driver::bio;
use crate::fs::log;
use crate::fs::superblock;
use crate::sync::SpinLock;

/// Serializes read-modify-write of bitmap blocks. `bread` hands out a
/// shared `Arc<Buffer>` with no exclusivity, so without this two tasks
/// on different harts could both observe a bit clear and claim the
/// same block (cross-linked files). Held only across the in-memory
/// scan/flip + `log_write` — no awaits inside.
static BITMAP_LOCK: SpinLock<()> = SpinLock::new(());

/// Allocate one free data block and return its block number.
/// Returns `None` if the disk is full (xv6 panics in balloc; we
/// propagate as an error instead).
pub async fn balloc(_dev: u32) -> Option<u32> {
    let sb = superblock::get();
    let mut b: u32 = 0;
    while b < sb.size {
        let blkno = sb.bmapstart + b / BPB;
        let buf = bio::bread(blkno).await;
        let mut found: Option<u32> = None;
        {
            // The scan-and-flip must be atomic w.r.t. other
            // balloc/bfree tasks — the buffer is shared.
            let _g = BITMAP_LOCK.lock();
            // Find a clear bit in this block (each block covers BPB blocks).
            let upto = BPB.min(sb.size - b);
            for bi in 0..upto {
                let m: u8 = 1u8 << (bi % 8);
                let byte_idx = (bi / 8) as usize;
                if buf.data()[byte_idx] & m == 0 {
                    // Mark in-use.
                    // Safety: BITMAP_LOCK serializes every mutator of
                    // bitmap blocks; concurrent readers tolerate the
                    // single-byte flip.
                    unsafe {
                        buf.data_mut()[byte_idx] |= m;
                    }
                    log::log_write(&buf);
                    found = Some(b + bi);
                    break;
                }
            }
        }
        if let Some(blk) = found {
            // Zero the freshly-allocated block on disk before returning,
            // so callers see clean contents. Within the same transaction.
            // Safety: the block was free until the flip above, so no
            // other task holds a reference to its contents.
            let zero = bio::bread(blk).await;
            unsafe {
                zero.data_mut().iter_mut().for_each(|x| *x = 0);
            }
            log::log_write(&zero);
            return Some(blk);
        }
        b += BPB;
    }
    None
}

/// Mark a previously-allocated data block as free.
pub async fn bfree(_dev: u32, b: u32) {
    let sb = superblock::get();
    let blkno = sb.bmapstart + b / BPB;
    let buf = bio::bread(blkno).await;
    let bi = b % BPB;
    let m: u8 = 1u8 << (bi % 8);
    let byte_idx = (bi / 8) as usize;
    let _g = BITMAP_LOCK.lock();
    assert!(
        buf.data()[byte_idx] & m != 0,
        "bfree: block {} was already free",
        b
    );
    unsafe {
        buf.data_mut()[byte_idx] &= !m;
    }
    log::log_write(&buf);
    let _ = BSIZE; // silence unused import in some builds
}
