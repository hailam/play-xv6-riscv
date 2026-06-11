//
// kbtest — read evdev-style events from /dev/input/0 (todo 12 M3).
//
// Prints each EV_KEY event as "kbtest: key <code> <down|up>" and
// exits after the first key release. Driven by the host harness via
// QMP send-key (which routes only to the virtio keyboard — the
// serial console is a separate path, so the shell stays clean).
//
#include "user.h"

#define EV_KEY 0x01

struct input_event {
  unsigned short type;
  unsigned short code;
  unsigned int value;
};

int
main(void)
{
  int fd = open("/dev/input/0", O_RDONLY);
  if(fd < 0){ printf("kbtest: open /dev/input/0 failed\n"); exit(1); }
  printf("kbtest: waiting for keys\n");
  struct input_event ev;
  for(;;){
    int n = read(fd, &ev, sizeof(ev));
    if(n < 0){ printf("kbtest: read failed\n"); exit(1); }
    if(n != sizeof(ev))
      continue; // partial event (shouldn't happen with 8-byte reads)
    if(ev.type != EV_KEY)
      continue; // ignore EV_SYN etc.
    printf("kbtest: key %d %s\n", ev.code, ev.value ? "down" : "up");
    if(ev.value == 0){
      printf("kbtest: done\n");
      exit(0);
    }
  }
}
