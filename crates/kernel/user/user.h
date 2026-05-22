// User-space declarations — xv6-style header. Included by every C
// user binary so they share signatures with `ulib.c` / `printf.c`.

#ifndef USER_USER_H
#define USER_USER_H

#include <stdarg.h>

typedef unsigned char  uchar;
typedef unsigned short ushort;
typedef unsigned int   uint;
typedef unsigned int   uint32;
typedef unsigned long  uint64;

#define DIRSIZ 14
#define T_DIR    1
#define T_FILE   2
#define T_DEVICE 3

struct stat {
    int          dev;
    uint         ino;
    short        type;
    short        nlink;
    uint         _pad;
    uint64       size;
    uint         mode;          // POSIX st_mode (S_IF* | rwx perms)
    ushort       uid;
    ushort       gid;
    uint         atime;         // monotonic uptime units (no wall clock)
    uint         mtime;
    uint         ctime;
};

// POSIX st_mode bits.
#define S_IFMT   0170000
#define S_IFDIR  0040000
#define S_IFCHR  0020000
#define S_IFREG  0100000
#define S_IRWXU  0000700
#define S_IRUSR  0000400
#define S_IWUSR  0000200
#define S_IXUSR  0000100
#define S_IRWXG  0000070
#define S_IRWXO  0000007
#define S_ISDIR(m) (((m) & S_IFMT) == S_IFDIR)
#define S_ISREG(m) (((m) & S_IFMT) == S_IFREG)
#define S_ISCHR(m) (((m) & S_IFMT) == S_IFCHR)

struct dirent {
    ushort inum;
    char   name[DIRSIZ];
};

// Syscalls — match xv6 user/user.h. `wait` still takes void here;
// G4 will widen this to `wait(int *)` once the kernel side lands.
int   fork(void);
__attribute__((noreturn)) void exit(int);
int   wait(int* status);
int   pipe(int*);
int   write(int, const void*, int);
int   read(int, void*, int);
int   close(int);
// POSIX lseek. off_t is 64-bit; xv6's inode size is u32 so the kernel
// rejects any new offset > 2^32 - 1.
long  lseek(int fd, long offset, int whence);
int   pread(int fd, void* buf, int len, long offset);
int   pwrite(int fd, const void* buf, int len, long offset);
#define SEEK_SET 0
#define SEEK_CUR 1
#define SEEK_END 2
// POSIX kill(pid, sig). Use SIGKILL or SIGTERM to actually kill.
int   kill(int pid, int sig);

// POSIX signal numbers (subset).
#define SIGHUP   1
#define SIGINT   2
#define SIGQUIT  3
#define SIGILL   4
#define SIGABRT  6
#define SIGKILL  9
#define SIGUSR1  10
#define SIGSEGV  11
#define SIGUSR2  12
#define SIGPIPE  13
#define SIGALRM  14
#define SIGTERM  15
#define SIGCHLD  17
#define SIGCONT  18
#define SIGSTOP  19
int   exec(const char*, char* const argv[]);
int   open(const char*, int);
int   mknod(const char*, short, short);
int   unlink(const char*);
int   fstat(int fd, struct stat*);
int   stat(const char*, struct stat*);
int   chmod(const char* path, uint mode);
int   chown(const char* path, ushort uid, ushort gid);
uint  getuid(void);
uint  getgid(void);
int   setuid(uint uid);
int   setgid(uint gid);
uint  geteuid(void);
uint  getegid(void);
uint  umask(uint mask);
int   fcntl(int fd, int cmd, long arg);
int   ftruncate(int fd, long length);
int   truncate(const char* path, long length);
int   execve(const char* path, char* const argv[], char* const envp[]);

extern char** environ;
char* getenv(const char* name);
int   setenv(const char* name, const char* value, int overwrite);
int   unsetenv(const char* name);

struct timeval {
    long long tv_sec;
    long long tv_usec;
};

struct timespec {
    long long tv_sec;
    long long tv_nsec;
};

int   getppid(void);
int   gettimeofday(struct timeval* tv, void* tz);
int   nanosleep(const struct timespec* req, struct timespec* rem);
long  brk(void* addr);
int   rmdir(const char* path);
int   wait4(int pid, int* status, int options, void* rusage);

// POSIX mmap/munmap. Slice 1 supports MAP_ANONYMOUS | MAP_PRIVATE
// only — anonymous private memory (malloc backend). File-backed
// mmap is slice 2.
#define PROT_NONE    0
#define PROT_READ    1
#define PROT_WRITE   2
#define PROT_EXEC    4
#define MAP_PRIVATE     0x02
#define MAP_ANONYMOUS   0x20
#define MAP_FAILED      ((void*)-1)

