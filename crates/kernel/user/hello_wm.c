//
// hello — demo client (todo 12 M4): a cyan window that turns red on
// the first key press routed to it by the display server.
//
#include "user.h"
#include "wm.h"

#define W 120
#define H 90
#define EV_KEY 0x01

struct input_event { unsigned short type, code; unsigned int value; };

static uint px[W];

static void
blit_solid(int fd, uint color)
{
  uint hdr[5] = { WM_BLIT, 0, 0, W, H };
  writen(fd, hdr, sizeof(hdr));
  for(int i = 0; i < W; i++)
    px[i] = color;
  for(int r = 0; r < H; r++)
    writen(fd, px, sizeof(px));
}

int
main(void)
{
  int s = socket(1, 1, 0);
  if(s < 0 || connect(s, WM_SOCK) < 0){
    printf("hello: connect failed\n");
    exit(1);
  }
  uint msg[4] = { WM_CREATE, W, H, 1 }; // want_keys
  writen(s, msg, sizeof(msg));
  uint id;
  if(readn(s, &id, 4) != 4){
    printf("hello: create failed\n");
    exit(1);
  }
  blit_solid(s, 0x0000FFFFu); // cyan
  printf("hello: mapped\n");
  for(;;){
    struct input_event ev;
    if(readn(s, &ev, 8) != 8)
      exit(0); // server gone
    if(ev.type == EV_KEY && ev.value == 1){
      blit_solid(s, 0x00FF0000u); // red
      printf("hello: key %d\n", ev.code);
    }
  }
}
