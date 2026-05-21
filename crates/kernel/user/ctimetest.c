// ctimetest — clock_gettime monotonicity + getdents enumeration.

#include "user.h"

static void die(const char* msg) {
    printf("ctimetest: %s\n", msg);
    exit(1);
}

int main(int argc, char* argv[]) {
    // 1) clock_gettime monotonic across sleep.
    struct timespec t0, t1;
    if (clock_gettime(CLOCK_MONOTONIC, &t0) < 0) die("clock_gettime#0 failed");
    sleep(3);
    if (clock_gettime(CLOCK_MONOTONIC, &t1) < 0) die("clock_gettime#1 failed");
    printf("t0=%lld.%lld t1=%lld.%lld\n",
           (long long)t0.tv_sec, (long long)t0.tv_nsec,
           (long long)t1.tv_sec, (long long)t1.tv_nsec);
    long long delta_ns =
        (t1.tv_sec - t0.tv_sec) * 1000000000LL + (t1.tv_nsec - t0.tv_nsec);
    printf("delta_ns=%lld (expected >0)\n", delta_ns);
    if (delta_ns <= 0) die("clock not monotonic");

    // Bogus clock id → -1.
    int r = clock_gettime(99, &t0);
    printf("clock_gettime(99) -> %d (expected -1)\n", r);
    if (r != -1) die("bogus clock id accepted");

    // 2) getdents on /.
    int fd = open("/", O_RDONLY);
    if (fd < 0) die("open(/) failed");
    char buf[256];
    int seen = 0;
    int got_readme = 0;
    while (1) {
        int n = getdents(fd, buf, sizeof(buf));
        if (n < 0) die("getdents failed");
        if (n == 0) break;
        int off = 0;
        while (off + (int)sizeof(struct dirent_p) <= n) {
            struct dirent_p* d = (struct dirent_p*)(buf + off);
            // Only print first few to keep output small.
            if (seen < 6) {
                printf("  ino=%llu len=%d name=\"", d->d_ino, d->d_namelen);
                for (int i = 0; i < d->d_namelen; i++)
                    write(1, &d->d_name[i], 1);
                printf("\"\n");
            }
            if (d->d_namelen == 6 &&
                d->d_name[0] == 'R' && d->d_name[1] == 'E' &&
                d->d_name[2] == 'A' && d->d_name[3] == 'D' &&
                d->d_name[4] == 'M' && d->d_name[5] == 'E') {
                got_readme = 1;
            }
            seen++;
            off += sizeof(struct dirent_p);
        }
    }
    close(fd);
    printf("getdents saw %d entries; README seen? %d\n", seen, got_readme);
    if (seen < 4) die("too few entries");
    if (!got_readme) die("didn't see README");

    printf("ctimetest ok\n");
    return 0;
}
