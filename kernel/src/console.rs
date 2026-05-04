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

use crate::spinlock::Mutex;

const BACKSPACE: c_int = 0x100;

/// Control-x
const fn ctrl(x: u8) -> u8 {
    x - b'@'
}

/// send one character to the uart.
/// called by printf(), and to echo input characters,
/// but not from write().
pub(super) fn putc(c: c_int) {
    use crate::sys::uartputc_sync;

    unsafe {
        match c {
            BACKSPACE => {
                // if the user typed backspace, overwrite with a space.
                uartputc_sync(b'\x08'.into());
                uartputc_sync(b' '.into());
                uartputc_sync(b'\x08'.into());
            }
            _ => {
                uartputc_sync(c);
            }
        }
    }
}

struct Cons {
    /// input
    buf: [c_char; 128],
    /// Read index
    r: usize,
    /// Write index
    w: usize,
    /// Edit index
    e: usize,
}

static CONS: Mutex<Cons> = Mutex::new(
    c"cons",
    Cons {
        buf: [0; _],
        r: 0,
        w: 0,
        e: 0,
    },
);

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
    let mut cons = CONS.lock();
    while n > 0 {
        // wait until interrupt handler has put some
        // input into cons.buffer.
        while cons.r == cons.w {
            unsafe {
                if crate::sys::killed(crate::sys::myproc()) != 0 {
                    return -1;
                }
                crate::sys::sleep((&raw mut cons.r).cast(), cons.lock.inner.get());
            }
        }

        let c = cons.buf[cons.r % cons.buf.len()];
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
        if unsafe { crate::sys::either_copyout(user_dst, dst, (&raw mut cbuf).cast(), 1) } == -1 {
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

    target - n
}

/// the console input interrupt handler.
/// uartintr() calls this for input character.
/// do erase/kill processing, append to cons.buf,
/// wake up consoleread() if a whole line has arrived.
pub(super) fn intr(mut c: u8) {
    let mut cons = CONS.lock();

    if c == ctrl(b'P') {
        // Print process list.
        unsafe {
            crate::sys::procdump();
        }
    } else if c == ctrl(b'U') {
        // Kill line.
        while cons.e != cons.w && cons.buf[(cons.e - 1) % cons.buf.len()] != b'\n' {
            cons.e -= 1;
            putc(BACKSPACE);
        }
    } else if c == ctrl(b'H') /* Backspace */ || c == b'\x7f'
    /* Delete key */
    {
        if cons.e != cons.w {
            cons.e -= 1;
            putc(BACKSPACE);
        }
    } else if c != 0 && cons.e.wrapping_sub(cons.r) < cons.buf.len() {
        if c == b'\r' {
            c = b'\n';
        }

        // echo back to the user.
        putc(c.into());

        // store for consumption by consoleread().
        let e = cons.e % cons.buf.len();
        cons.buf[e] = c;
        cons.e += 1;

        if c == b'\n' || c == ctrl(b'D') || cons.e.wrapping_sub(cons.r) == cons.buf.len() {
            // wake up consoleread() if a whole line (or end-of-file)
            // has arrived.
            cons.w = cons.e;
            unsafe {
                crate::sys::wakeup((&raw mut cons.r).cast());
            }
        }
    }
}

pub(super) fn init() {
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
