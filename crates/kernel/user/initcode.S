#
# Phase 5b initcode: parent forks, child sleeps then prints, parent
# blocks in wait. Verifies async sleep + wait wakers.
#

.attribute arch, "rv64gc"

.section .text
.global _start
_start:
        # fork()
        li      a7, 1
        ecall
        beqz    a0, child

parent:
        # write "parent: waiting\n"
        li      a0, 1
        la      a1, p_wait_msg
        li      a2, 16
        li      a7, 16
        ecall

        # wait()
        li      a7, 3
        ecall

        # write "parent: reaped\n"
        li      a0, 1
        la      a1, p_reap_msg
        li      a2, 15
        li      a7, 16
        ecall

        j       do_exit

child:
        # sleep(10) — 10 ticks ≈ 1s at our cadence.
        li      a0, 10
        li      a7, 13
        ecall

        # write "child woke\n"
        li      a0, 1
        la      a1, c_woke_msg
        li      a2, 11
        li      a7, 16
        ecall

do_exit:
        li      a0, 0
        li      a7, 2
        ecall

1:      j       1b

p_wait_msg:
        .ascii "parent: waiting\n"
p_reap_msg:
        .ascii "parent: reaped\n"
c_woke_msg:
        .ascii "child woke\n"
