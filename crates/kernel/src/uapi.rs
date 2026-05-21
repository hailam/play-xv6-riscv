//! User-kernel ABI constants — syscall numbers, matching upstream xv6 so
//! ported user binaries `Just Work`.

pub const SYS_FORK: usize = 1;
pub const SYS_EXIT: usize = 2;
pub const SYS_WAIT: usize = 3;
pub const SYS_PIPE: usize = 4;
pub const SYS_READ: usize = 5;
pub const SYS_KILL: usize = 6;
pub const SYS_EXEC: usize = 7;
pub const SYS_FSTAT: usize = 8;
pub const SYS_CHDIR: usize = 9;
pub const SYS_DUP: usize = 10;
pub const SYS_GETPID: usize = 11;
pub const SYS_SBRK: usize = 12;
pub const SYS_SLEEP: usize = 13;
pub const SYS_UPTIME: usize = 14;
pub const SYS_OPEN: usize = 15;
pub const SYS_WRITE: usize = 16;
pub const SYS_MKNOD: usize = 17;
pub const SYS_UNLINK: usize = 18;
pub const SYS_LINK: usize = 19;
pub const SYS_MKDIR: usize = 20;
pub const SYS_CLOSE: usize = 21;
pub const SYS_LSEEK: usize = 22;
pub const SYS_PREAD: usize = 23;
pub const SYS_PWRITE: usize = 24;
pub const SYS_STAT: usize = 25;
pub const SYS_CHMOD: usize = 26;
pub const SYS_CHOWN: usize = 27;
pub const SYS_GETUID: usize = 28;
pub const SYS_GETGID: usize = 29;
pub const SYS_SETUID: usize = 30;
pub const SYS_SETGID: usize = 31;
pub const SYS_GETEUID: usize = 32;
pub const SYS_GETEGID: usize = 33;
pub const SYS_UMASK: usize = 34;
pub const SYS_FCNTL: usize = 35;
pub const SYS_FTRUNCATE: usize = 36;
pub const SYS_TRUNCATE: usize = 37;

// lseek "whence" values — POSIX-standard.
pub const SEEK_SET: i32 = 0; // absolute offset
pub const SEEK_CUR: i32 = 1; // current + offset
pub const SEEK_END: i32 = 2; // EOF + offset

// open() flags (matches xv6 `kernel/fcntl.h`, plus POSIX additions).
pub const O_RDONLY: u32 = 0x000;
pub const O_WRONLY: u32 = 0x001;
pub const O_RDWR: u32 = 0x002;
pub const O_CREATE: u32 = 0x200;
pub const O_TRUNC: u32 = 0x400;
// POSIX additions (Tier 1 of the posix-compat track).
pub const O_APPEND: u32 = 0x800;
pub const O_CLOEXEC: u32 = 0x4000;
pub const O_NONBLOCK: u32 = 0x8000;

// fcntl commands (subset).
pub const F_DUPFD: i32 = 0;
pub const F_GETFD: i32 = 1;
pub const F_SETFD: i32 = 2;
pub const F_GETFL: i32 = 3;
pub const F_SETFL: i32 = 4;
pub const F_DUPFD_CLOEXEC: i32 = 1030; // Linux value

// Bits in F_GETFD/F_SETFD's third arg.
pub const FD_CLOEXEC: i32 = 1;

/// User-visible `struct stat`. Extended from xv6's 24-byte layout
/// to expose POSIX `st_mode`/`st_uid`/`st_gid`/`st_*time`. Total
/// now 48 bytes:
///
///   dev:i32 ino:u32 typ:i16 nlink:i16 pad:u32 size:u64
///   mode:u32 uid:u16 gid:u16 atime:u32 mtime:u32 ctime:u32
///
/// `typ` stays for backward compat with anything that read the old
/// 24-byte layout. `mode` synthesises POSIX-style `S_IFREG`/
/// `S_IFDIR`/`S_IFCHR` in the upper 4 bits and the rwx perm bits
/// in the lower 12 — what `chmod` and `umask` actually manipulate.
/// Timestamps are in monotonic uptime units (no wall clock yet).
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct Stat {
    pub dev: i32,
    pub ino: u32,
    pub typ: i16,
    pub nlink: i16,
    pub _pad: u32,
    pub size: u64,
    pub mode: u32,
    pub uid: u16,
    pub gid: u16,
    pub atime: u32,
    pub mtime: u32,
    pub ctime: u32,
}

// POSIX S_IF* file-type bits (top of st_mode).
pub const S_IFMT: u32 = 0o170000;
pub const S_IFDIR: u32 = 0o040000;
pub const S_IFCHR: u32 = 0o020000;
pub const S_IFREG: u32 = 0o100000;

/// Build a POSIX `st_mode` from xv6's typ + perm bits. Used by
/// fstat/stat to fill the new `mode` field.
#[inline]
pub fn stat_mode(typ: u16, perm: u16) -> u32 {
    use xv6_fs_layout::{T_DEVICE, T_DIR, T_FILE};
    let kind = match typ {
        T_DIR => S_IFDIR,
        T_FILE => S_IFREG,
        T_DEVICE => S_IFCHR,
        _ => 0,
    };
    kind | (perm as u32 & 0o7777)
}
