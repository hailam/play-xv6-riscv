# 10: sys_dup + pipe EOF + shell pipelines

**Done.** Per-fd `Arc<File>` (manual `Clone` bumps pipe counts, `Drop`
decrements). Pipe EOF: reader sees `writers == 0` and returns 0. Shell
parses `|` and forks two children with dup'd pipe ends.

## Notes
- First bug: `Arc<File>` shared across fds → `Drop` only fires when the
  *last* arc dies → writer count never hits 0. Fixed by manual `Clone`
  that gives each fd its own `Arc<File>`.
- Second bug: `_start` wasn't at offset 0 in the linked binary, so
  `sret` to user PC=0 ran `main` directly, skipping `_start`. Added
  `user/user.ld` that puts `.text.entry` first.

## Files
- `crates/kernel/src/file.rs` (final form)
- `crates/kernel/user/user.ld`
- `crates/kernel/user/ulib.S`
- `crates/kernel/user/sh.c`
- `crates/kernel/user/cat.c`
