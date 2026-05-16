// wr — create a file (truncating any existing) and write argv[2..] to it.
// Useful in lieu of a real shell `>` redirect for now.
//
//   wr /greet hello world

extern int open(const char*, int);
extern int close(int);
extern int write(int, const void*, int);

#define O_WRONLY 0x001
#define O_CREATE 0x200
#define O_TRUNC  0x400

static int u_strlen(const char* s) { int n = 0; while (s[n]) n++; return n; }

int main(int argc, char** argv) {
    if (argc < 2) {
        const char* msg = "usage: wr <path> [words...]\n";
        write(2, msg, u_strlen(msg));
        return -1;
    }
    int fd = open(argv[1], O_WRONLY | O_CREATE | O_TRUNC);
    if (fd < 0) {
        write(2, "wr: open failed\n", 16);
        return -1;
    }
    for (int i = 2; i < argc; i++) {
        write(fd, argv[i], u_strlen(argv[i]));
        if (i + 1 < argc) write(fd, " ", 1);
    }
    write(fd, "\n", 1);
    close(fd);
    return 0;
}
