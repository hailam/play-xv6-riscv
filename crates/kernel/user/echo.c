// echo — print argv[1..] separated by spaces, then a newline.

extern int write(int, const void*, int);

static int u_strlen(const char* s) {
    int n = 0;
    while (s[n]) n++;
    return n;
}

int main(int argc, char** argv) {
    for (int i = 1; i < argc; i++) {
        write(1, argv[i], u_strlen(argv[i]));
        if (i + 1 < argc) write(1, " ", 1);
    }
    write(1, "\n", 1);
    return 0;
}
