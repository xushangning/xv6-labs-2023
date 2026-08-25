use crate::{proc::Condvar, sys::myproc, trap::TICKS};

pub(super) unsafe extern "C" fn exit() -> u64 {
    crate::proc::exit(unsafe { super::argint(0) })
}

pub(super) unsafe extern "C" fn getpid() -> u64 {
    unsafe { (*myproc()).status.lock().pid.cast_unsigned().into() }
}

pub(super) unsafe extern "C" fn fork() -> u64 {
    crate::proc::fork().cast_unsigned().into()
}

pub(super) unsafe extern "C" fn wait() -> u64 {
    crate::proc::wait(unsafe { super::argaddr(0) })
        .cast_unsigned()
        .into()
}

pub(super) unsafe extern "C" fn sbrk() -> u64 {
    let addr = unsafe { (*myproc()).sz };
    if crate::proc::growproc(unsafe { super::argint(0).try_into().unwrap() }).is_err() {
        return (-1i64).cast_unsigned();
    }
    addr
}

// The function is exported as sys_sleep because the grading script we use as
// tests set a breakpoint at the name sys_sleep during testing.
#[unsafe(export_name = "sys_sleep")]
pub(super) unsafe extern "C" fn sleep() -> u64 {
    use crate::sys::killed;

    let mut n = unsafe { super::argint(0) };
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
    crate::proc::kill(unsafe { super::argint(0) })
        .cast_unsigned()
        .into()
}

pub(super) unsafe extern "C" fn uptime() -> u64 {
    TICKS.lock().0.into()
}
