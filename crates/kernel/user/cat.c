// cat — copy stdin (or each file argument) to stdout until EOF.

extern int open(const char*, int);
extern int close(int);
extern int read(int, void*, int);
extern int write(int, const void*, int);

#define O_RDONLY 0

static char buf[64];

static int u_strlen(const char* s) { int n = 0; while (s[n]) n++; return n; }

static void dump(int fd) {
    for (;;) {
        int n = read(fd, buf, (int)sizeof(buf));
        if (n <= 0) return;
        write(1, buf, n);
    }
}

int main(int argc, char** argv) {
    if (argc == 1) {
        dump(0);
        return 0;
    }
    for (int i = 1; i < argc; i++) {
        int fd = open(argv[i], O_RDONLY);
        if (fd < 0) {
            write(2, "cat: open failed: ", 18);
            write(2, argv[i], u_strlen(argv[i]));
            write(2, "\n", 1);
            continue;
        }
        dump(fd);
        close(fd);
    }
    return 0;
}
