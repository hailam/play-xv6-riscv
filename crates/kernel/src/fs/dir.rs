//! Directory operations. Phase 6.8 has only `dirlookup`; `dirlink`
//! (writes through log) comes with file-syscalls.

use alloc::sync::Arc;

use xv6_fs_layout::{Dirent, DIRSIZ, T_DIR};

use crate::fs::inode::{iget, readi, writei, Inode, LockedInode};

/// Look up `name` in directory `dir`. Returns the inode if found.
/// Caller must hold `dir`'s lock.
pub async fn dirlookup(dir: &LockedInode<'_>, name: &str) -> Option<Arc<Inode>> {
    assert_eq!(dir.state().typ, T_DIR, "dirlookup on non-directory");
    let entry_size = core::mem::size_of::<Dirent>() as u32;
    let mut off: u32 = 0;
    let dir_size = dir.state().size;
    let mut entry = Dirent::default();
    while off < dir_size {
        // Safety: copy into a freshly-default Dirent each iteration.
        let bytes = unsafe {
            core::slice::from_raw_parts_mut(
                &mut entry as *mut _ as *mut u8,
                entry_size as usize,
            )
        };
        let n = readi(dir, bytes, off).await;
        if n != entry_size as usize {
            break;
        }
        if entry.inum != 0 && dirent_name_matches(&entry, name) {
            return Some(iget(dir.dev(), entry.inum as u32));
        }
        off += entry_size;
    }
    None
}

fn dirent_name_matches(entry: &Dirent, name: &str) -> bool {
    let nb = name.as_bytes();
    if nb.len() > DIRSIZ {
        return false;
    }
    for i in 0..DIRSIZ {
        let want = nb.get(i).copied().unwrap_or(0);
        if entry.name[i] != want {
            return false;
        }
        if want == 0 {
            return true;
        }
    }
    // Filled the whole name field (no null terminator) and matched.
    nb.len() == DIRSIZ
}

/// Add `(name, inum)` to directory `dir`. Reuses the first free slot
/// (`inum == 0`); otherwise appends. Returns `false` if `name`
/// already exists. Caller must hold an open log transaction.
pub async fn dirlink(dir: &mut LockedInode<'_>, name: &str, inum: u16) -> bool {
    assert_eq!(dir.state().typ, T_DIR, "dirlink on non-directory");
    let entry_size = core::mem::size_of::<Dirent>() as u32;
    let dir_size = dir.state().size;
    let mut entry = Dirent::default();
    let mut free_off: Option<u32> = None;
    let mut off: u32 = 0;
    while off < dir_size {
        let bytes = unsafe {
            core::slice::from_raw_parts_mut(
                &mut entry as *mut _ as *mut u8,
                entry_size as usize,
            )
        };
        let n = readi(dir, bytes, off).await;
        if n != entry_size as usize {
            break;
        }
        if entry.inum == 0 {
            if free_off.is_none() {
                free_off = Some(off);
            }
        } else if dirent_name_matches(&entry, name) {
            return false; // already exists
        }
        off += entry_size;
    }
    let target_off = free_off.unwrap_or(dir_size);
    let mut new_entry = Dirent::default();
    new_entry.inum = inum;
    let nb = name.as_bytes();
    let n = nb.len().min(DIRSIZ);
    new_entry.name[..n].copy_from_slice(&nb[..n]);
    let bytes = unsafe {
        core::slice::from_raw_parts(
            &new_entry as *const _ as *const u8,
            entry_size as usize,
        )
    };
    let wrote = writei(dir, bytes, target_off).await;
    wrote == entry_size as usize
}

/// Clear directory entry at `off` (zero its `inum`). The slot becomes
/// reusable by `dirlink`. Caller must hold an open log transaction.
pub async fn dirunlink_at(dir: &mut LockedInode<'_>, off: u32) -> bool {
    let entry_size = core::mem::size_of::<Dirent>() as u32;
    let cleared = Dirent::default();
    let bytes = unsafe {
        core::slice::from_raw_parts(
            &cleared as *const _ as *const u8,
            entry_size as usize,
        )
    };
    let wrote = writei(dir, bytes, off).await;
    wrote == entry_size as usize
}

/// Find `name` in `dir`. Returns `(child_inode, dirent_offset)`.
pub async fn dirlookup_full(
    dir: &LockedInode<'_>,
    name: &str,
) -> Option<(Arc<Inode>, u32)> {
    let entry_size = core::mem::size_of::<Dirent>() as u32;
    let mut off: u32 = 0;
    let dir_size = dir.state().size;
    let mut entry = Dirent::default();
    while off < dir_size {
        let bytes = unsafe {
            core::slice::from_raw_parts_mut(
                &mut entry as *mut _ as *mut u8,
                entry_size as usize,
            )
        };
        let n = readi(dir, bytes, off).await;
        if n != entry_size as usize {
            break;
        }
        if entry.inum != 0 && dirent_name_matches(&entry, name) {
            return Some((iget(dir.dev(), entry.inum as u32), off));
        }
        off += entry_size;
    }
    None
}

/// Returns true iff the directory has any entries other than `.` and
/// `..` (used by `unlink` to refuse non-empty directories).
pub async fn dir_is_empty(dir: &LockedInode<'_>) -> bool {
    let entry_size = core::mem::size_of::<Dirent>() as u32;
    let mut off: u32 = 2 * entry_size; // skip . and ..
    let dir_size = dir.state().size;
    let mut entry = Dirent::default();
    while off < dir_size {
        let bytes = unsafe {
            core::slice::from_raw_parts_mut(
                &mut entry as *mut _ as *mut u8,
                entry_size as usize,
            )
        };
        let n = readi(dir, bytes, off).await;
        if n != entry_size as usize {
            break;
        }
        if entry.inum != 0 {
            return false;
        }
        off += entry_size;
    }
    true
}

/// Helper for iterating directory entries (used by the boot smoke test
/// to list `/`).
pub async fn for_each_entry<F: FnMut(u32, &str)>(
    dir: &LockedInode<'_>,
    mut visit: F,
) {
    assert_eq!(dir.state().typ, T_DIR);
    let entry_size = core::mem::size_of::<Dirent>() as u32;
    let mut off: u32 = 0;
    let dir_size = dir.state().size;
    let mut entry = Dirent::default();
    while off < dir_size {
        let bytes = unsafe {
            core::slice::from_raw_parts_mut(
                &mut entry as *mut _ as *mut u8,
                entry_size as usize,
            )
        };
        let n = readi(dir, bytes, off).await;
        if n != entry_size as usize {
            break;
        }
        if entry.inum != 0 {
            let end = entry.name.iter().position(|&c| c == 0).unwrap_or(DIRSIZ);
            let name = core::str::from_utf8(&entry.name[..end]).unwrap_or("?");
            visit(entry.inum as u32, name);
        }
        off += entry_size;
    }
}
