//! Console input: IRQ-side line discipline (xv6's `consoleintr`) plus
//! the cooked ring drained by `sys_read`.
//!
//! Typed characters land in an EDIT buffer (with echo, backspace and
//! ^U kill-line) and are only released to readers on `\n` or `^D` —
//! cooked/canonical mode, which is what `sys_ioctl`'s TCGETS already
//! advertises (`ICANON|ECHO`). `^D` travels in-band as 0x04 so a
//! blocked reader can return 0 (EOF) at the right point in the
//! stream; `console_read` consumes it without handing it to the user.

use alloc::collections::VecDeque;
use alloc::vec::Vec;
use core::task::Waker;

use hal::Hal;

use crate::arch::Arch;
use crate::sync::SpinLock;
use crate::wait::WakerList;

const CAP: usize = 256;
/// In-band EOF mark (^D). Never delivered to user buffers.
pub const EOF_SENTINEL: u8 = 0x04;

struct ConsIn {
    /// Completed lines (and EOF marks), ready for `sys_read`.
    cooked: VecDeque<u8>,
    /// The line currently being edited; not yet visible to readers.
    edit: Vec<u8>,
}

static STATE: SpinLock<ConsIn> = SpinLock::new(ConsIn {
    cooked: VecDeque::new(),
    edit: Vec::new(),
});

/// Multi-waiter: more than one proc can block reading the console
/// (e.g. parent and child both on fd 0) — wake them all and let the
/// losers re-park.
static READER: WakerList = WakerList::new();

fn echo(c: u8) {
    Arch::console_putc(c);
}

fn rubout() {
    // Erase the last echoed character: back, blank, back.
    Arch::console_putc(0x08);
    Arch::console_putc(b' ');
    Arch::console_putc(0x08);
}

/// Feed one byte from the UART RX IRQ handler through the line
/// discipline.
pub fn push(c: u8) {
    let c = if c == b'\r' { b'\n' } else { c };
    let mut wake = false;
    {
        let mut st = STATE.lock();
        match c {
            0x15 /* ^U: kill line */ => {
                while st.edit.pop().is_some() {
                    rubout();
                }
            }
            0x08 | 0x7f /* backspace / DEL */ => {
                if st.edit.pop().is_some() {
                    rubout();
                }
            }
            EOF_SENTINEL /* ^D */ => {
                // Release the partial line, then the EOF mark.
                if st.cooked.len() + st.edit.len() + 1 <= CAP {
                    let edit = core::mem::take(&mut st.edit);
                    st.cooked.extend(edit);
                    st.cooked.push_back(EOF_SENTINEL);
                    wake = true;
                }
            }
            b'\n' => {
                if st.cooked.len() + st.edit.len() + 1 <= CAP {
                    echo(b'\n');
                    let edit = core::mem::take(&mut st.edit);
                    st.cooked.extend(edit);
                    st.cooked.push_back(b'\n');
                    wake = true;
                }
            }
            c if c == b'\t' || (0x20..0x7f).contains(&c) => {
                if st.cooked.len() + st.edit.len() < CAP {
                    st.edit.push(c);
                    echo(c);
                }
            }
            _ => {}
        }
    }
    if wake {
        READER.wake_all();
    }
}

pub fn try_pop() -> Option<u8> {
    STATE.lock().cooked.pop_front()
}

/// Put a byte back at the FRONT of the cooked queue — used by
/// `console_read` to save an EOF mark for the next read when the
/// current one already has data to return.
pub fn unget(c: u8) {
    STATE.lock().cooked.push_front(c);
}

/// Number of unread cooked bytes (for FIONREAD / POLLIN).
pub fn pending() -> usize {
    STATE.lock().cooked.len()
}

pub fn register_waker(w: &Waker) {
    READER.register(w);
}

/// Boot the currently-parked readers (if any). Called by `sys_kill`
/// so a killed proc blocked in console read returns promptly.
pub fn wake() {
    READER.wake_all();
}
