# 11: argv support

**Done.** `sys_exec` reads the argv array from old user mem, lays out
`[argc | argv[]=NULL | strings]` at the top of the new stack page, sets
`tf.a1 = argv_va`. Returns `argc` so `proc_main`'s `tf.a0 = ret`
puts argc in a0.

## Notes
- Subtle interaction: `proc_main` overwrites `tf.a0` after every syscall.
  Don't set it inside `sys_exec`; return the desired value.

## Files
- `crates/kernel/src/syscall.rs::{sys_exec, read_user_argv, place_argv_on_user_page}`
- `crates/kernel/user/echo.c` (real echo printing argv[1..])
- `crates/kernel/user/sh.c` (tokenize)
