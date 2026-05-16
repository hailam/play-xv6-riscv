//! Filesystem layers (bottom-up):
//!   * `superblock` — single static cache, populated at boot.
//!   * `log`        — write-ahead log; only safe write path for fs.
//!   * `inode`      — inode cache + async `ilock` + `readi`.
//!   * `dir`        — directory operations on top of `inode`.
//!   * `path`       — `namei` / `nameiparent`.

pub mod bmap;
pub mod dir;
pub mod inode;
pub mod log;
pub mod path;
pub mod superblock;

pub use path::{namei, nameiparent};
