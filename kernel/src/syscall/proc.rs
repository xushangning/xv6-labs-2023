use core::mem::MaybeUninit;

use crate::sys::myproc;

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
    use crate::sys::{acquire, killed, release, ticks, tickslock};

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
    unsafe {
        acquire(&raw mut tickslock);
        let ticks0 = ticks;
        while ticks - ticks0 < n.try_into().unwrap() {
            if killed(myproc()) != 0 {
                release(&raw mut tickslock);
                return (-1i64).cast_unsigned();
            }
            crate::sys::sleep((&raw mut ticks).cast(), &raw mut tickslock);
        }
        release(&raw mut tickslock);
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
    use crate::sys::{acquire, release, ticks, tickslock};

    unsafe {
        acquire(&raw mut tickslock);
        let xticks = ticks;
        release(&raw mut tickslock);
        xticks.into()
    }
}
