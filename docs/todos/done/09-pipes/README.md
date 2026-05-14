# 09: pipes (proc-to-proc IPC)

**Done.** Per-proc fd table (`Vec<Option<Arc<File>>>`), `sys_pipe`/`sys_close`,
async pipe read/write with paired reader/writer wakers.

## Notes
- First time the async waker pattern was proc-to-proc instead of
  device-to-proc. Same primitive (`WakerCell`), different resource.

## Files
- `crates/kernel/src/file.rs` (initial)
- `crates/kernel/src/syscall.rs::{sys_pipe, pipe_read, pipe_write}`
- `crates/kernel/user/pipetest.S`
