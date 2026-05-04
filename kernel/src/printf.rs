use core::{
    ffi::{CStr, c_int},
    fmt::{self, Write},
};

use crate::sys::spinlock;

#[repr(C)]
struct Pr {
    lock: spinlock,
    locking: c_int,
}

unsafe extern "C" {
    static mut pr: Pr;
}

struct ConsoleSink;

impl Write for ConsoleSink {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for b in s.bytes() {
            crate::console::putc(b.into());
        }
        Ok(())
    }
}

pub(crate) fn _print(args: fmt::Arguments) {
    unsafe {
        let locking = pr.locking;
        if locking != 0 {
            crate::sys::acquire(&raw mut pr.lock);
        }
        let _ = ConsoleSink.write_fmt(args);
        if locking != 0 {
            crate::sys::release(&raw mut pr.lock);
        }
    }
}

#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => { $crate::printf::_print(format_args!($($arg)*)) };
}

#[macro_export]
macro_rules! println {
    ()            => { $crate::print!("\n") };
    ($($arg:tt)*) => { $crate::print!("{}\n", format_args!($($arg)*)) };
}

pub(crate) fn panic(s: &CStr) {
    unsafe {
        crate::sys::panic(s.as_ptr().cast_mut());
    }
}
