// env — print the environment, one VAR=VALUE per line.
//
// With no args, behaves like POSIX `env`: prints environ.
// With an arg of the form NAME=VALUE, executes argv[2..] (if any)
// with that variable added — but we keep it minimal: no exec form,
// just printing.

#include "user.h"

static int u_strlen(const char* s) {
    int n = 0;
    while (s[n]) n++;
    return n;
}

int main(int argc, char* argv[]) {
    // Strip "env" itself. If a NAME=VALUE assignment is passed,
    // setenv it and print.
    for (int i = 1; i < argc; i++) {
        char* eq = argv[i];
        while (*eq && *eq != '=') eq++;
        if (*eq != '=') break;
        *eq = 0;
        if (setenv(argv[i], eq + 1, 1) < 0) {
            write(2, "env: setenv failed\n", 19);
            return 1;
        }
        *eq = '=';
    }
    if (!environ) {
        // No env set up — print nothing.
        return 0;
    }
    for (char** p = environ; *p; p++) {
        write(1, *p, u_strlen(*p));
        write(1, "\n", 1);
    }
    return 0;
}
