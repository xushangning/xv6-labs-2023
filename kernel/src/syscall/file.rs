//! File-system system calls.
//! Mostly argument checking, since we don't trust
//! user code, and calls into file.c and fs.c.

use alloc::boxed::Box;
use core::{
    ffi::{c_char, c_int},
    mem::{DropGuard, MaybeUninit},
    ptr::NonNull,
    slice,
};

use super::{argint, argstr};
use crate::{
    file::File,
    kalloc::Page,
    rc::RcInner,
    sys::{NOFILE, argaddr, myproc},
};

unsafe extern "C" {
    fn create(
        path: *mut c_char,
        type_: core::ffi::c_short,
        major: core::ffi::c_short,
        minor: core::ffi::c_short,
    ) -> *mut crate::sys::inode;
}

/// Fetch the nth word-sized system call argument as a file descriptor
/// and return both the descriptor and the corresponding struct file.
fn argfd(n: c_int) -> Result<(c_int, NonNull<RcInner<File>>), ()> {
    let fd = unsafe { argint(n) };
    if fd < 0 || fd >= NOFILE.try_into().unwrap() {
        return Err(());
    }
    let f = unsafe { (*myproc()).ofile[usize::try_from(fd).unwrap()] }.ok_or(())?;
    Ok((fd, f))
}

/// Allocate a file descriptor for the given file.
/// Takes over file reference from caller on success.
fn fdalloc(f: NonNull<RcInner<File>>) -> c_int {
    let p = unsafe { myproc().as_mut().unwrap() };
    for (fd, of) in p.ofile.iter_mut().enumerate() {
        if of.is_none() {
            *of = Some(f);
            return fd.try_into().unwrap();
        }
    }
    -1
}

pub(super) unsafe extern "C" fn dup() -> u64 {
    let Ok((_, f)) = argfd(0) else {
        return (-1i64).cast_unsigned();
    };
    let fd = fdalloc(f);
    if fd < 0 {
        return (-1i64).cast_unsigned();
    }
    crate::file::dup(f);
    fd.cast_unsigned().into()
}

pub(super) unsafe extern "C" fn read() -> u64 {
    let mut p = MaybeUninit::uninit();

    unsafe {
        argaddr(1, p.as_mut_ptr());
    }
    let n = unsafe { argint(2) };
    let Ok((_, mut f)) = argfd(0) else {
        return (-1i64).cast_unsigned();
    };
    unsafe { f.as_mut().read(p.assume_init(), n).cast_unsigned().into() }
}

pub(super) unsafe extern "C" fn write() -> u64 {
    let mut p = MaybeUninit::uninit();

    unsafe {
        argaddr(1, p.as_mut_ptr());
    }
    let n = unsafe { argint(2) };
    let Ok((_, mut f)) = argfd(0) else {
        return (-1i64).cast_unsigned();
    };
    unsafe { f.as_mut().write(p.assume_init(), n).cast_unsigned().into() }
}

