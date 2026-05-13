//! Cross-crate hooks for trap events. The kernel binary provides these
//! symbols; we re-export under local fns the trap handler can call.

extern "C" {
    fn kernel_on_timer();
    fn kernel_on_external(src: u32);
}

#[inline]
pub fn on_timer() {
    unsafe { kernel_on_timer() }
}

#[inline]
pub fn on_external(src: u32) {
    unsafe { kernel_on_external(src) }
}
