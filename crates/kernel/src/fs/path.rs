//! Path → inode resolution. Relative paths resolve from the caller's
//! current working directory; absolute paths start at inode 1.

use alloc::string::String;
use alloc::sync::Arc;

use crate::fs::dir::dirlookup;
use crate::fs::inode::{iget, ilock, Inode};

/// Resolve `path` against `start`. If the path is absolute (starts
/// with `/`), `start` is ignored and the walk begins at inode 1.
/// Follows symlinks (including the final component).
pub async fn namei_from(start: Arc<Inode>, path: &str) -> Option<Arc<Inode>> {
    namex(start, path, false, true).await.map(|(ip, _)| ip)
}

/// Resolve `path` against root.
pub async fn namei(path: &str) -> Option<Arc<Inode>> {
    namei_from(iget(0, 1), path).await
}

/// Like `namei_from` but returns the symlink itself if the final
/// component is a symlink (no last-component follow). Used by
/// `lstat`, `readlink`, `unlink` of a symlink.
pub async fn namei_nofollow(
    start: Arc<Inode>,
    path: &str,
) -> Option<Arc<Inode>> {
    namex(start, path, false, false).await.map(|(ip, _)| ip)
}

pub async fn nameiparent_from(
    start: Arc<Inode>,
    path: &str,
) -> Option<(Arc<Inode>, String)> {
    namex(start, path, true, true).await.and_then(|(ip, n)| n.map(|s| (ip, s)))
}

pub async fn nameiparent(path: &str) -> Option<(Arc<Inode>, String)> {
    nameiparent_from(iget(0, 1), path).await
}

/// Symlink-resolution hop limit. POSIX recommends 40; we match.
const MAX_SYMLINK_HOPS: u32 = 40;

async fn namex(
    start: Arc<Inode>,
    path: &str,
    want_parent: bool,
    follow_last: bool,
) -> Option<(Arc<Inode>, Option<String>)> {
    let mut ip = if path.starts_with('/') {
        iget(0, 1)
    } else {
        start
    };
    let mut path_buf;
    let mut path = path;
    let mut hops = 0u32;

    loop {
        path = path.trim_start_matches('/');
        if path.is_empty() {
            break;
        }

        let (name, rest) = match path.find('/') {
            Some(i) => (&path[..i], &path[i + 1..]),
            None => (path, ""),
        };
        let is_last = rest.trim_start_matches('/').is_empty();

        let li = ilock(&ip).await;
        if li.state().typ != xv6_fs_layout::T_DIR {
            return None;
        }
        if want_parent && is_last {
            drop(li);
            return Some((ip, Some(String::from(name))));
        }
        let Some(next) = dirlookup(&li, name).await else {
            return None;
        };
        drop(li);

        // Symlink-follow step. Inspect `next` without holding `ip`'s
        // lock. If it's a symlink AND either it's not the last
        // component OR follow_last is true, read its target and
        // splice in.
        let next_typ = {
            let nli = ilock(&next).await;
            nli.state().typ
        };
        if next_typ == xv6_fs_layout::T_SYMLINK && (!is_last || follow_last) {
            hops += 1;
            if hops > MAX_SYMLINK_HOPS {
                return None;
            }
            // Read the symlink target.
            let target = read_symlink_body(&next).await?;
            // Splice: rebuild path = target + "/" + rest. If target
            // is absolute, restart from root; else keep current ip.
            path_buf = if rest.is_empty() {
                target
            } else {
                let mut s = String::with_capacity(target.len() + 1 + rest.len());
                s.push_str(&target);
                s.push('/');
                s.push_str(rest);
                s
            };
            if path_buf.starts_with('/') {
                ip = iget(0, 1);
            }
            // ip stays — the symlink is relative to the directory
            // that contained it (which is the current `ip` AFTER
            // dirlookup but BEFORE assignment). Actually we already
            // moved ip to the symlink's *parent*'s dirlookup result
            // → no: dirlookup returned the symlink itself, not its
            // parent. We need the symlink's parent. Reset to the
            // parent that contained `next` — which is `ip` at the
            // time we did `dirlookup`. That `ip` was the directory
            // we just walked into; we didn't reassign yet. So `ip`
            // currently holds the parent. Good.
            path = path_buf.as_str();
            continue;
        }
        ip = next;
        path = rest;
    }

    if want_parent {
        return None;
    }
    Some((ip, None))
}

/// Read a symlink's target string from its inode data. Targets are
/// stored as plain bytes in the inode's file payload; we cap at
/// 256 to keep things bounded.
async fn read_symlink_body(ip: &Arc<Inode>) -> Option<String> {
    use crate::fs::inode::readi;
    let li = ilock(ip).await;
    let size = li.state().size as usize;
    if size == 0 || size > 256 {
        return None;
    }
    let mut buf = alloc::vec![0u8; size];
    let n = readi(&li, &mut buf, 0).await;
    if n != size {
        return None;
    }
    drop(li);
    core::str::from_utf8(&buf).ok().map(String::from)
}
