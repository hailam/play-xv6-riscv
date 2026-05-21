//! Host-side `mkfs`: produces an fs.img with valid superblock, zeroed
//! log region, inode table, data bitmap, and a root directory populated
//! from the `name:path` command-line entries.
//!
//! On-disk types + constants live in `xv6-fs-layout` and are shared
//! with the kernel-side fs code.

use std::env;
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::mem::size_of;

use xv6_fs_layout::{
    DInode, Dirent, Superblock, BSIZE, DIRSIZ, FSMAGIC, IPB, LOGSIZE, NDIRECT, NINODES,
    T_DIR, T_FILE,
};

const FSSIZE: u32 = 2048; // 1 MiB
const NINODEBLOCKS: u32 = (NINODES + IPB - 1) / IPB;
const NBMAPBLOCKS: u32 = 1;

struct FsBuilder {
    img: File,
    sb: Superblock,
    next_inum: u32,
    next_block: u32,
    bitmap: Vec<u8>,
}

impl FsBuilder {
    fn new(path: &str) -> Self {
        let logstart = 2;
        let inodestart = logstart + 1 + LOGSIZE;
        let bmapstart = inodestart + NINODEBLOCKS;
        let datastart = bmapstart + NBMAPBLOCKS;

        let sb = Superblock {
            magic: FSMAGIC,
            size: FSSIZE,
            nblocks: FSSIZE - datastart,
            ninodes: NINODES,
            nlog: LOGSIZE,
            logstart,
            inodestart,
            bmapstart,
        };

        let img = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(path)
            .expect("create img");
        img.set_len(FSSIZE as u64 * BSIZE as u64).unwrap();

        let mut bitmap = vec![0u8; NBMAPBLOCKS as usize * BSIZE];
        for b in 0..datastart {
            bitmap[(b / 8) as usize] |= 1 << (b % 8);
        }

        let mut me = Self {
            img,
            sb,
            next_inum: 1,
            next_block: datastart,
            bitmap,
        };
        me.write_sb();
        me
    }

    fn write_sb(&mut self) {
        let mut buf = vec![0u8; BSIZE];
        let bytes = unsafe {
            std::slice::from_raw_parts(
                &self.sb as *const _ as *const u8,
                size_of::<Superblock>(),
            )
        };
        buf[..bytes.len()].copy_from_slice(bytes);
        self.write_block(1, &buf);
    }

    fn read_block(&mut self, blkno: u32) -> Vec<u8> {
        self.img
            .seek(SeekFrom::Start(blkno as u64 * BSIZE as u64))
            .unwrap();
        let mut buf = vec![0u8; BSIZE];
        self.img.read_exact(&mut buf).unwrap();
        buf
    }

    fn write_block(&mut self, blkno: u32, data: &[u8]) {
        assert_eq!(data.len(), BSIZE);
        self.img
            .seek(SeekFrom::Start(blkno as u64 * BSIZE as u64))
            .unwrap();
        self.img.write_all(data).unwrap();
    }

    fn alloc_block(&mut self) -> u32 {
        let b = self.next_block;
        self.next_block += 1;
        assert!(b < FSSIZE, "out of disk space");
        self.bitmap[(b / 8) as usize] |= 1 << (b % 8);
        b
    }

    fn read_inode(&mut self, inum: u32) -> DInode {
        let blkno = inum / IPB + self.sb.inodestart;
        let off = (inum % IPB) as usize * size_of::<DInode>();
        let buf = self.read_block(blkno);
        unsafe { std::ptr::read_unaligned(buf[off..].as_ptr() as *const DInode) }
    }

    fn write_inode(&mut self, inum: u32, dino: &DInode) {
        let blkno = inum / IPB + self.sb.inodestart;
        let off = (inum % IPB) as usize * size_of::<DInode>();
        let mut buf = self.read_block(blkno);
        let bytes = unsafe {
            std::slice::from_raw_parts(dino as *const _ as *const u8, size_of::<DInode>())
        };
        buf[off..off + bytes.len()].copy_from_slice(bytes);
        self.write_block(blkno, &buf);
    }

    fn alloc_inode(&mut self, typ: u16) -> u32 {
        let inum = self.next_inum;
        self.next_inum += 1;
        assert!(inum < NINODES, "inode table full");
        // POSIX default mode by type. Matches the kernel-side
        // `create_at_path` defaults so files in fs.img get the same
        // perms whether they were written by mkfs or by a running
        // user proc via open(O_CREATE).
        let mode = match typ {
            xv6_fs_layout::T_DIR => 0o755,
            xv6_fs_layout::T_FILE => 0o644,
            xv6_fs_layout::T_DEVICE => 0o666,
            _ => 0o644,
        };
        let dino = DInode {
            typ,
            nlink: 1,
            mode,
            ..DInode::default()
        };
        self.write_inode(inum, &dino);
        inum
    }

