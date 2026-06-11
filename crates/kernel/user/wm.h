//
// Window protocol (todo 12 M4) — clients talk to the display server
// over an AF_UNIX socket. All message words are native-endian u32.
//
//   client → server:
//     [WM_CREATE, w, h, want_keys]            → server replies [win_id]
//     [WM_BLIT, x, y, w, h] + w*h*4 px bytes  (XRGB8888, window coords)
//   server → client (only when want_keys=1):
//     raw 8-byte input events {u16 type, u16 code, u32 value}
//
// The server composites windows onto /dev/fb0 and routes keyboard
// input to the most recently created want_keys window.
//
#ifndef WM_H
#define WM_H

#define WM_SOCK   "/wm.sock"
#define WM_CREATE 1
#define WM_BLIT   2

// Read exactly n bytes (sockets may return short reads).
static int
readn(int fd, void *buf, int n)
{
  char *p = (char*)buf;
  int got = 0;
  while(got < n){
    int r = read(fd, p + got, n - got);
    if(r <= 0)
      return r;
    got += r;
  }
  return got;
}

// Write exactly n bytes.
static int
writen(int fd, const void *buf, int n)
{
  const char *p = (const char*)buf;
  int put = 0;
  while(put < n){
    int r = write(fd, p + put, n - put);
    if(r <= 0)
      return r;
    put += r;
  }
  return put;
}

#endif
