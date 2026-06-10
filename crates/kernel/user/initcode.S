#
# initcode — first user proc; immediately execs /init (the real init,
# which forks /sh and loops in wait() reaping reparented orphans).
#

.attribute arch, "rv64gc"

.section .text
.global _start
_start:
        la      a0, init_path
        li      a1, 0
        li      a7, 7                # SYS_EXEC
        ecall
        li      a0, -1
        li      a7, 2                # SYS_EXIT
        ecall
1:      j       1b

init_path:
        .asciz "/init"
