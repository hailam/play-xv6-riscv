// ls — list a directory entry by entry, or stat a single file.

extern int open(const char*, int);
extern int close(int);
extern int read(int, void*, int);
extern int write(int, const void*, int);
extern int fstat(int, void*);

#define O_RDONLY 0
#define T_DIR    1
#define T_FILE   2
#define T_DEVICE 3

#define DIRSIZ   14

struct stat {
    int          dev;
    unsigned int ino;
    short        type;
    short        nlink;
    unsigned int _pad;
    unsigned long size;
};

struct dirent {
    unsigned short inum;
    char           name[DIRSIZ];
};

static int u_strlen(const char* s) {
    int n = 0;
    while (s[n]) n++;
    return n;
}

static void puts_raw(const char* s) {
    write(1, s, u_strlen(s));
}

static void put_u64(unsigned long n) {
    char buf[24];
    int  i = 0;
    if (n == 0) {
        buf[i++] = '0';
    } else {
        while (n > 0) {
            buf[i++] = (char)('0' + (n % 10));
            n /= 10;
        }
    }
    while (i--) write(1, &buf[i], 1);
}

static const char* typestr(short t) {
    switch (t) {
        case T_DIR:    return "DIR ";
        case T_FILE:   return "FILE";
        case T_DEVICE: return "DEV ";
        default:       return "??? ";
    }
}

static void print_entry(const char* name, const struct stat* st) {
    puts_raw(typestr(st->type));
    write(1, "  inum=", 7);
    put_u64(st->ino);
    write(1, "  size=", 7);
    put_u64(st->size);
    write(1, "  ", 2);
    puts_raw(name);
    write(1, "\n", 1);
}

int main(int argc, char** argv) {
    const char* path = (argc >= 2) ? argv[1] : "/";

    int fd = open(path, O_RDONLY);
    if (fd < 0) {
        puts_raw("ls: cannot open ");
        puts_raw(path);
        write(1, "\n", 1);
        return -1;
    }

    struct stat st;
    if (fstat(fd, &st) < 0) {
        puts_raw("ls: cannot stat ");
        puts_raw(path);
        write(1, "\n", 1);
        close(fd);
        return -1;
    }

    if (st.type != T_DIR) {
        print_entry(path, &st);
        close(fd);
        return 0;
    }

    struct dirent de;
    while (read(fd, &de, sizeof(de)) == sizeof(de)) {
        if (de.inum == 0) continue;
        // Open the child by inode? we don't have that — open by name.
        // We re-stat by opening each name. The directory contains
        // null-terminated short names.
        // Build "path/name" or just "/name" if path == "/".
        char full[64];
        int  i = 0;
        if (path[0] == '/' && path[1] == 0) {
            full[i++] = '/';
        } else {
            int pl = u_strlen(path);
            for (int j = 0; j < pl && i < 60; j++) full[i++] = path[j];
            full[i++] = '/';
        }
        for (int j = 0; j < DIRSIZ && de.name[j] && i < 63; j++) {
            full[i++] = de.name[j];
        }
        full[i] = 0;

        int cfd = open(full, O_RDONLY);
        if (cfd < 0) {
            puts_raw("ls: open failed for ");
            puts_raw(full);
            write(1, "\n", 1);
            continue;
        }
        struct stat cst;
        if (fstat(cfd, &cst) < 0) {
            close(cfd);
            continue;
        }
        // Pretty-print only the leaf name.
        char leaf[DIRSIZ + 1];
        int  k = 0;
        for (; k < DIRSIZ && de.name[k]; k++) leaf[k] = de.name[k];
        leaf[k] = 0;
        print_entry(leaf, &cst);
        close(cfd);
    }

    close(fd);
    return 0;
}
