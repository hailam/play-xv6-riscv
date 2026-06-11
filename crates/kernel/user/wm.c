//
// wm — the display server (todo 12 M4). A plain user process:
//   * owns /dev/fb0 (composites windows onto it)
//   * owns /dev/input/0 (routes keys to the focused client)
//   * listens on the AF_UNIX socket /wm.sock for clients
//
// Windows tile horizontally in fixed 168px slots with a 2px white
// border. Focus = most recently created want_keys window. poll()
// multiplexes input + listener + client fds — the whole server is
// one cooperative loop.
//
#include "user.h"
#include "wm.h"

#define FBIOGET_DIMS 0x4600
#define MAXWIN 4
#define SLOT_W 168
#define EV_KEY 0x01

struct fb_dims { uint width, height, stride, bpp; };

struct win {
  int fd;        // client socket (-1 = free)
  int x, y, w, h;
  int want_keys;
};

static int fb;
static struct fb_dims dims;
static struct win wins[MAXWIN];
static char rowbuf[640 * 4];

static void
fb_fill(int x, int y, int w, int h, uint color)
{
  if(w > 640) w = 640;
  for(int i = 0; i < w; i++)
    ((uint*)rowbuf)[i] = color;
  for(int r = 0; r < h; r++){
    lseek(fb, (long)(y + r) * dims.stride + (long)x * 4, 0);
    write(fb, rowbuf, w * 4);
  }
}

static void
close_win(struct win *wn)
{
  // Clear the window (incl. border) back to the desktop color.
  fb_fill(wn->x - 2, wn->y - 2, wn->w + 4, wn->h + 4, 0x00303030u);
  close(wn->fd);
  wn->fd = -1;
}

static void
handle_client(struct win *wn)
{
  uint hdr[5];
  if(readn(wn->fd, hdr, 4) <= 0){ close_win(wn); return; }
  if(hdr[0] == WM_BLIT){
    if(readn(wn->fd, hdr + 1, 16) <= 0){ close_win(wn); return; }
    int x = hdr[1], y = hdr[2], w = hdr[3], h = hdr[4];
    if(x < 0 || y < 0 || w <= 0 || x + w > wn->w || (uint)w * 4 > sizeof(rowbuf)){
      printf("wm: bad blit\n");
      close_win(wn);
      return;
    }
    for(int r = 0; r < h; r++){
      if(readn(wn->fd, rowbuf, w * 4) <= 0){ close_win(wn); return; }
      if(y + r >= wn->h)
        continue; // clip rows past the window edge (still drain)
      lseek(fb, (long)(wn->y + y + r) * dims.stride + (long)(wn->x + x) * 4, 0);
      write(fb, rowbuf, w * 4);
    }
  } else {
    printf("wm: bad opcode %d\n", hdr[0]);
    close_win(wn);
  }
}

int
main(void)
{
  fb = open("/dev/fb0", O_RDWR);
  if(fb < 0 || ioctl(fb, FBIOGET_DIMS, &dims) < 0){
    printf("wm: no framebuffer\n");
    exit(1);
  }
  int kbd = open("/dev/input/0", O_RDONLY);
  if(kbd < 0){
    printf("wm: no keyboard\n");
    exit(1);
  }
  unlink(WM_SOCK); // stale node from a previous run
  int ls = socket(1, 1, 0);
  if(ls < 0 || bind(ls, WM_SOCK) < 0 || listen(ls, 4) < 0){
    printf("wm: socket setup failed\n");
    exit(1);
  }
  for(int i = 0; i < MAXWIN; i++)
    wins[i].fd = -1;

  // Desktop background.
  fb_fill(0, 0, dims.width, dims.height, 0x00303030u);
  printf("wm: ready (%dx%d)\n", dims.width, dims.height);

  for(;;){
    struct pollfd pfd[2 + MAXWIN];
    int map[2 + MAXWIN];
    int n = 0;
    pfd[n].fd = kbd; pfd[n].events = POLLIN; map[n++] = -1;
    pfd[n].fd = ls;  pfd[n].events = POLLIN; map[n++] = -2;
    for(int i = 0; i < MAXWIN; i++){
      if(wins[i].fd >= 0){
        pfd[n].fd = wins[i].fd; pfd[n].events = POLLIN; map[n++] = i;
      }
    }
    if(poll(pfd, n, 1000) < 0){
      printf("wm: poll failed\n");
      exit(1);
    }
    for(int k = 0; k < n; k++){
      if(!(pfd[k].revents & (POLLIN | POLLHUP)))
        continue;
      if(map[k] == -1){
        // Keyboard: route raw events to the focused window (most
        // recently created want_keys client).
        char ev[8];
        if(readn(kbd, ev, 8) != 8)
          continue;
        for(int i = MAXWIN - 1; i >= 0; i--){
          if(wins[i].fd >= 0 && wins[i].want_keys){
            writen(wins[i].fd, ev, 8);
            break;
          }
        }
      } else if(map[k] == -2){
        // New client.
        int cfd = accept(ls);
        if(cfd < 0)
          continue;
        uint msg[4];
        if(readn(cfd, msg, 16) <= 0 || msg[0] != WM_CREATE){
          close(cfd);
          continue;
        }
        int slot = -1;
        for(int i = 0; i < MAXWIN; i++)
          if(wins[i].fd < 0){ slot = i; break; }
        uint maxw = SLOT_W - 8, maxh = dims.height - 16;
        if(slot < 0 || msg[1] == 0 || msg[1] > maxw || msg[2] > maxh){
          close(cfd);
          continue;
        }
        struct win *wn = &wins[slot];
        wn->fd = cfd;
        wn->w = msg[1];
        wn->h = msg[2];
        wn->want_keys = msg[3];
        wn->x = 8 + slot * SLOT_W;
        wn->y = 8;
        // Border + blank interior, then tell the client its id.
        fb_fill(wn->x - 2, wn->y - 2, wn->w + 4, wn->h + 4, 0x00FFFFFFu);
        fb_fill(wn->x, wn->y, wn->w, wn->h, 0x00000000u);
        uint id = slot;
        writen(cfd, &id, 4);
        printf("wm: window %d (%dx%d) mapped\n", slot, wn->w, wn->h);
      } else {
        handle_client(&wins[map[k]]);
      }
    }
  }
}
