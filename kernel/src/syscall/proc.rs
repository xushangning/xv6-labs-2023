use core::mem::MaybeUninit;

use crate::{proc::Condvar, sys::myproc, trap::TICKS};

pub(super) unsafe extern "C" fn exit() -> u64 {
    let mut n = MaybeUninit::uninit();
    unsafe { crate::sys::argint(0, n.as_mut_ptr()) };
    crate::proc::exit(unsafe { n.assume_init() })
}

pub(super) unsafe extern "C" fn getpid() -> u64 {
    unsafe { (*myproc()).pid.cast_unsigned().into() }
}

pub(super) unsafe extern "C" fn fork() -> u64 {
    crate::proc::fork().cast_unsigned().into()
}

pub(super) unsafe extern "C" fn wait() -> u64 {
    let mut p: u64 = 0;
    unsafe { crate::sys::argaddr(0, &mut p) };
    crate::proc::wait(p).cast_unsigned().into()
}

pub(super) unsafe extern "C" fn sbrk() -> u64 {
    let mut n = MaybeUninit::uninit();

    unsafe { crate::sys::argint(0, n.as_mut_ptr()) };
    let addr = unsafe { (*myproc()).sz };
    if crate::proc::growproc(unsafe { n.assume_init() }) < 0 {
        return (-1i64).cast_unsigned();
    }
    addr
}

// The function is exported as sys_sleep because the grading script we use as
// tests set a breakpoint at the name sys_sleep during testing.
#[unsafe(export_name = "sys_sleep")]
pub(super) unsafe extern "C" fn sleep() -> u64 {
    use crate::sys::killed;

    let mut n = {
        let mut n = MaybeUninit::uninit();
        unsafe {
            crate::sys::argint(0, n.as_mut_ptr());
            n.assume_init()
        }
    };
    if n < 0 {
        n = 0;
    }
    let mut ticks = TICKS.lock();
    let ticks0 = ticks.0;
    while ticks.0 - ticks0 < n.try_into().unwrap() {
        unsafe {
            if killed(myproc()) != 0 {
                return (-1i64).cast_unsigned();
            }
            ticks = Condvar::wait(&*ticks, ticks);
        }
    }
    0
}

pub(super) unsafe extern "C" fn kill() -> u64 {
    let mut pid = MaybeUninit::uninit();
    unsafe { crate::sys::argint(0, pid.as_mut_ptr()) };
    crate::proc::kill(unsafe { pid.assume_init() })
        .cast_unsigned()
        .into()
}

pub(super) unsafe extern "C" fn uptime() -> u64 {
    TICKS.lock().0.into()
}
