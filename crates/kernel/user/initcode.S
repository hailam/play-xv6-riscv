#
# initcode — first user proc; immediately execs /sh.
#

.attribute arch, "rv64gc"

.section .text
.global _start
_start:
        la      a0, sh_path
        li      a1, 0
        li      a7, 7                # SYS_EXEC
        ecall
        li      a0, -1
        li      a7, 2                # SYS_EXIT
        ecall
1:      j       1b

sh_path:
        .asciz "/sh"
