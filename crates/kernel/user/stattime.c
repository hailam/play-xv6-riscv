// stattime — verify atime/mtime/ctime tracking.
//
// Timestamps are in "uptime units" (some monotonic tick / TIMER_INTERVAL).
// We don't care about the unit — only that values are monotonic and
// that mtime/ctime bump on writes, ctime bumps on chmod, atime bumps
// on reads.

#include "user.h"

static void die(const char* msg) {
    printf("stattime: %s\n", msg);
    exit(1);
}

static void busy_wait(void) {
    // Burn enough time that the timestamp tick advances. now_secs()
    // ticks every TIMER_INTERVAL = ~100ms, so loop ~1.5s.
    sleep(2);
}

int main(int argc, char* argv[]) {
    int fd = open("/sttest", O_CREATE | O_RDWR);
    if (fd < 0) die("open(/sttest) failed");
    write(fd, "hello", 5);
    close(fd);

    struct stat s0;
    if (stat("/sttest", &s0) < 0) die("stat#0 failed");
    printf("after create+write: atime=%d mtime=%d ctime=%d\n",
           s0.atime, s0.mtime, s0.ctime);
    // All three should equal each other (ialloc and write both
    // happen "now"), and all > 0.
    if (s0.mtime == 0 || s0.ctime == 0) die("zero timestamps on create");
    if (s0.mtime != s0.ctime) die("mtime != ctime on fresh write");

    busy_wait();

    // 1) Append more data — mtime + ctime should advance, atime
    //    should not (we haven't read).
    fd = open("/sttest", O_WRONLY | O_APPEND);
    write(fd, "world", 5);
    close(fd);

    struct stat s1;
    if (stat("/sttest", &s1) < 0) die("stat#1 failed");
    printf("after append:       atime=%d mtime=%d ctime=%d\n",
           s1.atime, s1.mtime, s1.ctime);
    if (s1.mtime <= s0.mtime) die("mtime didn't advance on write");
    if (s1.ctime <= s0.ctime) die("ctime didn't advance on write");

    busy_wait();

    // 2) chmod — ctime advances, mtime stays put.
    if (chmod("/sttest", 0644) < 0) die("chmod failed");
    struct stat s2;
    if (stat("/sttest", &s2) < 0) die("stat#2 failed");
    printf("after chmod:        atime=%d mtime=%d ctime=%d\n",
           s2.atime, s2.mtime, s2.ctime);
    if (s2.ctime <= s1.ctime) die("ctime didn't advance on chmod");
    if (s2.mtime != s1.mtime) die("mtime moved on chmod (it shouldn't)");

    busy_wait();

    // 3) Read — atime advances. We observe via fstat() on the open
    //    fd so the in-memory atime bump survives (it isn't flushed
    //    to disk; if we closed and re-stat'd, the inode-cache slot
    //    would get reused and we'd re-load disk state).
    fd = open("/sttest", O_RDONLY);
    char buf[16];
    read(fd, buf, sizeof(buf));

    struct stat s3;
    if (fstat(fd, &s3) < 0) die("fstat#3 failed");
    printf("after read:         atime=%d mtime=%d ctime=%d\n",
           s3.atime, s3.mtime, s3.ctime);
    if (s3.atime <= s2.atime) die("atime didn't advance on read");
    if (s3.mtime != s2.mtime) die("mtime moved on read (it shouldn't)");
    if (s3.ctime != s2.ctime) die("ctime moved on read (it shouldn't)");
    close(fd);

    unlink("/sttest");
    printf("stattime ok\n");
    return 0;
}
