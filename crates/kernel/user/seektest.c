// seektest — quick sanity check for sys_lseek.
//
// Opens README, prints its size via fstat, then exercises SEEK_SET /
// SEEK_CUR / SEEK_END and reads one byte at each position to confirm
// the offset really moved.

#include "user.h"

static void die(const char* msg) {
    printf("seektest: %s\n", msg);
    exit(1);
}

int main(int argc, char* argv[]) {
    int fd = open("README", O_RDONLY);
    if (fd < 0) die("open(README) failed");

    struct stat st;
    if (fstat(fd, &st) < 0) die("fstat failed");
    printf("README size = %d bytes\n", (int)st.size);

    // 1) lseek(0, SEEK_END) → size
    long n = lseek(fd, 0, SEEK_END);
    printf("SEEK_END(0) -> %d\n", (int)n);
    if (n != (long)st.size) die("SEEK_END mismatch");

    // 2) lseek(0, SEEK_SET) → 0
    n = lseek(fd, 0, SEEK_SET);
    printf("SEEK_SET(0) -> %d\n", (int)n);
    if (n != 0) die("SEEK_SET 0 mismatch");

    // 3) Read first byte at offset 0.
    char b0 = 0;
    if (read(fd, &b0, 1) != 1) die("read#1 failed");
    printf("byte@0 = '%c' (0x%x)\n", b0, b0);

    // 4) lseek(5, SEEK_SET), read one byte — should be README[5].
    n = lseek(fd, 5, SEEK_SET);
    printf("SEEK_SET(5) -> %d\n", (int)n);
    char b5 = 0;
    if (read(fd, &b5, 1) != 1) die("read#2 failed");
    printf("byte@5 = '%c' (0x%x)\n", b5, b5);

    // 5) lseek(-3, SEEK_CUR) — we're at offset 6 now, so go to 3.
    n = lseek(fd, -3, SEEK_CUR);
    printf("SEEK_CUR(-3) -> %d (expected 3)\n", (int)n);

    // 6) Negative absolute offset — should fail.
    n = lseek(fd, -1, SEEK_SET);
    printf("SEEK_SET(-1) -> %d (expected -1)\n", (int)n);
    if (n != -1) die("negative offset accepted");

    // 7) pread at offset 5 — should match byte@5 above ('t')
    //    AND must NOT touch the file's offset (we left it at 3
    //    after SEEK_CUR(-3); pread at 5 leaves it at 3).
    long where = lseek(fd, 0, SEEK_CUR);
    printf("SEEK_CUR(0) -> %d (expected 3)\n", (int)where);
    char pb = 0;
    int got = pread(fd, &pb, 1, 5);
    printf("pread(1@5) = %d, byte = '%c'\n", got, pb);
    if (got != 1 || pb != 't') die("pread byte mismatch");
    long after = lseek(fd, 0, SEEK_CUR);
    printf("offset after pread -> %d (expected 3, should be untouched)\n", (int)after);
    if (after != 3) die("pread moved the offset");

    close(fd);

    // 8) pwrite test against a fresh file in /tmp.
    int wfd = open("/seekwrite", O_CREATE | O_RDWR);
    if (wfd < 0) die("open(/seekwrite) failed");
    if (pwrite(wfd, "AB", 2, 0) != 2) die("pwrite(0) failed");
    if (pwrite(wfd, "Z", 1, 4) != 1) die("pwrite(4) failed");
    char rb[8];
    memset(rb, '_', sizeof(rb));
    int got2 = pread(wfd, rb, 5, 0);
    printf("pwrite-then-pread: got=%d bytes=[%c,%c,%c,%c,%c]\n",
           got2, rb[0], rb[1], rb[2], rb[3], rb[4]);
    if (got2 != 5 || rb[0] != 'A' || rb[1] != 'B' || rb[4] != 'Z')
        die("pwrite/pread data mismatch");
    close(wfd);
    unlink("/seekwrite");

    // 9) O_APPEND — every write should land at EOF regardless of
    //    the fd's offset. Open fresh, write "1", lseek to 0, write
    //    "2" — file should end up "12" not "2".
    int afd = open("/appendf", O_CREATE | O_RDWR | O_APPEND);
    if (afd < 0) die("open(/appendf) failed");
    if (write(afd, "1", 1) != 1) die("append write#1 failed");
    if (lseek(afd, 0, SEEK_SET) != 0) die("lseek append failed");
    if (write(afd, "2", 1) != 1) die("append write#2 failed");
    char ab[4];
    memset(ab, '_', sizeof(ab));
    if (lseek(afd, 0, SEEK_SET) != 0) die("rewind for read failed");
    int got3 = read(afd, ab, sizeof(ab));
    printf("O_APPEND: wrote 1, rewind, wrote 2 -> read %d bytes \"%c%c\" (expected 12)\n",
           got3, ab[0], ab[1]);
    if (got3 != 2 || ab[0] != '1' || ab[1] != '2') die("O_APPEND mismatch");
    close(afd);
    unlink("/appendf");

    // 10) Path-based stat() — open-less version of fstat. Should
    //     return the same size as we measured via fstat above.
    struct stat st2;
    if (stat("README", &st2) < 0) die("stat(README) failed");
    printf("stat(README): size=%d nlink=%d type=%d\n",
           (int)st2.size, st2.nlink, st2.type);
    if (st2.size != st.size) die("stat size differs from fstat");
    // Non-existent path should return -1.
    if (stat("/nonexistent-xyz", &st2) != -1)
        die("stat(/nonexistent) should fail");
    printf("stat(/nonexistent) -> -1 OK\n");

    printf("seektest ok\n");
    return 0;
}
