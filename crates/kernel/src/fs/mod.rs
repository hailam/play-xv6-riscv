//! Filesystem layers. Built bottom-up:
//!   * `log` — write-ahead log; the only safe write path for fs.
//!   * (future) inode, dir, path
//!   * (future) the syscall surface that ties them to procs

pub mod log;
