// chmodtest — verify mode/uid/gid round-trip through chmod, stat,
// chown.

#include "user.h"

static void die(const char* msg) {
    printf("chmodtest: %s\n", msg);
    exit(1);
}

int main(int argc, char* argv[]) {
    // 1) Stat README — should report S_IFREG and a known mode.
    struct stat st;
    if (stat("README", &st) < 0) die("stat(README) failed");
    printf("README: mode=0%o uid=%d gid=%d type=%d\n",
           st.mode, st.uid, st.gid, st.type);
    if (!S_ISREG(st.mode)) die("README not S_IFREG");
    if ((st.mode & 0777) != 0644) die("README perm != 0644");

    // 2) Create a new file via open(O_CREATE) — should get 0644.
    int fd = open("/cmtest", O_CREATE | O_RDWR);
    if (fd < 0) die("open(/cmtest) failed");
    write(fd, "x", 1);
    close(fd);
    if (stat("/cmtest", &st) < 0) die("stat(/cmtest) failed");
    printf("created: mode=0%o (expected 100644)\n", st.mode);
    if ((st.mode & 0777) != 0644) die("created perm != 0644");

    // 3) chmod to 0600, re-stat.
    if (chmod("/cmtest", 0600) < 0) die("chmod 0600 failed");
    if (stat("/cmtest", &st) < 0) die("stat after chmod failed");
    printf("chmod 0600: mode=0%o\n", st.mode);
    if ((st.mode & 0777) != 0600) die("chmod 0600 didn't take");

    // 4) chmod 0755, re-stat. Verify S_IFREG sticks (type bits untouched).
    if (chmod("/cmtest", 0755) < 0) die("chmod 0755 failed");
    if (stat("/cmtest", &st) < 0) die("stat#3 failed");
    printf("chmod 0755: mode=0%o\n", st.mode);
    if ((st.mode & 0777) != 0755) die("chmod 0755 didn't take");
    if (!S_ISREG(st.mode)) die("file type bits lost after chmod");

    // 5) chown to uid=42 gid=99.
    if (chown("/cmtest", 42, 99) < 0) die("chown failed");
    if (stat("/cmtest", &st) < 0) die("stat after chown failed");
    printf("chown 42:99: uid=%d gid=%d\n", st.uid, st.gid);
    if (st.uid != 42 || st.gid != 99) die("chown didn't take");

    // 6) chown with -1 (u16::MAX) sentinels — should leave one field
    //    untouched.
    if (chown("/cmtest", 7, (ushort)-1) < 0) die("chown gid=-1 failed");
    if (stat("/cmtest", &st) < 0) die("stat after partial chown failed");
    printf("chown 7:-1: uid=%d gid=%d (expected 7,99)\n", st.uid, st.gid);
    if (st.uid != 7 || st.gid != 99) die("partial chown didn't preserve gid");

    // 7) Make a directory — should be S_IFDIR with 0755.
    if (mkdir("/cmdir") < 0) die("mkdir failed");
    if (stat("/cmdir", &st) < 0) die("stat(/cmdir) failed");
    printf("mkdir: mode=0%o type=%d\n", st.mode, st.type);
    if (!S_ISDIR(st.mode)) die("mkdir not S_IFDIR");
    if ((st.mode & 0777) != 0755) die("mkdir perm != 0755");

    unlink("/cmtest");
    unlink("/cmdir");

    printf("chmodtest ok\n");
    return 0;
}
