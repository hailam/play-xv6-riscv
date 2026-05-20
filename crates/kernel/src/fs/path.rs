//! Path → inode resolution. Relative paths resolve from the caller's
//! current working directory; absolute paths start at inode 1.

use alloc::string::String;
use alloc::sync::Arc;

use crate::fs::dir::dirlookup;
use crate::fs::inode::{iget, ilock, Inode};

/// Resolve `path` against `start`. If the path is absolute (starts
/// with `/`), `start` is ignored and the walk begins at inode 1.
pub async fn namei_from(start: Arc<Inode>, path: &str) -> Option<Arc<Inode>> {
    namex(start, path, false).await.map(|(ip, _)| ip)
}

/// Resolve `path` against root.
pub async fn namei(path: &str) -> Option<Arc<Inode>> {
    namei_from(iget(0, 1), path).await
}

pub async fn nameiparent_from(
    start: Arc<Inode>,
    path: &str,
) -> Option<(Arc<Inode>, String)> {
    namex(start, path, true).await.and_then(|(ip, n)| n.map(|s| (ip, s)))
}

pub async fn nameiparent(path: &str) -> Option<(Arc<Inode>, String)> {
    nameiparent_from(iget(0, 1), path).await
}

async fn namex(
    start: Arc<Inode>,
    path: &str,
    want_parent: bool,
) -> Option<(Arc<Inode>, Option<String>)> {
    let mut ip = if path.starts_with('/') {
        iget(0, 1)
    } else {
        start
    };
    let mut path = path;

    loop {
        path = path.trim_start_matches('/');
        if path.is_empty() {
            break;
        }

        let (name, rest) = match path.find('/') {
            Some(i) => (&path[..i], &path[i + 1..]),
            None => (path, ""),
        };

        let li = ilock(&ip).await;
        if li.state().typ != xv6_fs_layout::T_DIR {
            return None;
        }
        if want_parent && rest.trim_start_matches('/').is_empty() {
            drop(li);
            return Some((ip, Some(String::from(name))));
        }
        let Some(next) = dirlookup(&li, name).await else {
            return None;
        };
        drop(li);
        ip = next;
        path = rest;
    }

    if want_parent {
        return None;
    }
    Some((ip, None))
}
