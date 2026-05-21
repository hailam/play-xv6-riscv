// credtest — POSIX credentials + umask + open-permission enforcement.

#include "user.h"

static void die(const char* msg) {
    printf("credtest: %s\n", msg);
    exit(1);
}

int main(int argc, char* argv[]) {
    // 1) Initial uid/gid should be 0 (root) — init/sh inherits.
    uint uid0 = getuid();
    uint gid0 = getgid();
    printf("initial uid=%d gid=%d (expected 0,0)\n", uid0, gid0);
    if (uid0 != 0 || gid0 != 0) die("init not root");

    // 2) umask — default 0o022 from Proc::with_layout.
    uint prev = umask(0077);
    printf("umask: previous=0%o (expected 022)\n", prev);
    if (prev != 0022) die("default umask != 022");

    // 3) Create a file under umask 0077 — should be 0666 & ~077 = 0600.
    int fd = open("/credfile", O_CREATE | O_RDWR);
    if (fd < 0) die("open(/credfile) failed");
    write(fd, "hello", 5);
    close(fd);

    struct stat st;
    if (stat("/credfile", &st) < 0) die("stat(/credfile) failed");
    printf("created under umask 077: mode=0%o (expected 100600)\n", st.mode);
    if ((st.mode & 0777) != 0600) die("umask 077 didn't take");
    if (st.uid != 0 || st.gid != 0) die("file not owned by root");

    // Restore umask for the rest.
    umask(prev);

    // 4) chown the file to uid=42, then test enforcement after setuid.
    if (chown("/credfile", 42, 99) < 0) die("chown failed");
    if (stat("/credfile", &st) < 0) die("stat after chown failed");
    if (st.uid != 42 || st.gid != 99) die("chown didn't take");

    // 5) chmod to 0600 (owner-only rw). Then drop privilege and try to
    //    open as a non-owner non-root — must fail.
    if (chmod("/credfile", 0600) < 0) die("chmod 0600 failed");

    int pid = fork();
    if (pid < 0) die("fork failed");
    if (pid == 0) {
        // Child: drop to uid=1234 (not owner). Open should fail.
        if (setuid(1234) < 0) {
            printf("child: setuid(1234) failed unexpectedly\n");
            exit(1);
        }
        printf("child uid after setuid = %d (expected 1234)\n", getuid());
        int rfd = open("/credfile", O_RDONLY);
        printf("child open(O_RDONLY 0600 owned-by-42) -> %d (expected -1)\n", rfd);
        if (rfd >= 0) {
            close(rfd);
            exit(2);
        }
        // chmod by non-owner non-root should also fail.
        int cm = chmod("/credfile", 0644);
        printf("child chmod -> %d (expected -1)\n", cm);
        if (cm == 0) exit(3);
        // setuid back to 0 should fail (only root can setuid).
        int bk = setuid(0);
        printf("child setuid(0) -> %d (expected -1)\n", bk);
        if (bk == 0) exit(4);
        exit(0);
    }
    int status = 0;
    wait(&status);
    printf("child wait status=%d (expected 0)\n", status);
    if (status != 0) die("child failed enforcement assertions");

    // 6) Back in the parent (still uid=0). chmod world-readable then
    //    confirm a non-owner can open.
    if (chmod("/credfile", 0604) < 0) die("chmod 0604 failed");
    pid = fork();
    if (pid < 0) die("fork#2 failed");
    if (pid == 0) {
        if (setuid(5678) < 0) exit(11);
        int rfd = open("/credfile", O_RDONLY);
        printf("child2 open(0604 world-read) -> %d (expected >=0)\n", rfd);
        if (rfd < 0) exit(12);
        close(rfd);
        // Write should still be denied (world bit is r--).
        int wfd = open("/credfile", O_WRONLY);
        printf("child2 open(O_WRONLY 0604) -> %d (expected -1)\n", wfd);
        if (wfd >= 0) { close(wfd); exit(13); }
        exit(0);
    }
    wait(&status);
    printf("child2 wait status=%d (expected 0)\n", status);
    if (status != 0) die("child2 failed");

    // Cleanup.
    unlink("/credfile");

    printf("credtest ok\n");
    return 0;
}
