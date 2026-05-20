// ln — create a hard link.

extern int link(const char*, const char*);
extern int write(int, const void*, int);

static int u_strlen(const char* s) { int n = 0; while (s[n]) n++; return n; }

int main(int argc, char** argv) {
    if (argc != 3) {
        const char* m = "usage: ln <old> <new>\n";
        write(2, m, u_strlen(m));
        return -1;
    }
    if (link(argv[1], argv[2]) < 0) {
        write(2, "ln: failed\n", 11);
        return -1;
    }
    return 0;
}
