// fdfiletest — dup2, getcwd, rename round-trip.

#include "user.h"

static void die(const char* msg) {
    printf("fdfiletest: %s\n", msg);
    exit(1);
}

int main(int argc, char* argv[]) {
    // 1) dup2: open a file, dup2 it onto fd 5, write through fd 5,
    //    read back via the original fd.
    int fd = open("/dup2tmp", O_CREATE | O_RDWR);
    if (fd < 0) die("open(/dup2tmp) failed");
    int newfd = dup2(fd, 5);
    printf("dup2(fd=%d, 5) -> %d (expected 5)\n", fd, newfd);
    if (newfd != 5) die("dup2 didn't land on 5");
    // Write via newfd, read via fd. The position is shared because
    // both fds point at the same File::Inode-with-cloned-offset...
    // wait — our File::clone gives independent offsets (Atomic copy).
    // So write at fd=5 advances 5's offset, read at fd starts at 0.
    if (write(newfd, "abc", 3) != 3) die("write via newfd failed");
    char buf[8] = {0};
    int n = read(fd, buf, 3);
    printf("read via orig fd: n=%d buf=\"%c%c%c\"\n", n, buf[0], buf[1], buf[2]);
    if (n != 3 || buf[0] != 'a' || buf[1] != 'b' || buf[2] != 'c')
        die("dup2 file contents wrong");

    // dup2(fd, fd) is a no-op.
    int same = dup2(fd, fd);
    if (same != fd) die("dup2(fd, fd) didn't return fd");

    close(fd);
    close(newfd);
    unlink("/dup2tmp");

    // 2) getcwd: should be "/" initially.
    char cwd[64];
    int g = getcwd(cwd, sizeof(cwd));
    printf("getcwd at root -> %d \"%s\"\n", g, cwd);
    if (g < 0) die("getcwd at root failed");
    if (cwd[0] != '/' || cwd[1] != 0) die("cwd not \"/\"");

    // 3) Make a directory, chdir into it, getcwd.
    mkdir("/fdftdir");
    if (chdir("/fdftdir") < 0) die("chdir failed");
    g = getcwd(cwd, sizeof(cwd));
    printf("getcwd after chdir -> %d \"%s\"\n", g, cwd);
    if (g < 0) die("getcwd in subdir failed");
    // Should be "/fdftdir".
    {
        const char* expect = "/fdftdir";
        for (int i = 0; expect[i] || cwd[i]; i++)
            if (expect[i] != cwd[i]) die("cwd != /fdftdir");
    }

    // 4) Rename within the cwd: create a file, rename it, open the
    //    new name to verify.
    int f = open("aaa", O_CREATE | O_RDWR);
    if (f < 0) die("open(aaa) failed");
    write(f, "x", 1);
    close(f);
    int r = rename("aaa", "bbb");
    printf("rename(aaa, bbb) -> %d (expected 0)\n", r);
    if (r != 0) die("rename failed");
    if (open("aaa", O_RDONLY) >= 0) die("aaa still openable after rename");
    int b = open("bbb", O_RDONLY);
    printf("open(bbb) after rename -> %d\n", b);
    if (b < 0) die("bbb not openable after rename");
    char rb[2] = {0};
    read(b, rb, 1);
    if (rb[0] != 'x') die("rename lost file content");
    close(b);
    unlink("bbb");

    // 5) chdir back, remove the subdir.
    chdir("/");
    unlink("/fdftdir");

    printf("fdfiletest ok\n");
    return 0;
}
