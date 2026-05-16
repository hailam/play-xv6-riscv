//! Superblock cache. Read once at boot from block 1.

use core::sync::atomic::Ordering;

use xv6_fs_layout::{Superblock, FSMAGIC};

use crate::driver::bio;
use crate::sync::SpinLock;

static SB: SpinLock<Superblock> = SpinLock::new(Superblock::zero());

pub async fn init() {
    let buf = bio::bread(1).await;
    let sb = unsafe { core::ptr::read_unaligned(buf.data().as_ptr() as *const Superblock) };
    assert_eq!(sb.magic, FSMAGIC, "fs: bad superblock magic {:#x}", { sb.magic });
    *SB.lock() = sb;
    let _ = Ordering::Release; // silence unused import in some builds
}

pub fn get() -> Superblock {
    *SB.lock()
}
