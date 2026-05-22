// symlinktest — symlink/readlink/lstat + transparent follow on open.

#include "user.h"

static int u_strcmp(const char* a, const char* b) {
    while (*a && *a == *b) { a++; b++; }
    return (unsigned char)*a - (unsigned char)*b;
}

static void die(const char* msg) {
    printf("symlinktest: %s\n", msg);
    exit(1);
}

int main(int argc, char* argv[]) {
    // 1) Create a target file and a symlink to it.
    int fd = open("/slt-target", O_CREATE | O_RDWR);
    if (fd < 0) die("open target failed");
    write(fd, "payload", 7);
    close(fd);

    if (symlink("/slt-target", "/slt-link") != 0)
        die("symlink failed");

    // 2) lstat on the symlink reports T_SYMLINK + S_IFLNK in mode.
    struct stat st;
    if (lstat("/slt-link", &st) < 0) die("lstat failed");
    printf("lstat link: type=%d mode=0%o (expect type=4 S_IFLNK)\n",
           st.type, st.mode);
    if (st.type != 4) die("lstat type != T_SYMLINK");
    if (!S_ISLNK(st.mode)) die("lstat mode missing S_IFLNK");

    // 3) stat on the symlink follows to the target (T_FILE).
    if (stat("/slt-link", &st) < 0) die("stat through link failed");
    printf("stat link (followed): type=%d size=%d (expect type=2 size=7)\n",
           st.type, (int)st.size);
    if (st.type != 2) die("stat didn't follow link");
    if (st.size != 7) die("followed size wrong");

    // 4) readlink returns the target string (no NUL).
    char buf[64];
    int n = readlink("/slt-link", buf, sizeof(buf));
    buf[n] = 0;
    printf("readlink -> %d \"%s\" (expect \"/slt-target\")\n", n, buf);
    if (n != 11 || u_strcmp(buf, "/slt-target") != 0) die("readlink wrong");

    // 5) open through the symlink reads the target's contents.
    fd = open("/slt-link", O_RDONLY);
    if (fd < 0) die("open through link failed");
    char rb[16] = {0};
    int got = read(fd, rb, sizeof(rb));
    close(fd);
    printf("read through link: %d \"%s\"\n", got, rb);
    if (got != 7 || u_strcmp(rb, "payload") != 0)
        die("transparent follow on open didn't read target");

    // 6) Loop detection: link → link2 → link. Open should fail
    //    rather than hang.
    if (symlink("/slt-loop2", "/slt-loop1") != 0)
        die("symlink loop1 failed");
    if (symlink("/slt-loop1", "/slt-loop2") != 0)
        die("symlink loop2 failed");
    fd = open("/slt-loop1", O_RDONLY);
    printf("open through loop -> %d (expect -1)\n", fd);
    if (fd >= 0) die("opened a symlink loop");

    // Cleanup.
    unlink("/slt-link");
    unlink("/slt-target");
    unlink("/slt-loop1");
    unlink("/slt-loop2");

    printf("symlinktest ok\n");
    return 0;
}
