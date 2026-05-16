//! Path → inode resolution.

use alloc::string::String;
use alloc::sync::Arc;

use crate::fs::dir::dirlookup;
use crate::fs::inode::{iget, ilock, Inode};

/// Resolve a path to an `Arc<Inode>`. Absolute paths start at inode 1.
pub async fn namei(path: &str) -> Option<Arc<Inode>> {
    namex(path, false).await.map(|(ip, _)| ip)
}

/// Resolve a path's parent directory + the final component's name
/// (without crossing into the final component). Used by syscalls that
/// create (e.g. `mkdir`, `create`).
#[allow(dead_code)]
pub async fn nameiparent(path: &str) -> Option<(Arc<Inode>, String)> {
    namex(path, true).await.and_then(|(ip, n)| n.map(|s| (ip, s)))
}

async fn namex(mut path: &str, want_parent: bool) -> Option<(Arc<Inode>, Option<String>)> {
    // Always start from root for now. Per-proc cwd lands in the
    // file-syscall phase.
    let mut ip = iget(0, 1);

    loop {
        // Skip leading slashes.
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
            // Last component; caller wants the parent (current `ip`) + name.
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
        return None; // path was empty or just `/`
    }
    Some((ip, None))
}
