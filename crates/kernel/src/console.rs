//! Global locked console. `println!` / `print!` route here.

use core::fmt::{self, Write};

use crate::arch::{Arch, Hal};
use crate::sync::SpinLock;

struct LockedWriter;
impl Write for LockedWriter {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for b in s.bytes() {
            Arch::console_putc(b);
        }
        Ok(())
    }
}

pub static CONSOLE: SpinLock<()> = SpinLock::new(());

pub fn _print(args: fmt::Arguments) {
    let _guard = CONSOLE.lock();
    let _ = LockedWriter.write_fmt(args);
}

/// Bypasses the lock — for panic paths where we'd otherwise deadlock.
pub fn _print_unlocked(args: fmt::Arguments) {
    let _ = LockedWriter.write_fmt(args);
}

#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => ($crate::console::_print(core::format_args!($($arg)*)));
}

#[macro_export]
macro_rules! println {
    () => ($crate::print!("\n"));
    ($($arg:tt)*) => ($crate::console::_print(core::format_args!("{}\n", core::format_args!($($arg)*))));
}
