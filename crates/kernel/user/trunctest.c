// trunctest — POSIX ftruncate/truncate verification.

#include "user.h"

static void die(const char* msg) {
    printf("trunctest: %s\n", msg);
    exit(1);
}

static int file_size(const char* p) {
    struct stat st;
    if (stat(p, &st) < 0) return -1;
    return (int)st.size;
}

int main(int argc, char* argv[]) {
    // Build a 10-byte file.
    int fd = open("/tr", O_CREATE | O_RDWR);
    if (fd < 0) die("open(/tr) failed");
    if (write(fd, "0123456789", 10) != 10) die("write failed");
    if (file_size("/tr") != 10) die("initial size wrong");
    printf("initial size = 10\n");

    // 1) ftruncate(5) — shrink.
    if (ftruncate(fd, 5) != 0) die("ftruncate(5) failed");
    int sz = file_size("/tr");
    printf("after ftruncate(5): size=%d (expected 5)\n", sz);
    if (sz != 5) die("shrink size wrong");

    // Read back from offset 0 — should be "01234".
    lseek(fd, 0, SEEK_SET);
    char buf[16];
    int n = read(fd, buf, sizeof(buf));
    printf("read after shrink: %d bytes \"%c%c%c%c%c\"\n",
           n, buf[0], buf[1], buf[2], buf[3], buf[4]);
    if (n != 5) die("read len after shrink wrong");

    // 2) ftruncate(20) — grow, leaves a sparse hole.
    if (ftruncate(fd, 20) != 0) die("ftruncate(20) failed");
    sz = file_size("/tr");
    printf("after ftruncate(20): size=%d (expected 20)\n", sz);
    if (sz != 20) die("grow size wrong");

    // Read offsets 5..19 — sparse-hole bytes within the original first
    // block read as zero (xv6 reads zero-init buffers).
    lseek(fd, 0, SEEK_SET);
    n = read(fd, buf, 16);
    printf("read after grow: %d bytes, byte@10=%d (expected 0)\n",
           n, (int)(unsigned char)buf[10]);
    if (n != 16) die("read len after grow wrong");
    if (buf[5] != 0 || buf[10] != 0 || buf[15] != 0) die("hole bytes nonzero");

    close(fd);

    // 3) Path-based truncate(0) — empties the file.
    if (truncate("/tr", 0) != 0) die("truncate(0) failed");
    sz = file_size("/tr");
    printf("after truncate(0): size=%d (expected 0)\n", sz);
    if (sz != 0) die("truncate-to-zero wrong");

    // 4) Permission check: chmod 0400 (read-only), drop priv, fail
    //    to truncate.
    fd = open("/tr", O_WRONLY | O_CREATE);
    write(fd, "abc", 3);
    close(fd);
    chmod("/tr", 0400);
    chown("/tr", 99, 99);

    int pid = fork();
    if (pid == 0) {
        if (setuid(7) < 0) exit(11);
        int r = truncate("/tr", 1);
        printf("non-owner truncate -> %d (expected -1)\n", r);
        if (r != -1) exit(12);
        // ftruncate via an existing fd shouldn't even be openable
        // (chmod 0400 + non-owner: no read since uid != 99, world=0).
        int rfd = open("/tr", O_RDONLY);
        printf("non-owner open(0400 not own) -> %d (expected -1)\n", rfd);
        if (rfd >= 0) exit(13);
        exit(0);
    }
    int status = 0;
    wait(&status);
    if (status != 0) die("child enforcement failed");

    unlink("/tr");
    printf("trunctest ok\n");
    return 0;
}
