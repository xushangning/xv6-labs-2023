//! File-system system calls.
//! Mostly argument checking, since we don't trust
//! user code, and calls into file.c and fs.c.

use core::{ffi::c_int, mem::MaybeUninit, ptr};

use crate::{
    file::File,
    sys::{NOFILE, argaddr, argint, myproc},
};

/// Fetch the nth word-sized system call argument as a file descriptor
/// and return both the descriptor and the corresponding struct file.
fn argfd(n: c_int, pfd: Option<&mut c_int>, pf: Option<&mut *mut File>) -> c_int {
    let fd = unsafe {
        let mut fd = MaybeUninit::<i32>::uninit();
        argint(n, fd.as_mut_ptr());
        fd.assume_init()
    };
    if fd < 0 || fd >= NOFILE.try_into().unwrap() {
        return -1;
    }
    let f = unsafe { (*myproc()).ofile[usize::try_from(fd).unwrap()] };
    if f.is_null() {
        return -1;
    }
    if let Some(pfd) = pfd {
        *pfd = fd;
    }
    if let Some(pf) = pf {
        *pf = f;
    }
    0
}

pub(super) unsafe extern "C" fn read() -> u64 {
    let mut p = MaybeUninit::uninit();
    let mut n = MaybeUninit::uninit();

    unsafe {
        argaddr(1, p.as_mut_ptr());
        argint(2, n.as_mut_ptr());
    }
    let mut f = ptr::null_mut();
    if argfd(0, None, Some(&mut f)) < 0 {
        return (-1i64).cast_unsigned();
    }
    unsafe {
        (*f).read(p.assume_init(), n.assume_init())
            .cast_unsigned()
            .into()
    }
}
