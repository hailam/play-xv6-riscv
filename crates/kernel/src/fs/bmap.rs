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

/// Allocate one free data block and return its block number.
/// Returns `0` if the disk is full (matches xv6's panic-on-balloc
/// behaviour — we propagate as an error instead).
pub async fn balloc(_dev: u32) -> Option<u32> {
    let sb = superblock::get();
    let mut b: u32 = 0;
    while b < sb.size {
        let blkno = sb.bmapstart + b / BPB;
        let buf = bio::bread(blkno).await;
        let mut found: Option<u32> = None;
        // Find a clear bit in this block (each block covers BPB blocks).
        let upto = BPB.min(sb.size - b);
        for bi in 0..upto {
            let m: u8 = 1u8 << (bi % 8);
            let byte_idx = (bi / 8) as usize;
            if buf.data()[byte_idx] & m == 0 {
                // Mark in-use.
                // Safety: we hold the only outstanding Arc to `buf`
                // here besides the cache; no concurrent mutator (no
                // other balloc/bfree races because all writes go
                // through this single bio-cache slot under the cache
                // lock acquired in bread).
                unsafe {
                    buf.data_mut()[byte_idx] |= m;
                }
                log::log_write(&buf);
                found = Some(b + bi);
                break;
            }
        }
        if let Some(blk) = found {
            // Zero the freshly-allocated block on disk before returning,
            // so callers see clean contents. Within the same transaction.
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
