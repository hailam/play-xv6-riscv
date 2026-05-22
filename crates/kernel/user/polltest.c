// polltest — POSIX poll() across file / pipe / negative-fd / bad-fd.

#include "user.h"

static void die(const char* msg) {
    printf("polltest: %s\n", msg);
    exit(1);
}

int main(int argc, char* argv[]) {
    // 1) File-backed fd should always be ready.
    int rfd = open("/README", O_RDONLY);
    if (rfd < 0) die("open(/README) failed");
    struct pollfd p = {rfd, POLLIN | POLLOUT, 0};
    int r = poll(&p, 1, 0);
    printf("poll(file POLLIN|POLLOUT) -> %d revents=0x%x (expect 1, both bits)\n",
           r, p.revents);
    if (r != 1) die("file should be ready");
    if (!(p.revents & POLLIN) || !(p.revents & POLLOUT))
        die("file revents missing IN|OUT");
    close(rfd);

    // 2) Negative fd → ignored, revents stays 0.
    struct pollfd pneg = {-1, POLLIN, 0xbeef};
    r = poll(&pneg, 1, 0);
    printf("poll(fd=-1) -> %d revents=0x%x (expect 0, 0)\n", r, pneg.revents);
    if (r != 0) die("negative fd not ignored");

    // 3) Invalid fd (out-of-range or closed) → POLLNVAL.
    struct pollfd pinv = {999, POLLIN, 0};
    r = poll(&pinv, 1, 0);
    printf("poll(fd=999) -> %d revents=0x%x (expect 1, POLLNVAL)\n",
           r, pinv.revents);
    if (r != 1 || pinv.revents != POLLNVAL) die("bad-fd not POLLNVAL");

    // 4) Pipe: drained for read → not ready; writer has space → ready.
    int fds[2];
    if (pipe(fds) < 0) die("pipe failed");
    struct pollfd pp[2] = {
        {fds[0], POLLIN, 0},
        {fds[1], POLLOUT, 0},
    };
    r = poll(pp, 2, 0);
    printf("empty pipe: r=%d rd=0x%x wr=0x%x (expect 1, 0, POLLOUT)\n",
           r, pp[0].revents, pp[1].revents);
    if (r != 1) die("empty pipe: only writer should be ready");
    if (pp[0].revents != 0 || !(pp[1].revents & POLLOUT))
        die("empty pipe revents wrong");

    // Now write a byte: reader becomes ready.
    write(fds[1], "x", 1);
    pp[0].revents = 0;
    pp[1].revents = 0;
    r = poll(pp, 2, 0);
    printf("buffered pipe: r=%d rd=0x%x wr=0x%x (expect 2, POLLIN, POLLOUT)\n",
           r, pp[0].revents, pp[1].revents);
    if (r != 2) die("both pipe ends should be ready");
    close(fds[0]);
    close(fds[1]);

    // 5) Timeout: poll a not-ready negative-fd-only set with a short
    //    timeout → returns 0 after ~timeout.
    struct pollfd pt = {-1, POLLIN, 0};
    struct timespec t0, t1;
    clock_gettime(CLOCK_MONOTONIC, &t0);
    r = poll(&pt, 1, 250);
    clock_gettime(CLOCK_MONOTONIC, &t1);
    long long el_ms =
        (t1.tv_sec - t0.tv_sec) * 1000LL +
        (t1.tv_nsec - t0.tv_nsec) / 1000000LL;
    printf("poll(timeout=250ms) -> %d elapsed=%lldms (expect 0, ~250ms)\n",
           r, el_ms);
    if (r != 0) die("timeout poll didn't return 0");
    if (el_ms < 100) die("timeout fired too early");

    printf("polltest ok\n");
    return 0;
}
