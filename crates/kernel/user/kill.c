// kill — send SIGTERM to a pid argument.

#include "user.h"

static int u_strlen(const char* s) { int n = 0; while (s[n]) n++; return n; }

int main(int argc, char** argv) {
    if (argc != 2) {
        const char* msg = "usage: kill <pid>\n";
        write(2, msg, u_strlen(msg));
        return -1;
    }
    int pid = 0;
    for (const char* p = argv[1]; *p; p++) {
        if (*p < '0' || *p > '9') return -1;
        pid = pid * 10 + (*p - '0');
    }
    if (kill(pid, SIGTERM) < 0) {
        write(2, "kill: failed\n", 13);
        return -1;
    }
    return 0;
}
