//! Console input and output, to the uart.
//! Reads are line at a time.
//! Implements special input characters:
//!   newline -- end of line
//!   control-h -- backspace
//!   control-u -- kill line
//!   control-d -- end of file
//!   control-p -- print process list

use core::ffi::{c_char, c_int, c_uint};

#[repr(C)]
struct Cons {
    lock: crate::sys::spinlock,

    /// input
    buf: [c_char; 128],
    /// Read index
    r: c_uint,
    /// Write index
    w: c_uint,
    /// Edit index
    e: c_uint,
}

unsafe extern "C" {
    static mut cons: Cons;
    fn consoleread(user_dst: c_int, dst: u64, n: c_int) -> c_int;
    fn consolewrite(user_src: c_int, src: u64, n: c_int) -> c_int;
}

pub(crate) fn init() {
    use crate::sys::devsw;

    unsafe {
        crate::sys::initlock(&raw mut cons.lock, c"cons".as_ptr().cast_mut());

        crate::sys::uartinit();

        // connect read and write system calls
        // to consoleread and consolewrite.
        (*&raw mut devsw)
            .as_mut_ptr()
            .add(crate::sys::CONSOLE as usize)
            .write(devsw {
                read: Some(consoleread),
                write: Some(consolewrite),
            });
    }
}
