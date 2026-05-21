//! Shared on-disk filesystem layout. Pulled in by both the kernel's
//! `fs/` module and the host `mkfs/` binary so the two stay in lockstep.

#![no_std]

pub const BSIZE: usize = 512;
pub const FSMAGIC: u32 = 0x10203040;
pub const NINODES: u32 = 200;
pub const NDIRECT: usize = 12;
pub const NINDIRECT: usize = BSIZE / 4;
pub const MAXFILE: u32 = (NDIRECT + NINDIRECT) as u32;
pub const LOGSIZE: u32 = 30;
pub const DIRSIZ: usize = 14;

pub const T_DIR: u16 = 1;
pub const T_FILE: u16 = 2;
pub const T_DEVICE: u16 = 3;

/// On-disk filesystem superblock; lives at block 1.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Superblock {
    pub magic: u32,
    pub size: u32,
    pub nblocks: u32,
    pub ninodes: u32,
    pub nlog: u32,
    pub logstart: u32,
    pub inodestart: u32,
    pub bmapstart: u32,
}

impl Superblock {
    pub const fn zero() -> Self {
        Self {
            magic: 0,
            size: 0,
            nblocks: 0,
            ninodes: 0,
            nlog: 0,
            logstart: 0,
            inodestart: 0,
            bmapstart: 0,
        }
    }
}

/// On-disk inode (packed into inode blocks). 128 bytes each — bumped
/// from xv6's 64 to make room for POSIX mode/uid/gid plus reserved
/// space for future timestamps. IPB = BSIZE/128 = 4.
///
/// Layout:
///   typ/major/minor/nlink/mode/uid/gid + 1 reserved u16  =  16 B
///   size + 3 reserved u32 (atime/mtime/ctime placeholder) =  16 B
///   addrs [u32; 13]                                       =  52 B
///   _reserved [u8; 44]                                    =  44 B
///                                                          ----
///                                                          128 B
#[repr(C)]
#[derive(Clone, Copy)]
pub struct DInode {
    pub typ: u16,
    pub major: u16,
    pub minor: u16,
    pub nlink: u16,
    pub mode: u16,
    pub uid: u16,
    pub gid: u16,
    pub _reserved0: u16,
    pub size: u32,
    pub _reserved_time: [u32; 3],
    pub addrs: [u32; NDIRECT + 1],
    pub _reserved_tail: [u8; 44],
}

impl Default for DInode {
    fn default() -> Self {
        Self {
            typ: 0,
            major: 0,
            minor: 0,
            nlink: 0,
            mode: 0,
            uid: 0,
            gid: 0,
            _reserved0: 0,
            size: 0,
            _reserved_time: [0; 3],
            addrs: [0; NDIRECT + 1],
            _reserved_tail: [0; 44],
        }
    }
}

/// Directory entry: 16 bytes.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Dirent {
    pub inum: u16,
    pub name: [u8; DIRSIZ],
}

impl Default for Dirent {
    fn default() -> Self {
        Self {
            inum: 0,
            name: [0; DIRSIZ],
        }
    }
}

pub const IPB: u32 = (BSIZE / core::mem::size_of::<DInode>()) as u32;
pub const BPB: u32 = (BSIZE * 8) as u32;

/// Block number containing inode `inum`.
#[inline]
pub const fn iblock(inum: u32, sb: &Superblock) -> u32 {
    inum / IPB + sb.inodestart
}

const _: () = {
    assert!(core::mem::size_of::<Superblock>() <= BSIZE);
    assert!(core::mem::size_of::<DInode>() == 128);
    assert!(core::mem::size_of::<Dirent>() == 16);
    assert!(BSIZE % core::mem::size_of::<DInode>() == 0);
};
