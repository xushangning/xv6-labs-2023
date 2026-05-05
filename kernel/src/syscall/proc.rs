use core::mem::MaybeUninit;

pub(super) unsafe extern "C" fn exit() -> u64 {
    let mut n = MaybeUninit::uninit();
    unsafe { crate::sys::argint(0, n.as_mut_ptr()) };
    crate::proc::exit(unsafe { n.assume_init() })
}

pub(super) unsafe extern "C" fn getpid() -> u64 {
    unsafe { (*crate::sys::myproc()).pid.cast_unsigned().into() }
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
    let addr = unsafe { (*crate::sys::myproc()).sz };
    if crate::proc::growproc(unsafe { n.assume_init() }) < 0 {
        return (-1i64).cast_unsigned();
    }
    addr
}

pub(super) unsafe extern "C" fn kill() -> u64 {
    let mut pid = MaybeUninit::uninit();
    unsafe { crate::sys::argint(0, pid.as_mut_ptr()) };
    crate::proc::kill(unsafe { pid.assume_init() })
        .cast_unsigned()
        .into()
}