pub(super) unsafe extern "C" fn open() -> u64 {
    use crate::{
        fcntl::OMode,
        file::FileKind,
        log::OpGuard,
        param::MAXPATH,
        stat::InodeType,
        sys::{NDEV, ilock, itrunc, iunlock, iunlockput, namei},
    };

    let omode = OMode::from_bits_retain(unsafe { argint(1) });
    let mut path = MaybeUninit::<[c_char; MAXPATH]>::uninit();
    if unsafe { argstr(0, path.as_mut()) } < 0 {
        return (-1i64).cast_unsigned();
    }

    let _op_guard = OpGuard::new();

    let ip = if omode.intersects(OMode::CREATE) {
        let Some(ip) =
            (unsafe { create(path.as_mut_ptr().cast(), InodeType::File as i16, 0, 0).as_mut() })
        else {
            return (-1i64).cast_unsigned();
        };
        ip
    } else {
        let Some(ip) = (unsafe { namei(path.as_mut_ptr().cast()).as_mut() }) else {
            return (-1i64).cast_unsigned();
        };
        unsafe { ilock(ip) };
        if matches!(ip.type_, InodeType::Dir) && omode != OMode::RDONLY {
            unsafe { iunlockput(ip) };
            return (-1i64).cast_unsigned();
        }
        ip
    };

    if matches!(ip.type_, InodeType::Device) && (ip.major < 0 || ip.major as u32 >= NDEV) {
        unsafe { iunlockput(ip) };
        return (-1i64).cast_unsigned();
    }

    let Some(mut f) = crate::file::alloc() else {
        unsafe { iunlockput(ip) };
        return (-1i64).cast_unsigned();
    };

    let fd = fdalloc(f);
    if fd < 0 {
        crate::file::close(f);
        unsafe { iunlockput(ip) };
        return (-1i64).cast_unsigned();
    }

    let f = unsafe { f.as_mut() };
    match ip.type_ {
        InodeType::Device => {
            f.kind = FileKind::Device {
                ip: NonNull::from_mut(ip),
                major: ip.major,
            };
        }
        _ => {
            f.kind = FileKind::Inode {
                ip: NonNull::from_mut(ip),
                off: 0,
            };
        }
    }
    f.readable = !omode.intersects(OMode::WRONLY);
    f.writable = omode.intersects(OMode::WRONLY | OMode::RDWR);

    if omode.intersects(OMode::TRUNC) && matches!(ip.type_, InodeType::File) {
        unsafe { itrunc(ip) };
    }

    unsafe { iunlock(ip) };

    fd.cast_unsigned().into()
}

pub(super) unsafe extern "C" fn close() -> u64 {
    let Ok((fd, f)) = argfd(0) else {
        return (-1i64).cast_unsigned();
    };
    unsafe { (*myproc()).ofile[usize::try_from(fd).unwrap()] = None };
    crate::file::close(f);
    0
}

pub(super) unsafe extern "C" fn fstat() -> u64 {
    let st = unsafe { super::argaddr(1) }; // user pointer to struct stat
    let Ok((_, mut f)) = argfd(0) else {
        return (-1i64).cast_unsigned();
    };
    unsafe {
        match f.as_mut().stat(st) {
            Ok(_) => 0,
            Err(_) => (-1i64).cast_unsigned(),
        }
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
    if unsafe { argstr(0, path.as_mut()) } < 0 {
        return (-1i64).cast_unsigned();
    }
    let mut argv = heapless::Vec::<Box<MaybeUninit<Page>>, { crate::param::MAXARG }>::new();
    for _ in 0..argv.capacity() {
        let Ok(uarg) = super::fetchaddr(uargv.addr()) else {
            return (-1i64).cast_unsigned();
        };
        if uarg == 0 {
            break;
        }
        let Ok(mut arg) = Box::<Page>::try_new_uninit() else {
            return (-1i64).cast_unsigned();
        };
        if super::fetchstr(uarg, arg.as_bytes_mut()).is_err() {
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
    let rf = DropGuard::new(rf, crate::file::close);
    let wf = DropGuard::new(wf, crate::file::close);
    let p = unsafe { myproc().as_mut().unwrap() };
    let mut fd = [-1, -1];
    fd[0] = fdalloc(*rf);
    if fd[0] < 0 {
        return (-1i64).cast_unsigned();
    }
    fd[1] = fdalloc(*wf);
    if fd[1] < 0 {
        p.ofile[fd[0] as usize] = None;
        return (-1i64).cast_unsigned();
    }
    let pt = p.pagetable.as_mut().unwrap().as_mut();
    if unsafe { crate::vm::copyout(pt, fdarray, bytemuck::bytes_of(&fd)).is_err() } {
        p.ofile[fd[0] as usize] = None;
        p.ofile[fd[1] as usize] = None;
        return (-1i64).cast_unsigned();
    }
    DropGuard::dismiss(rf);
    DropGuard::dismiss(wf);
    0
}
