#
# echo — printed banner showing this is a distinct embedded binary from
# initcode, reached via sys_exec.
#

.attribute arch, "rv64gc"

.section .text
.global _start
_start:
        li      a0, 1
        la      a1, msg
        li      a2, 16
        li      a7, 16               # SYS_WRITE
        ecall

        li      a0, 0
        li      a7, 2                # SYS_EXIT
        ecall

1:      j       1b

msg:
        .ascii "echo: alive!!!\n\n"
