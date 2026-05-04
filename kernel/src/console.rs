//! Console input and output, to the uart.
//! Reads are line at a time.
//! Implements special input characters:
//!   newline -- end of line
//!   control-h -- backspace
//!   control-u -- kill line
//!   control-d -- end of file
//!   control-p -- print process list

use core::{
    ffi::{c_char, c_int},
    mem::MaybeUninit,
};

use crate::sys::{acquire, release, spinlock};

const BACKSPACE: c_int = 0x100;

/// Control-x
const fn ctrl(x: u8) -> u8 {
    x - b'@'
}

struct Cons {
    lock: spinlock,

    /// input
    buf: [c_char; 128],
    /// Read index
    r: usize,
    /// Write index
    w: usize,
    /// Edit index
    e: usize,
}

static mut CONS: Cons = Cons {
    lock: spinlock {
        name: c"cons".as_ptr().cast_mut(),
        locked: 0,
        cpu: core::ptr::null_mut(),
    },
    buf: [0; _],
    r: 0,
    w: 0,
    e: 0,
};

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
        acquire(&raw mut CONS.lock);
    }
    while n > 0 {
        unsafe {
            // wait until interrupt handler has put some
            // input into cons.buffer.
            while CONS.r == CONS.w {
                if crate::sys::killed(crate::sys::myproc()) != 0 {
                    release(&raw mut CONS.lock);
                    return -1;
                }
                crate::sys::sleep((&raw mut CONS.r).cast(), &raw mut CONS.lock);
            }

            let c = CONS.buf[CONS.r % (*&raw const CONS).buf.len()];
            CONS.r += 1;

            // end-of-file
            if c == ctrl(b'D') {
                if n < target {
                    // Save ^D for next time, to make sure
                    // caller gets a 0-byte result.
                    CONS.r -= 1;
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
        release(&raw mut CONS.lock);
    }

    target - n
}

/// the console input interrupt handler.
/// uartintr() calls this for input character.
/// do erase/kill processing, append to cons.buf,
/// wake up consoleread() if a whole line has arrived.
pub(super) fn intr(mut c: u8) {
    use crate::sys::consputc;

    unsafe {
        acquire(&raw mut CONS.lock);

        if c == ctrl(b'P') {
            // Print process list.
            crate::sys::procdump();
        } else if c == ctrl(b'U') {
            // Kill line.
            while CONS.e != CONS.w && CONS.buf[(CONS.e - 1) % (*&raw const CONS).buf.len()] != b'\n'
            {
                CONS.e -= 1;
                consputc(BACKSPACE);
            }
        } else if c == ctrl(b'H') /* Backspace */ || c == b'\x7f'
        /* Delete key */
        {
            if CONS.e != CONS.w {
                CONS.e -= 1;
                consputc(BACKSPACE);
            }
        } else if c != 0 && CONS.e.wrapping_sub(CONS.r) < (*&raw const CONS).buf.len() {
            if c == b'\r' {
                c = b'\n';
            }

            // echo back to the user.
            consputc(c.into());

            // store for consumption by consoleread().
            CONS.buf[CONS.e % (*&raw const CONS).buf.len()] = c;
            CONS.e += 1;

            if c == b'\n'
                || c == ctrl(b'D')
                || CONS.e.wrapping_sub(CONS.r) == (*&raw const CONS).buf.len()
            {
                // wake up consoleread() if a whole line (or end-of-file)
                // has arrived.
                CONS.w = CONS.e;
                crate::sys::wakeup((&raw mut CONS.r).cast());
            }
        }

        release(&raw mut CONS.lock);
    }
}

pub(crate) fn init() {
    use crate::sys::devsw;

    unsafe {
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