    fn write_file(&mut self, inum: u32, data: &[u8]) {
        let mut dino = self.read_inode(inum);
        dino.size = data.len() as u32;

        let nblocks = (data.len() + BSIZE - 1) / BSIZE;
        let mut indirect: Option<Vec<u8>> = None;

        for i in 0..nblocks {
            let blkno = self.alloc_block();
            let start = i * BSIZE;
            let end = (start + BSIZE).min(data.len());
            let mut block = vec![0u8; BSIZE];
            block[..end - start].copy_from_slice(&data[start..end]);
            self.write_block(blkno, &block);

            if i < NDIRECT {
                dino.addrs[i] = blkno;
            } else {
                if indirect.is_none() {
                    let ind_blk = self.alloc_block();
                    dino.addrs[NDIRECT] = ind_blk;
                    indirect = Some(vec![0u8; BSIZE]);
                }
                let idx = (i - NDIRECT) * 4;
                indirect.as_mut().unwrap()[idx..idx + 4]
                    .copy_from_slice(&(blkno as u32).to_le_bytes());
            }
        }

        if let Some(ind) = indirect {
            let ind_blk = dino.addrs[NDIRECT];
            self.write_block(ind_blk, &ind);
        }

        self.write_inode(inum, &dino);
    }

    fn dirlink(&mut self, dir_inum: u32, name: &str, target: u32) {
        let mut dir = self.read_inode(dir_inum);
        let entry_size = size_of::<Dirent>();
        let offset = dir.size as usize;
        let block_idx = offset / BSIZE;
        let block_off = offset % BSIZE;
        assert!(block_idx < NDIRECT, "directory needs indirect block (not implemented)");

        let blkno = if dir.addrs[block_idx] == 0 {
            let b = self.alloc_block();
            dir.addrs[block_idx] = b;
            self.write_block(b, &vec![0u8; BSIZE]);
            b
        } else {
            dir.addrs[block_idx]
        };

        let mut entry = Dirent::default();
        entry.inum = target as u16;
        let nb = name.as_bytes();
        let n = nb.len().min(DIRSIZ);
        entry.name[..n].copy_from_slice(&nb[..n]);

        let mut block = self.read_block(blkno);
        let bytes = unsafe {
            std::slice::from_raw_parts(&entry as *const _ as *const u8, entry_size)
        };
        block[block_off..block_off + entry_size].copy_from_slice(bytes);
        self.write_block(blkno, &block);

        dir.size += entry_size as u32;
        self.write_inode(dir_inum, &dir);
    }

    fn commit(&mut self) {
        for i in 0..(self.bitmap.len() / BSIZE) {
            let start = i * BSIZE;
            let end = start + BSIZE;
            let blkno = self.sb.bmapstart + i as u32;
            let chunk = self.bitmap[start..end].to_vec();
            self.write_block(blkno, &chunk);
        }
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("usage: mkfs <out.img> [name:path]...");
        std::process::exit(1);
    }
    let img_path = &args[1];
    let entries: Vec<(String, String)> = args[2..]
        .iter()
        .map(|s| {
            let (n, p) = s.split_once(':').expect("entry must be name:path");
            (n.to_string(), p.to_string())
        })
        .collect();

    let mut b = FsBuilder::new(img_path);

    println!(
        "mkfs: layout sb=1 log={}..{} inodes={}..{} bmap={} data={}..{}",
        b.sb.logstart,
        b.sb.logstart + 1 + b.sb.nlog,
        b.sb.inodestart,
        b.sb.inodestart + NINODEBLOCKS,
        b.sb.bmapstart,
        b.sb.bmapstart + NBMAPBLOCKS,
        b.sb.size,
    );

    let root = b.alloc_inode(T_DIR);
    assert_eq!(root, 1, "root inode must be 1");
    b.dirlink(root, ".", root);
    b.dirlink(root, "..", root);

    for (name, path) in &entries {
        let mut data = Vec::new();
        File::open(path)
            .unwrap_or_else(|e| panic!("open {}: {}", path, e))
            .read_to_end(&mut data)
            .unwrap();
        let inum = b.alloc_inode(T_FILE);
        b.write_file(inum, &data);
        b.dirlink(root, name, inum);
        println!("  /{} → inum {} ({} bytes)", name, inum, data.len());
    }

    b.commit();
    let final_size = std::fs::metadata(img_path).unwrap().len();
    println!("mkfs: {} written ({} bytes, {} blocks used)", img_path, final_size, b.next_block);
}
