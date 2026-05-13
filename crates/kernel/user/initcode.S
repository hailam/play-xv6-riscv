#
# Phase 4b/5a initcode: fork, both parent and child write distinct
# banners then exit. Verifies the async executor schedules two procs.
#

.attribute arch, "rv64gc"

.section .text
.global _start
_start:
        # fork()
        li      a7, 1                # SYS_FORK
        ecall

        beqz    a0, child

parent:
        li      a0, 1
        la      a1, parent_msg
        li      a2, 7                # "parent\n"
        li      a7, 16               # SYS_WRITE
        ecall
        j       do_exit

child:
        li      a0, 1
        la      a1, child_msg
        li      a2, 6                # "child\n"
        li      a7, 16
        ecall

do_exit:
        li      a0, 0
        li      a7, 2                # SYS_EXIT
        ecall

        # exit should not return; spin defensively.
1:      j       1b

parent_msg:
        .ascii "parent\n"
child_msg:
        .ascii "child\n"
