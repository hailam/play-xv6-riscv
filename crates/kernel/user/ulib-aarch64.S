//
// User-mode runtime for aarch64: _start + syscall stubs.
//
// ABI mirror of upstream xv6/riscv: caller passes args in x0..x5;
// return value lands in x0. Syscall number goes in x8, then `svc #0`
// vectors into trampoline_vector_table+0x400 (uservec_sync) in the
// kernel. Trap entry decodes ESR_EL1.EC = 0x15 and dispatches to
// the syscall table just like the riscv ecall path.
//

.section .text.entry, "ax"
.global _start
_start:
        bl      main
        // main returned — fall through to exit(retval). x0 already
        // holds main's return value per the AAPCS64 calling convention.
        mov     x8, #2          // SYS_EXIT
        svc     #0
1:      b       1b

.macro DEFINE_SYSCALL name, nr
.section .text, "ax"
.global \name
\name:
        mov     x8, #\nr
        svc     #0
        ret
.endm

DEFINE_SYSCALL fork,     1
.global exit
exit:
        mov     x8, #2
        svc     #0
2:      b       2b          // exit is noreturn; defensive spin.

DEFINE_SYSCALL wait,     3
DEFINE_SYSCALL pipe,     4
DEFINE_SYSCALL read,     5
DEFINE_SYSCALL kill,     6
DEFINE_SYSCALL exec,     7
DEFINE_SYSCALL fstat,    8
DEFINE_SYSCALL chdir,    9
DEFINE_SYSCALL dup,     10
DEFINE_SYSCALL getpid,  11
DEFINE_SYSCALL sys_sbrk, 12
DEFINE_SYSCALL sleep,   13
DEFINE_SYSCALL uptime,  14
DEFINE_SYSCALL open,    15
DEFINE_SYSCALL write,   16
DEFINE_SYSCALL mknod,   17
DEFINE_SYSCALL unlink,  18
DEFINE_SYSCALL link,    19
DEFINE_SYSCALL mkdir,   20
DEFINE_SYSCALL close,   21
DEFINE_SYSCALL lseek,   22
DEFINE_SYSCALL pread,   23
DEFINE_SYSCALL pwrite,  24
DEFINE_SYSCALL stat,    25
