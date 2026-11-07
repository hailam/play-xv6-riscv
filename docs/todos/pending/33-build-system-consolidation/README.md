# 33: Audit and consolidate user-space build orchestration (`build.rs` vs `xmake.lua` vs `Makefile`)

**Status:** Pending — needs discovery, research & verification
**Scope:** `crates/kernel/build.rs`, `crates/kernel/src/embed.rs`, `xmake.lua`, `Makefile`

## Problem statement

Build logic is split across three tools and contains vestigial plumbing from earlier milestones:
- `crates/kernel/src/embed.rs` only embeds `INITCODE_BIN_PATH` into the kernel image, yet `crates/kernel/build.rs` (~900 LoC) still compiles and links all ~50 user programs during `cargo build -p kernel` and emits unused `cargo:rustc-env=*_BIN_PATH` variables for each one (including fallback dummy paths on unsupported arches at lines 248–252).
- `crates/kernel/build.rs` shells out to `git clone`, `xmake`, `configure.sh`, and `make` inside Cargo's build script to fetch and build `picolibc`, `bc`/`dc`, and `lua`.
- `xmake.lua`'s header comment notes that it was intended as phase 1 of replacing the split build pipeline, but stopped at `picolibc`.

## Discovery & research checklist

1. **Map the current build dependency graph:**
   - Confirm that `crates/kernel/` only requires `INITCODE_BIN_PATH` (and the linker script `kernel.ld` / `kernel-aarch64.ld`) to compile and link the kernel binary itself.
   - Trace how `Makefile` (`fs.img` and `fs-aarch64.img` targets) consumes `target/user/*.elf` and `target/user-aarch64/*.elf` via `mkfs`.
2. **Evaluate consolidation options:**
   - **Option A (Keep Cargo-driven build, slim `build.rs`):** Remove the 50 dead `*_BIN_PATH` env vars and dummy fallback branch in `crates/kernel/build.rs`, factor compiler/linker invocation tables cleanly, and ensure `cargo:rerun-if-changed` tracks all headers (`user.h`, `third_party/` glue).
   - **Option B (Move user-space C builds & third-party ports to `xmake.lua` or `Makefile`):** Have `crates/kernel/build.rs` build only `initcode` (plus linker script flags), while `xmake` / `Makefile` builds `ulib`, `picolibc`, `bc`, `lua`, and user ELFs in parallel before invoking `mkfs`.
3. **Check header dependency tracking:**
   - Currently `compile_to_obj` in `crates/kernel/build.rs` emits `cargo:rerun-if-changed=user/<name>.c`, but does not emit `cargo:rerun-if-changed` for `user/user.h` or `user/gui.h`. Verify whether editing `user.h` fails to trigger a rebuild of user binaries.

## Verification

- Test clean builds (`make clean && make fs.img && make fs-aarch64.img`) and incremental rebuilds (touching `crates/kernel/user/user.h` and a single `.c` file).
- Confirm both `fs.img` and `fs-aarch64.img` contain the full set of user binaries and pass the test suite.
