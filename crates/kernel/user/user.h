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
    uint         _pad;          // matches our current kernel layout (G2 not yet fixed)
    uint64       size;
};

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
int   kill(int);
int   exec(const char*, char* const argv[]);
int   open(const char*, int);
int   mknod(const char*, short, short);
int   unlink(const char*);
int   fstat(int fd, struct stat*);
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
int   stat(const char*, struct stat*);
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

#endif
