// cat — copy stdin to stdout until EOF.

extern int read(int, void*, int);
extern int write(int, const void*, int);

static char buf[64];

int main(void) {
    for (;;) {
        int n = read(0, buf, (int)sizeof(buf));
        if (n <= 0) return 0;
        write(1, buf, n);
    }
}
