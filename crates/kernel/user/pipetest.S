#
# pipetest — proc-to-proc IPC via async pipe.
#
#   pipe(pipefd);             // returns [rfd, wfd]
#   child = fork();
#   child branch:
#       read(pipefd[0], buf, 16);
#       write(1, buf, n);
#       exit(0);
#   parent branch:
#       write(pipefd[1], "msg\n", 4);
#       wait();
#       exit(0);
#

.attribute arch, "rv64gc"

.section .text
.global _start
_start:
        # pipe(&pipefd)
        la      a0, pipefd
        li      a7, 4                # SYS_PIPE
        ecall

        # fork
        li      a7, 1
        ecall
        beqz    a0, p_child

p_parent:
        # write(pipefd[1], "msg\n", 4)
        la      t0, pipefd
        lw      a0, 4(t0)            # a0 = wfd
        la      a1, msg
        li      a2, 4
        li      a7, 16
        ecall

        # wait()
        li      a7, 3
        ecall

        # exit(0)
        li      a0, 0
        li      a7, 2
        ecall

p_child:
        # read(pipefd[0], buf, 16)
        la      t0, pipefd
        lw      a0, 0(t0)            # a0 = rfd
        la      a1, buf
        li      a2, 16
        li      a7, 5
        ecall
        # a0 = bytes read

        # write(1, buf, a0)
        mv      a2, a0
        li      a0, 1
        la      a1, buf
        li      a7, 16
        ecall

        # exit(0)
        li      a0, 0
        li      a7, 2
        ecall

1:      j       1b

msg:
        .ascii "msg\n"

.balign 4
pipefd:
        .space 8
buf:
        .space 16
