//! File-system system calls.
//! Mostly argument checking, since we don't trust
//! user code, and calls into file.c and fs.c.

use alloc::boxed::Box;
use core::{
    ffi::{c_char, c_int},
    mem::{DropGuard, MaybeUninit},
    ptr, slice,
};

use crate::{
    file::File,
    kalloc::Page,
    sys::{NOFILE, argaddr, argint, argstr, fetchaddr, fetchstr, fileclose, myproc},
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

/// Allocate a file descriptor for the given file.
/// Takes over file reference from caller on success.
fn fdalloc(f: *mut File) -> c_int {
    let p = unsafe { myproc().as_mut().unwrap() };
    for (fd, of) in p.ofile.iter_mut().enumerate() {
        if of.is_null() {
            *of = f;
            return fd.try_into().unwrap();
        }
    }
    -1
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

pub(super) unsafe extern "C" fn write() -> u64 {
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
        (*f).write(p.assume_init(), n.assume_init())
            .cast_unsigned()
            .into()
    }
}

pub(super) unsafe extern "C" fn exec() -> u64 {
    use crate::param::MAXPATH;

    let mut uargv = {
        let mut uargv = MaybeUninit::<*const u64>::uninit();
        unsafe {
            argaddr(1, uargv.as_mut_ptr().cast());
            uargv.assume_init()
        }
    };

    let mut path = MaybeUninit::<[c_char; MAXPATH]>::uninit();
    if unsafe { argstr(0, path.as_mut_ptr().cast(), MAXPATH.try_into().unwrap()) } < 0 {
        return (-1i64).cast_unsigned();
    }
    let mut argv = heapless::Vec::<Box<MaybeUninit<Page>>, { crate::param::MAXARG }>::new();
    for _ in 0..argv.capacity() {
        let uarg = {
            let mut uarg = MaybeUninit::uninit();
            if unsafe { fetchaddr(uargv.addr().try_into().unwrap(), uarg.as_mut_ptr()) } < 0 {
                return (-1i64).cast_unsigned();
            }
            unsafe { uarg.assume_init() }
        };
        if uarg == 0 {
            break;
        }
        let Ok(mut arg) = Box::<Page>::try_new_uninit() else {
            return (-1i64).cast_unsigned();
        };
        if unsafe {
            fetchstr(
                uarg,
                arg.as_mut_ptr().cast(),
                crate::riscv::PGSIZE.try_into().unwrap(),
            )
        } < 0
        {
            return (-1i64).cast_unsigned();
        }
        unsafe {
            argv.push_unchecked(arg);
        };
        uargv = unsafe { uargv.add(1) };
    }

    match crate::exec::exec(path.as_ptr().cast(), unsafe {
        slice::from_raw_parts(argv.as_ptr().cast(), argv.len())
    }) {
        Ok(ret) => ret.try_into().unwrap(),
        Err(_) => (-1i64).cast_unsigned(),
    }
}

pub(super) unsafe extern "C" fn pipe() -> u64 {
    // user pointer to array of two integers
    let fdarray: usize = {
        let mut fdarray = MaybeUninit::uninit();
        unsafe { argaddr(0, fdarray.as_mut_ptr()) };
        unsafe { fdarray.assume_init().try_into().unwrap() }
    };

    let Ok((rf, wf)) = crate::pipe::alloc() else {
        return (-1i64).cast_unsigned();
    };
    let rf = DropGuard::new(rf, |f: *mut File| unsafe {
        if !f.is_null() {
            fileclose(f);
        }
    });
    let wf = DropGuard::new(wf, |f: *mut File| unsafe {
        if !f.is_null() {
            fileclose(f);
        }
    });
    let p = unsafe { myproc().as_mut().unwrap() };
    let mut fd = [-1, -1];
    fd[0] = fdalloc(*rf);
    if fd[0] < 0 {
        return (-1i64).cast_unsigned();
    }
    fd[1] = fdalloc(*wf);
    if fd[1] < 0 {
        p.ofile[fd[0] as usize] = ptr::null_mut();
        return (-1i64).cast_unsigned();
    }
    let pt = p.pagetable.as_mut().unwrap().as_mut();
    if unsafe { crate::vm::copyout(pt, fdarray, bytemuck::bytes_of(&fd)).is_err() } {
        p.ofile[fd[0] as usize] = ptr::null_mut();
        p.ofile[fd[1] as usize] = ptr::null_mut();
        return (-1i64).cast_unsigned();
    }
    DropGuard::dismiss(rf);
    DropGuard::dismiss(wf);
    0
}
