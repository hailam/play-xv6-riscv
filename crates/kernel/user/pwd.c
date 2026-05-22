// pwd — print the current working directory.

#include "user.h"

int main(int argc, char* argv[]) {
    char buf[256];
    int n = getcwd(buf, sizeof(buf));
    if (n < 0) {
        const char* err = "pwd: getcwd failed\n";
        int i = 0; while (err[i]) i++;
        write(2, err, i);
        return 1;
    }
    write(1, buf, n);
    write(1, "\n", 1);
    return 0;
}
