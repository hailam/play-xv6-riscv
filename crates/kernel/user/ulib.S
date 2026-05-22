#
# User-mode runtime: _start + syscall wrappers. Linked with every C
# user program. The C convention is: caller puts args in a0..a5,
# return value lands in a0. We just stuff the syscall number into a7
# and `ecall`.
#

.attribute arch, "rv64gc"

.section .text.entry
.global _start
_start:
        # Kernel set a0=argc, a1=argv, a2=envp before sret. Stash
        # envp into the global `environ` (defined in ulib.c) so
        # getenv/setenv have something to look at.
        la      t0, environ
        sd      a2, 0(t0)
        call    main
        # `main` returned; fall through to exit with its return value.
        li      a7, 2
        ecall
1:      j       1b

.macro DEFINE_SYSCALL name, nr
.global \name
\name:
        li      a7, \nr
        ecall
        ret
.endm

DEFINE_SYSCALL fork,   1
.global exit
exit:
        li      a7, 2
        ecall
2:      j       2b           # exit is noreturn; defensive spin.
DEFINE_SYSCALL wait,   3
DEFINE_SYSCALL pipe,   4
DEFINE_SYSCALL read,   5
DEFINE_SYSCALL kill,   6
DEFINE_SYSCALL exec,   7
DEFINE_SYSCALL fstat,  8
DEFINE_SYSCALL chdir,  9
DEFINE_SYSCALL dup,    10
DEFINE_SYSCALL getpid, 11
DEFINE_SYSCALL sys_sbrk, 12
DEFINE_SYSCALL sleep,  13
DEFINE_SYSCALL uptime, 14
DEFINE_SYSCALL open,   15
DEFINE_SYSCALL write,  16
DEFINE_SYSCALL mknod,  17
DEFINE_SYSCALL unlink, 18
DEFINE_SYSCALL link,   19
DEFINE_SYSCALL mkdir,  20
DEFINE_SYSCALL close,  21
DEFINE_SYSCALL lseek,  22
DEFINE_SYSCALL pread,  23
DEFINE_SYSCALL pwrite, 24
DEFINE_SYSCALL stat,   25
DEFINE_SYSCALL chmod,    26
DEFINE_SYSCALL chown,    27
DEFINE_SYSCALL getuid,   28
DEFINE_SYSCALL getgid,   29
DEFINE_SYSCALL setuid,   30
DEFINE_SYSCALL setgid,   31
DEFINE_SYSCALL geteuid,  32
DEFINE_SYSCALL getegid,  33
DEFINE_SYSCALL umask,    34
DEFINE_SYSCALL fcntl,    35
DEFINE_SYSCALL ftruncate,36
DEFINE_SYSCALL truncate, 37
DEFINE_SYSCALL _sys_sigaction, 38
DEFINE_SYSCALL sigprocmask, 40
DEFINE_SYSCALL dup2,     41
DEFINE_SYSCALL getcwd,   42
DEFINE_SYSCALL rename,   43
DEFINE_SYSCALL waitpid,  44
DEFINE_SYSCALL pause,    45
DEFINE_SYSCALL alarm,    46
DEFINE_SYSCALL clock_gettime, 47
DEFINE_SYSCALL getdents,      48
DEFINE_SYSCALL execve,        49
DEFINE_SYSCALL getppid,       50
DEFINE_SYSCALL gettimeofday,  51
DEFINE_SYSCALL nanosleep,     52
DEFINE_SYSCALL brk,           53
DEFINE_SYSCALL rmdir,         54
DEFINE_SYSCALL wait4,         55
DEFINE_SYSCALL mmap,          56
DEFINE_SYSCALL munmap,        57

// _sigret — POSIX signal restorer trampoline. The kernel writes
// `ra := &_sigret` into the trapframe before delivering a signal,
// so the handler's `ret` falls through to this; we then drop into
// SYS_SIGRETURN which restores the saved trapframe.
.section .text
.global _sigret
_sigret:
        li      a7, 39           // SYS_SIGRETURN
        ecall
1:      j       1b
