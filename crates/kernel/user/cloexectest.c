// cloexectest — exercises O_CLOEXEC, fcntl(F_GETFD/F_SETFD), and
// fcntl(F_DUPFD/F_DUPFD_CLOEXEC).

#include "user.h"

static void die(const char* msg) {
    printf("cloexectest: %s\n", msg);
    exit(1);
}

int main(int argc, char* argv[]) {
    // 1) Open without O_CLOEXEC — F_GETFD should report 0.
    int fd = open("README", O_RDONLY);
    if (fd < 0) die("open(README) failed");
    int fl = fcntl(fd, F_GETFD, 0);
    printf("open w/o cloexec: F_GETFD -> %d (expected 0)\n", fl);
    if (fl != 0) die("F_GETFD said cloexec on a plain open");

    // 2) F_SETFD(FD_CLOEXEC) -> F_GETFD reports 1.
    if (fcntl(fd, F_SETFD, FD_CLOEXEC) != 0) die("F_SETFD failed");
    fl = fcntl(fd, F_GETFD, 0);
    printf("after F_SETFD: F_GETFD -> %d (expected 1)\n", fl);
    if (fl != FD_CLOEXEC) die("F_SETFD didn't take");

    // 3) Clear it again.
    if (fcntl(fd, F_SETFD, 0) != 0) die("F_SETFD clear failed");
    fl = fcntl(fd, F_GETFD, 0);
    printf("after clear: F_GETFD -> %d (expected 0)\n", fl);
    if (fl != 0) die("F_SETFD clear didn't take");

    // 4) F_DUPFD — like dup but lets us pick the starting fd. Result
    //    should have cloexec=0 even if source had it set.
    if (fcntl(fd, F_SETFD, FD_CLOEXEC) != 0) die("re-set cloexec failed");
    int nfd = fcntl(fd, F_DUPFD, 10);
    printf("F_DUPFD(10) -> %d (expected >=10)\n", nfd);
    if (nfd < 10) die("F_DUPFD didn't honor start");
    int nfl = fcntl(nfd, F_GETFD, 0);
    printf("dup'd fd's cloexec -> %d (expected 0, dup strips it)\n", nfl);
    if (nfl != 0) die("F_DUPFD didn't strip cloexec");

    // 5) F_DUPFD_CLOEXEC sets it on the new fd.
    // NOFILE is small (16); start above nfd=10 but stay within.
    int nfd2 = fcntl(fd, F_DUPFD_CLOEXEC, 11);
    int nfl2 = fcntl(nfd2, F_GETFD, 0);
    printf("F_DUPFD_CLOEXEC -> %d, cloexec=%d (expected 1)\n", nfd2, nfl2);
    if (nfl2 != FD_CLOEXEC) die("F_DUPFD_CLOEXEC didn't set cloexec");
    close(nfd2);
    close(nfd);
    // fd still has cloexec set from step (4).

    // 6) Open another fd with O_CLOEXEC inline — F_GETFD should be 1.
    int fd2 = open("README", O_RDONLY | O_CLOEXEC);
    if (fd2 < 0) die("open w/ O_CLOEXEC failed");
    int fl2 = fcntl(fd2, F_GETFD, 0);
    printf("open(O_CLOEXEC): F_GETFD -> %d (expected 1)\n", fl2);
    if (fl2 != FD_CLOEXEC) die("O_CLOEXEC didn't set cloexec");

    // 7) Fork + exec — the child should NOT inherit fd or fd2 (both
    //    have cloexec). The child execs `echo "child"`.
    int pid = fork();
    if (pid < 0) die("fork failed");
    if (pid == 0) {
        // In child: fd and fd2 should already be closed.
        // We can't observe directly (no /proc), but if exec doesn't
        // close them and the binary tries to use them later we'd
        // leak. Use F_GETFD on fd; if it returns -1, the fd is gone.
        int s = fcntl(fd, F_GETFD, 0);
        // We're a child still — exec hasn't run yet, so fd is still
        // there. Exec it.
        char* argv2[] = {"echo", "child-saw-fd-precount", 0};
        // Pre-test: count cloexec fds before exec via fcntl probe.
        // Just print the pre-exec state and exec.
        printf("pre-exec child: fcntl(fd)=%d\n", s);
        exec("/echo", argv2);
        printf("exec failed\n");
        exit(1);
    }
    int status = 0;
    wait(&status);
    printf("child status=%d\n", status);

    // 8) F_GETFL / F_SETFL round-trip on O_NONBLOCK.
    int fl3 = fcntl(fd2, F_GETFL, 0);
    printf("F_GETFL on README fd2 -> 0x%x (expected 0)\n", fl3);
    if (fcntl(fd2, F_SETFL, O_NONBLOCK) != 0) die("F_SETFL failed");
    fl3 = fcntl(fd2, F_GETFL, 0);
    printf("after F_SETFL O_NONBLOCK: F_GETFL -> 0x%x (expected 0x8000)\n", fl3);
    if ((fl3 & O_NONBLOCK) == 0) die("O_NONBLOCK didn't take");

    close(fd);
    close(fd2);

    printf("cloexectest ok\n");
    return 0;
}
