// ioctltest — TIOCGWINSZ + TCGETS/TCSETS + FIONREAD on console;
// non-tty fds reject the terminal cmds.

#include "user.h"

static void die(const char* msg) {
    printf("ioctltest: %s\n", msg);
    exit(1);
}

int main(int argc, char* argv[]) {
    // 1) isatty on stdin (fd 0 = console) → true.
    int t = isatty(0);
    printf("isatty(0) -> %d (expected 1)\n", t);
    if (t != 1) die("isatty(0) failed");

    // 2) isatty on a regular file → false.
    int fd = open("/README", O_RDONLY);
    if (fd < 0) die("open(/README) failed");
    t = isatty(fd);
    printf("isatty(README) -> %d (expected 0)\n", t);
    if (t != 0) die("isatty on file said yes");
    close(fd);

    // 3) TIOCGWINSZ on console.
    struct winsize ws;
    int r = ioctl(0, TIOCGWINSZ, &ws);
    printf("TIOCGWINSZ -> %d row=%d col=%d (expect 0/24/80)\n",
           r, ws.ws_row, ws.ws_col);
    if (r != 0) die("TIOCGWINSZ failed");
    if (ws.ws_row != 24 || ws.ws_col != 80) die("winsize wrong");

    // 4) TCGETS round-trips through TCSETS.
    struct termios t1, t2;
    if (ioctl(0, TCGETS, &t1) != 0) die("TCGETS failed");
    printf("TCGETS: iflag=0x%x oflag=0x%x lflag=0x%x\n",
           t1.c_iflag, t1.c_oflag, t1.c_lflag);
    if (ioctl(0, TCSETS, &t1) != 0) die("TCSETS failed");
    if (ioctl(0, TCGETS, &t2) != 0) die("TCGETS#2 failed");
    if (t1.c_iflag != t2.c_iflag || t1.c_lflag != t2.c_lflag)
        die("termios round-trip mismatch");

    // 5) FIONREAD on console — likely 0 (we haven't typed since
    //    the test was invoked), but the syscall must succeed.
    int n = 0;
    r = ioctl(0, FIONREAD, &n);
    printf("FIONREAD -> %d n=%d (expect 0 success, n>=0)\n", r, n);
    if (r != 0) die("FIONREAD failed");
    if (n < 0) die("FIONREAD negative");

    // 6) Bogus ioctl on console → -1.
    r = ioctl(0, 0x12345, &ws);
    printf("bogus ioctl -> %d (expected -1)\n", r);
    if (r != -1) die("bogus ioctl accepted");

    printf("ioctltest ok\n");
    return 0;
}
