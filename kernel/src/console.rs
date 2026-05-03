//! Console input and output, to the uart.
//! Reads are line at a time.
//! Implements special input characters:
//!   newline -- end of line
//!   control-h -- backspace
//!   control-u -- kill line
//!   control-d -- end of file
//!   control-p -- print process list

use core::{
    ffi::{c_char, c_int, c_uint},
    mem::MaybeUninit,
};

use crate::sys::release;

/// Control-x
const fn ctrl(x: u8) -> u8 {
    x - b'@'
}

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
}

unsafe extern "C" fn write(user_src: c_int, src: u64, n: c_int) -> c_int {
    for i in 0..n {
        let mut c: MaybeUninit<c_char> = MaybeUninit::uninit();
        if unsafe {
            crate::sys::either_copyin(
                c.as_mut_ptr().cast(),
                user_src,
                src + u64::try_from(i).unwrap(),
                1,
            )
        } == -1
        {
            return i;
        }
        unsafe { crate::sys::uartputc(c.assume_init().into()) };
    }

    n
}

/// user read()s from the console go here.
/// copy (up to) a whole input line to dst.
/// user_dist indicates whether dst is a user
/// or kernel address.
unsafe extern "C" fn read(user_dst: c_int, mut dst: u64, mut n: c_int) -> c_int {
    let target = n;
    unsafe {
        crate::sys::acquire(&raw mut cons.lock);
    }
    while n > 0 {
        unsafe {
            // wait until interrupt handler has put some
            // input into cons.buffer.
            while cons.r == cons.w {
                if crate::sys::killed(crate::sys::myproc()) != 0 {
                    release(&raw mut cons.lock);
                    return -1;
                }
                crate::sys::sleep((&raw mut cons.r).cast(), &raw mut cons.lock);
            }

            let c = cons.buf[cons.r as usize % (*&raw const cons).buf.len()];
            cons.r += 1;

            // end-of-file
            if c == ctrl(b'D') {
                if n < target {
                    // Save ^D for next time, to make sure
                    // caller gets a 0-byte result.
                    cons.r -= 1;
                }
                break;
            }

            // copy the input byte to the user-space buffer.
            let mut cbuf = c;
            if crate::sys::either_copyout(user_dst, dst, (&raw mut cbuf).cast(), 1) == -1 {
                break;
            }

            dst += 1;
            n -= 1;

            if c == b'\n' {
                // a whole line has arrived, return to
                // the user-level read().
                break;
            }
        }
    }

    unsafe {
        release(&raw mut cons.lock);
    }

    target - n
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
                read: Some(read),
                write: Some(write),
            });
    }
}
