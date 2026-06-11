//
// init — the first real user process (pid 1). The kernel's embedded
// initcode execs us. Two jobs, exactly like xv6's init:
//   * keep a shell running (restart it if it exits), and
//   * sit in wait() so orphaned children the kernel reparents to us
//     get reaped promptly (their exit status is discarded).
//
// fds 0/1/2 arrive pre-wired to the console (the kernel populates the
// first proc's fd table), so unlike xv6 we don't open("console").
//

#include "user.h"

// Device-node majors (must match crate::uapi).
#define FB_MAJOR 1
#define INPUT_MAJOR 2

int
main(void)
{
  // Populate /dev (idempotent — these may already exist on a
  // persistent fs.img; mkdir/mknod just fail harmlessly then). The
  // framebuffer node only works if the kernel brought ramfb up, but
  // creating the node is cheap regardless.
  mkdir("/dev");
  mknod("/dev/fb0", FB_MAJOR, 0);
  mkdir("/dev/input");
  mknod("/dev/input/0", INPUT_MAJOR, 0);

  for(;;){
    int shpid = fork();
    if(shpid < 0){
      printf("init: fork failed\n");
      sleep(10);
      continue;
    }
    if(shpid == 0){
      char *argv[] = { "sh", 0 };
      exec("/sh", argv);
      printf("init: exec /sh failed\n");
      exit(1);
    }
    for(;;){
      int st;
      int wpid = wait(&st);
      if(wpid == shpid){
        printf("init: sh exited, restarting\n");
        break;
      }
      if(wpid < 0){
        printf("init: wait error %d\n", wpid);
        sleep(10);
        break;
      }
      // Otherwise: an orphan we inherited — nothing to do.
    }
  }
}
