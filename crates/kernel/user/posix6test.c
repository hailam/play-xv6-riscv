// posix6test — getppid, gettimeofday, nanosleep, brk, rmdir, wait4.

#include "user.h"

static void die(const char* msg) {
    printf("posix6test: %s\n", msg);
    exit(1);
}

int main(int argc, char* argv[]) {
    int my = getpid();
    int pp = getppid();
    printf("getpid=%d getppid=%d (expected pp>0)\n", my, pp);
    if (pp <= 0) die("getppid <= 0");

    struct timeval tv0, tv1;
    gettimeofday(&tv0, 0);
    sleep(2);
    gettimeofday(&tv1, 0);
    long long du = (tv1.tv_sec - tv0.tv_sec) * 1000000LL +
                   (tv1.tv_usec - tv0.tv_usec);
    printf("gettimeofday delta_us=%lld (expected >0)\n", du);
    if (du <= 0) die("gettimeofday not monotonic");

    // nanosleep — 250ms.
    struct timespec req = {0, 250000000};
    int r = nanosleep(&req, 0);
    printf("nanosleep(250ms) -> %d (expected 0)\n", r);
    if (r != 0) die("nanosleep failed");

    // brk: query → current; set to current + 4096 → grow.
    long cur = brk(0);
    printf("brk(0) -> %ld (current break)\n", cur);
    if (cur < 0) die("brk(0) failed");
    long r2 = brk((void*)(cur + 4096));
    printf("brk(+4k) -> %ld (expected 0)\n", r2);
    if (r2 != 0) die("brk grow failed");
    long after = brk(0);
    printf("brk(0) after grow -> %ld (expected %ld)\n", after, cur + 4096);
    if (after != cur + 4096) die("brk didn't take");
    brk((void*)cur);  // shrink back

    // rmdir: success on empty, fail on non-empty + non-dir.
    mkdir("/rmd");
    r = rmdir("/rmd");
    printf("rmdir(/rmd empty) -> %d (expected 0)\n", r);
    if (r != 0) die("rmdir empty failed");

    mkdir("/rmd2");
    int f = open("/rmd2/x", O_CREATE | O_RDWR);
    close(f);
    r = rmdir("/rmd2");
    printf("rmdir(non-empty) -> %d (expected -1)\n", r);
    if (r != -1) die("rmdir non-empty succeeded");
    unlink("/rmd2/x");
    rmdir("/rmd2");

    // rmdir on a file → -1.
    f = open("/rmf", O_CREATE | O_RDWR);
    close(f);
    r = rmdir("/rmf");
    printf("rmdir(file) -> %d (expected -1)\n", r);
    if (r != -1) die("rmdir on file succeeded");
    unlink("/rmf");

    // wait4: fork child that exits 42; wait4 reaps + clears rusage.
    int pid = fork();
    if (pid == 0) {
        sleep(2);
        exit(42);
    }
    int status = 0;
    // Kernel Rusage = 2 Timevals (32B) + [i64; 14] (112B) = 144B.
    // Size buffer to match so wait4 doesn't trample a neighbor.
    char rusage[144];
    for (int i = 0; i < 144; i++) rusage[i] = (char)0xff;
    int reaped = wait4(pid, &status, 0, rusage);
    printf("wait4 reaped=%d status=%d ru[0]=%d (expected pid=%d, 42, 0)\n",
           reaped, status, (int)(unsigned char)rusage[0], pid);
    if (reaped != pid) die("wait4 wrong pid");
    if (status != 42) die("wait4 wrong status");
    if (rusage[0] != 0) die("wait4 didn't zero rusage");

    printf("posix6test ok\n");
    return 0;
}
