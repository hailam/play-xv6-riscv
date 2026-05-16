// mkdir — create a directory.

extern int mkdir(const char*);
extern int write(int, const void*, int);

static int u_strlen(const char* s) { int n = 0; while (s[n]) n++; return n; }

int main(int argc, char** argv) {
    if (argc != 2) {
        const char* msg = "usage: mkdir <path>\n";
        write(2, msg, u_strlen(msg));
        return -1;
    }
    if (mkdir(argv[1]) < 0) {
        write(2, "mkdir: failed\n", 14);
        return -1;
    }
    return 0;
}
