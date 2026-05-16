// rm — unlink a file (or empty directory).

extern int unlink(const char*);
extern int write(int, const void*, int);

static int u_strlen(const char* s) { int n = 0; while (s[n]) n++; return n; }

int main(int argc, char** argv) {
    if (argc < 2) {
        const char* msg = "usage: rm <path>...\n";
        write(2, msg, u_strlen(msg));
        return -1;
    }
    int rc = 0;
    for (int i = 1; i < argc; i++) {
        if (unlink(argv[i]) < 0) {
            write(2, "rm: failed: ", 12);
            write(2, argv[i], u_strlen(argv[i]));
            write(2, "\n", 1);
            rc = -1;
        }
    }
    return rc;
}