void* mmap(void* addr, unsigned int length, int prot, int flags,
           int fd, long offset);
int   munmap(void* addr, unsigned int length);

int   symlink(const char* target, const char* linkpath);
int   readlink(const char* path, char* buf, unsigned int len);
int   lstat(const char* path, struct stat* st);

int   ioctl(int fd, int cmd, void* arg);

// ioctl request numbers (Linux values).
#define TIOCGWINSZ  0x5413
#define TCGETS      0x5401
#define TCSETS      0x5402
#define TCSETSW     0x5403
#define TCSETSF     0x5404
#define FIONREAD    0x541B

struct winsize {
    unsigned short ws_row;
    unsigned short ws_col;
    unsigned short ws_xpixel;
    unsigned short ws_ypixel;
};

struct termios {
    unsigned int   c_iflag, c_oflag, c_cflag, c_lflag;
    unsigned char  c_line;
    unsigned char  c_cc[19];
};

// libc convenience: isatty(fd) — succeeds iff TCGETS works on fd.
static inline int isatty(int fd) {
    struct termios t;
    return ioctl(fd, TCGETS, &t) == 0 ? 1 : 0;
}

// POSIX file-type bit for symlinks (paired with the existing
// S_IFDIR/S_IFREG/S_IFCHR in `struct stat`'s `mode`).
#define S_IFLNK   0120000
#define S_ISLNK(m) (((m) & S_IFMT) == S_IFLNK)

// POSIX-ish sigaction. Slim — we don't expose sa_flags or
// SA_SIGINFO. `handler` is a function pointer (or SIG_DFL/SIG_IGN);
// `mask` is the set of signals to block while it runs.
typedef void (*sighandler_t)(int);
#define SIG_DFL ((sighandler_t)0)
#define SIG_IGN ((sighandler_t)1)

struct sigaction {
    sighandler_t sa_handler;
    unsigned int sa_mask;
};

int sigaction(int signum, const struct sigaction* act,
              struct sigaction* oldact);

// POSIX sigprocmask. `how` is one of:
#define SIG_BLOCK    0
#define SIG_UNBLOCK  1
#define SIG_SETMASK  2
// Our sigset_t is a u32 (32 signals max — matches our internal
// bitmask width). Pass it by value as the `set` arg; the kernel
// writes the previous mask back through `oldset` if non-null.
typedef unsigned int sigset_t;
int sigprocmask(int how, sigset_t set, sigset_t* oldset);

int   dup2(int oldfd, int newfd);
int   getcwd(char* buf, unsigned int len);
int   rename(const char* old_path, const char* new_path);
int   waitpid(int pid, int* status, int options);
int   pause(void);
unsigned int alarm(unsigned int seconds);

#define WNOHANG 1

// POSIX clock_gettime — only MONOTONIC is meaningful (no RTC).
// `struct timespec` declared up top alongside `struct timeval`.
#define CLOCK_REALTIME  0
#define CLOCK_MONOTONIC 1

int clock_gettime(int clk, struct timespec* ts);

// POSIX-ish getdents — packed 24-byte records.
struct dirent_p {
    unsigned long long d_ino;
    unsigned short     d_reclen;
    unsigned short     d_namelen;
    char               d_name[14];
    char               _pad[2];
};

int getdents(int fd, void* buf, unsigned int len);

#define O_CLOEXEC  0x4000
#define O_NONBLOCK 0x8000

#define F_DUPFD          0
#define F_GETFD          1
#define F_SETFD          2
#define F_GETFL          3
#define F_SETFL          4
#define F_DUPFD_CLOEXEC  1030
#define FD_CLOEXEC       1
int   link(const char*, const char*);
int   mkdir(const char*);
int   chdir(const char*);
int   dup(int);
int   getpid(void);
char* sbrk(int);
char* sbrklazy(int);
char* sys_sbrk(int n, int lazy);   // raw syscall — prefer sbrk/sbrklazy
int   sleep(int);
int   uptime(void);

#define SBRK_EAGER 0
#define SBRK_LAZY  1

// ulib.c
char* strcpy(char*, const char*);
int   strcmp(const char*, const char*);
uint  strlen(const char*);
void* memset(void*, int, uint);
char* strchr(const char*, char);
char* gets(char*, int max);
int   atoi(const char*);
void* memmove(void*, const void*, int);
int   memcmp(const void*, const void*, uint);
void* memcpy(void*, const void*, uint);

// umalloc.c
void* malloc(uint);
void  free(void*);

// printf.c
void  fprintf(int, const char*, ...);
void  printf(const char*, ...);

#define O_RDONLY  0x000
#define O_WRONLY  0x001
#define O_RDWR    0x002
#define O_CREATE  0x200
#define O_TRUNC   0x400
#define O_APPEND  0x800

#endif
