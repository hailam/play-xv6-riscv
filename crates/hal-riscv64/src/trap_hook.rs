//! Cross-crate hook for trap events. The kernel binary provides
//! `kernel_on_timer`; we re-export it under a local name so the trap
//! handler can call it without naming the kernel crate.

extern "C" {
    fn kernel_on_timer();
}

#[inline]
pub fn on_timer() {
    unsafe { kernel_on_timer() }
}
