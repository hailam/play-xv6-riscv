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
DEFINE_SYSCALL exec,   7
DEFINE_SYSCALL dup,    10
DEFINE_SYSCALL sleep,  13
DEFINE_SYSCALL write,  16
DEFINE_SYSCALL close,  21
