# revisit: futures-task dependency

**Why deferred:** Original plan said "no external crates in phase 1."
Hand-rolled the executor + waker plumbing (~150 LoC of subtle code).
It works and is reliable.

**What would trigger revisit:**
- We hit a bug in waker semantics that turns out to be well-handled by
  the standard crate
- The hand-rolled `RawWakerVTable` becomes painful to extend (e.g., for
  per-CPU executors needing more complex waker bookkeeping)
- We want `futures::stream::Stream` or `select!` for more sophisticated
  scheduling

**What we'd swap in:** `futures-task` (no_std, no_alloc-feature
optional). Doesn't grow the unsafe budget — it's safe Rust.

**Hand-rolled bits to keep:** Our `WakerCell` is fine as-is.
