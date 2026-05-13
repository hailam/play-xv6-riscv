#
# Phase 5d initcode: a minimal shell.
#   loop:
#     prompt "$ "
#     read line (up to 63 bytes, stops at '\n')
#     strip trailing '\n', null-terminate
#     loop again when empty
#     fork; child execs the typed path; parent waits
#
# No argv yet — exec sees just the first word (and treats the whole line
# as path). Type "/echo" or "/hello".
#

.attribute arch, "rv64gc"

.section .text
.global _start
_start:
shell_loop:
        # write(1, "$ ", 2)
        li      a0, 1
        la      a1, prompt
        li      a2, 2
        li      a7, 16
        ecall

        # n = read(0, buf, 63)
        li      a0, 0
        la      a1, buf
        li      a2, 63
        li      a7, 5
        ecall
        # when n <= 0, exit gracefully
        blez    a0, sh_eof

        # when last byte is '\n', replace with 0.
        la      t0, buf
        add     t1, t0, a0
        addi    t1, t1, -1
        lbu     t2, 0(t1)
        li      t3, 10                # '\n'
        bne     t2, t3, no_strip
        sb      zero, 0(t1)
no_strip:
        # zero-terminate at buf+n (defensive)
        la      t0, buf
        add     t0, t0, a0
        sb      zero, 0(t0)

        # when buf[0] == 0 (empty after strip), loop
        la      t0, buf
        lbu     t1, 0(t0)
        beqz    t1, shell_loop

        # fork
        li      a7, 1
        ecall
        beqz    a0, sh_child

        # parent: wait()
        li      a7, 3
        ecall
        j       shell_loop

sh_child:
        # exec(buf, NULL)
        la      a0, buf
        li      a1, 0
        li      a7, 7
        ecall

        # exec failed: print "bad cmd\n" and exit(-1).
        li      a0, 1
        la      a1, bad_msg
        li      a2, 8
        li      a7, 16
        ecall
        li      a0, -1
        li      a7, 2
        ecall

sh_eof:
        # write(1, "bye!\n", 5); exit(0).
        li      a0, 1
        la      a1, eof_msg
        li      a2, 5
        li      a7, 16
        ecall
        li      a0, 0
        li      a7, 2
        ecall

1:      j       1b

prompt:
        .ascii "$ "
bad_msg:
        .ascii "bad cmd\n"
eof_msg:
        .ascii "bye!\n"

.balign 8
buf:
        .space 64
