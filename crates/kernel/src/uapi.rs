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

// open() flags (matches xv6 `kernel/fcntl.h`).
pub const O_RDONLY: u32 = 0x000;
pub const O_WRONLY: u32 = 0x001;
pub const O_RDWR: u32 = 0x002;
pub const O_CREATE: u32 = 0x200;
pub const O_TRUNC: u32 = 0x400;

/// User-visible `struct stat`. Field order + size matches xv6
/// `kernel/stat.h`. Total = 24 bytes (4 + 4 + 2 + 2 + 4 pad + 8).
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct Stat {
    pub dev: i32,
    pub ino: u32,
    pub typ: i16,
    pub nlink: i16,
    pub _pad: u32,
    pub size: u64,
}
