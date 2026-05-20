//! File-system system calls.
//! Mostly argument checking, since we don't trust
//! user code, and calls into file.c and fs.c.

use alloc::boxed::Box;
use core::{
    ffi::{c_char, c_int, c_short, c_uint},
    mem::MaybeUninit,
    ptr::NonNull,
    slice,
};

use super::{argint, argstr};
use crate::{
    file::File,
    fs::{DIRSIZ, Inode},
    kalloc::Page,
    rc::{Rc, UniqueRc},
    stat::InodeType,
    sys::{NOFILE, argaddr, myproc},
};

/// Fetch the nth word-sized system call argument as a file descriptor
/// and return both the descriptor and the corresponding struct file.
fn argfd<'a>(n: c_int) -> Result<(c_int, &'a Rc<File>), ()> {
    let fd = unsafe { argint(n) };
    if fd < 0 || fd >= NOFILE.try_into().unwrap() {
        return Err(());
    }
    let f = unsafe {
        (*myproc()).ofile[usize::try_from(fd).unwrap()]
            .as_ref()
            .ok_or(())?
    };
    Ok((fd, f))
}

/// Allocate a file descriptor for the given file.
/// Takes over file reference from caller on success.
fn fdalloc(f: Rc<File>) -> Result<c_int, Rc<File>> {
    let p = unsafe { myproc().as_mut().unwrap() };
    for (fd, of) in p.ofile.iter_mut().enumerate() {
        if of.is_none() {
            *of = Some(f);
            return Ok(fd.try_into().unwrap());
        }
    }
    Err(f)
}

pub(super) unsafe extern "C" fn dup() -> u64 {
    let Ok((_, f)) = argfd(0) else {
        return (-1i64).cast_unsigned();
    };
    match fdalloc(crate::file::dup(f)) {
        Ok(fd) => fd.cast_unsigned().into(),
        Err(f) => {
            crate::file::close(f);
            (-1i64).cast_unsigned()
        }
    }
}

pub(super) unsafe extern "C" fn read() -> u64 {
    let mut p = MaybeUninit::uninit();

    unsafe {
        argaddr(1, p.as_mut_ptr());
    }
    let n = unsafe { argint(2) };
    let Ok((_, f)) = argfd(0) else {
        return (-1i64).cast_unsigned();
    };
    unsafe { f.read(p.assume_init(), n).cast_unsigned().into() }
}

pub(super) unsafe extern "C" fn write() -> u64 {
    let mut p = MaybeUninit::uninit();

    unsafe {
        argaddr(1, p.as_mut_ptr());
    }
    let n = unsafe { argint(2) };
    let Ok((_, f)) = argfd(0) else {
        return (-1i64).cast_unsigned();
    };
    unsafe { f.write(p.assume_init(), n).cast_unsigned().into() }
}

pub(super) unsafe extern "C" fn open() -> u64 {
    use crate::{
        fcntl::OMode,
        file::FileKind,
        log::OpGuard,
        param::MAXPATH,
        sys::NDEV,
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
        let Some(ip) = (unsafe { crate::fs::Inode::namei(path.as_mut_ptr().cast()).as_mut() }) else {
            return (-1i64).cast_unsigned();
        };
        unsafe { ip.lock() };
        if matches!(ip.type_, InodeType::Dir) && omode != OMode::RDONLY {
            unsafe { ip.unlock_put() };
            return (-1i64).cast_unsigned();
        }
        ip
    };

    if matches!(ip.type_, InodeType::Device) && (ip.major < 0 || ip.major as u32 >= NDEV) {
        unsafe { ip.unlock_put() };
        return (-1i64).cast_unsigned();
    }

    if omode.intersects(OMode::TRUNC) && matches!(ip.type_, InodeType::File) {
        unsafe { ip.trunc() };
    }

    unsafe { ip.unlock() };

    let Some(mut f) = crate::file::alloc() else {
        unsafe { ip.put() };
        return (-1i64).cast_unsigned();
    };

    *f = File {
        kind: match ip.type_ {
            InodeType::Device => FileKind::Device {
                ip: NonNull::from_mut(ip),
                major: ip.major,
            },
            _ => FileKind::Inode {
                ip: NonNull::from_mut(ip),
                off: 0.into(),
            },
        },
        readable: !omode.intersects(OMode::WRONLY),
        writable: omode.intersects(OMode::WRONLY | OMode::RDWR),
    };

    match fdalloc(UniqueRc::into_rc(f)) {
        Ok(fd) => fd.cast_unsigned().into(),
        Err(f) => {
            crate::file::close(f);
            (-1i64).cast_unsigned()
        }
    }
}

pub(super) unsafe extern "C" fn close() -> u64 {
    let Ok((fd, _)) = argfd(0) else {
        return (-1i64).cast_unsigned();
    };
    crate::file::close(unsafe {
        (*myproc()).ofile[usize::try_from(fd).unwrap()]
            .take()
            .unwrap()
    });
    0
}

pub(super) unsafe extern "C" fn fstat() -> u64 {
    let st = unsafe { super::argaddr(1) }; // user pointer to struct stat
    let Ok((_, f)) = argfd(0) else {
        return (-1i64).cast_unsigned();
    };
    match f.stat(st) {
        Ok(_) => 0,
        Err(_) => (-1i64).cast_unsigned(),
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
    let p = unsafe { myproc().as_mut().unwrap() };
    let mut fd = [-1, -1];
    match fdalloc(rf) {
        Ok(f) => fd[0] = f,
        Err(f) => {
            crate::file::close(f);
            return (-1i64).cast_unsigned();
        }
    }
    match fdalloc(wf) {
        Ok(f) => fd[1] = f,
        Err(f) => {
            crate::file::close(f);
            return (-1i64).cast_unsigned();
        }
    }
    let pt = p.pagetable.as_mut().unwrap().as_mut();
    if unsafe { crate::vm::copyout(pt, fdarray, bytemuck::bytes_of(&fd)).is_err() } {
        crate::file::close(p.ofile[fd[0] as usize].take().unwrap());
        crate::file::close(p.ofile[fd[1] as usize].take().unwrap());
        return (-1i64).cast_unsigned();
    }
    0
}

// Is the directory dp empty except for "." and ".." ?
unsafe fn isdirempty(dp: *mut Inode) -> bool {
    unsafe {
        let de_size = core::mem::size_of::<crate::sys::dirent>();
        let mut off = 2 * de_size;
        while off < (*dp).size as usize {
            let mut de = MaybeUninit::<crate::sys::dirent>::uninit();
            if (*dp).read(false, de.as_mut_ptr().addr(), off, de_size) != Ok(de_size) {
                panic!("isdirempty: readi");
            }
            if de.assume_init().inum != 0 {
                return false;
            }
            off += de_size;
        }
        true
    }
}

unsafe fn create(
    path: *mut c_char,
    type_: c_short,
    major: c_short,
    minor: c_short,
) -> *mut Inode {
    unsafe {
        let mut name = [0u8; DIRSIZ];
        let dp = crate::sys::nameiparent(path, name.as_mut_ptr().cast());
        if dp.is_null() {
            return core::ptr::null_mut();
        }
        (*dp).lock();

        let ip = crate::fs::dirlookup(dp, name.as_mut_ptr().cast(), core::ptr::null_mut());
        if !ip.is_null() {
            (*dp).unlock_put();
            (*ip).lock();
            if type_ == InodeType::File as c_short
                && ((*ip).type_ == InodeType::File || (*ip).type_ == InodeType::Device)
            {
                return ip;
            }
            (*ip).unlock_put();
            return core::ptr::null_mut();
        }

        let ip = crate::sys::ialloc((*dp).dev, type_);
        if ip.is_null() {
            (*dp).unlock_put();
            return core::ptr::null_mut();
        }

        (*ip).lock();
        (*ip).major = major;
        (*ip).minor = minor;
        (*ip).nlink = 1;
        crate::sys::iupdate(ip);

        let ok = (|| {
            if type_ == InodeType::Dir as c_short {
                if crate::sys::dirlink(ip, c".".as_ptr().cast_mut(), (*ip).inum) < 0
                    || crate::sys::dirlink(ip, c"..".as_ptr().cast_mut(), (*dp).inum) < 0
                {
                    return false;
                }
            }
            crate::sys::dirlink(dp, name.as_mut_ptr().cast(), (*ip).inum) >= 0
        })();

        if !ok {
            (*ip).nlink = 0;
            crate::sys::iupdate(ip);
            (*ip).unlock_put();
            (*dp).unlock_put();
            return core::ptr::null_mut();
        }

        if type_ == InodeType::Dir as c_short {
            (*dp).nlink += 1;
            crate::sys::iupdate(dp);
        }

        (*dp).unlock_put();
        ip
    }
}

// Create the path new as a link to the same inode as old.
pub(super) unsafe extern "C" fn link() -> u64 {
    use crate::{log::OpGuard, param::MAXPATH};

    let mut name = [0u8; DIRSIZ];
    let mut old = MaybeUninit::<[c_char; MAXPATH]>::uninit();
    let mut new = MaybeUninit::<[c_char; MAXPATH]>::uninit();

    if unsafe { argstr(0, old.as_mut()) } < 0 || unsafe { argstr(1, new.as_mut()) } < 0 {
        return (-1i64).cast_unsigned();
    }

    let _op_guard = OpGuard::new();

    let ip = unsafe { Inode::namei(old.as_mut_ptr().cast()) };
    if ip.is_null() {
        return (-1i64).cast_unsigned();
    }

    unsafe {
        (*ip).lock();
        if (*ip).type_ == InodeType::Dir {
            (*ip).unlock_put();
            return (-1i64).cast_unsigned();
        }
        (*ip).nlink += 1;
        crate::sys::iupdate(ip);
        (*ip).unlock();
    }

    let ok = unsafe {
        let dp = crate::sys::nameiparent(new.as_mut_ptr().cast(), name.as_mut_ptr().cast());
        if dp.is_null() {
            false
        } else {
            (*dp).lock();
            if (*dp).dev != (*ip).dev
                || crate::sys::dirlink(dp, name.as_mut_ptr().cast(), (*ip).inum) < 0
            {
                (*dp).unlock_put();
                false
            } else {
                (*dp).unlock_put();
                (*ip).put();
                true
            }
        }
    };

    if ok {
        0
    } else {
        unsafe {
            (*ip).lock();
            (*ip).nlink -= 1;
            crate::sys::iupdate(ip);
            (*ip).unlock_put();
        }
        (-1i64).cast_unsigned()
    }
}

pub(super) unsafe extern "C" fn unlink() -> u64 {
    use crate::{log::OpGuard, param::MAXPATH};

    let mut name = [0u8; DIRSIZ];
    let mut path = MaybeUninit::<[c_char; MAXPATH]>::uninit();

    if unsafe { argstr(0, path.as_mut()) } < 0 {
        return (-1i64).cast_unsigned();
    }

    let _op_guard = OpGuard::new();

    let dp =
        unsafe { crate::sys::nameiparent(path.as_mut_ptr().cast(), name.as_mut_ptr().cast()) };
    if dp.is_null() {
        return (-1i64).cast_unsigned();
    }

    unsafe { (*dp).lock() };

    // Cannot unlink "." or "..".
    if unsafe {
        crate::sys::namecmp(name.as_ptr(), c".".as_ptr()) == 0
            || crate::sys::namecmp(name.as_ptr(), c"..".as_ptr()) == 0
    } {
        unsafe { (*dp).unlock_put() };
        return (-1i64).cast_unsigned();
    }

    let mut off: c_uint = 0;
    let ip = unsafe { crate::fs::dirlookup(dp, name.as_mut_ptr().cast(), &mut off) };
    if ip.is_null() {
        unsafe { (*dp).unlock_put() };
        return (-1i64).cast_unsigned();
    }

    unsafe { (*ip).lock() };

    if unsafe { (*ip).nlink } < 1 {
        panic!("unlink: nlink < 1");
    }
    if unsafe { (*ip).type_ } == InodeType::Dir && !unsafe { isdirempty(ip) } {
        unsafe {
            (*ip).unlock_put();
            (*dp).unlock_put();
        }
        return (-1i64).cast_unsigned();
    }

    let de: crate::sys::dirent = unsafe { core::mem::zeroed() };
    let de_size = core::mem::size_of::<crate::sys::dirent>();
    if unsafe {
        (*dp).write(false, core::ptr::addr_of!(de).addr() as u64, off as usize, de_size)
    } != Ok(de_size)
    {
        panic!("unlink: writei");
    }

    if unsafe { (*ip).type_ } == InodeType::Dir {
        unsafe {
            (*dp).nlink -= 1;
            crate::sys::iupdate(dp);
        }
    }
    unsafe { (*dp).unlock_put() };

    unsafe {
        (*ip).nlink -= 1;
        crate::sys::iupdate(ip);
        (*ip).unlock_put();
    }

    0
}

pub(super) unsafe extern "C" fn mkdir() -> u64 {
    use crate::{log::OpGuard, param::MAXPATH};

    let mut path = MaybeUninit::<[c_char; MAXPATH]>::uninit();

    let _op_guard = OpGuard::new();
    if unsafe { argstr(0, path.as_mut()) } < 0 {
        return (-1i64).cast_unsigned();
    }
    let ip = unsafe { create(path.as_mut_ptr().cast(), InodeType::Dir as c_short, 0, 0) };
    if ip.is_null() {
        return (-1i64).cast_unsigned();
    }
    unsafe { (*ip).unlock_put() };
    0
}

pub(super) unsafe extern "C" fn mknod() -> u64 {
    use crate::{log::OpGuard, param::MAXPATH};

    let mut path = MaybeUninit::<[c_char; MAXPATH]>::uninit();
    let major = unsafe { argint(1) } as c_short;
    let minor = unsafe { argint(2) } as c_short;

    let _op_guard = OpGuard::new();
    if unsafe { argstr(0, path.as_mut()) } < 0 {
        return (-1i64).cast_unsigned();
    }
    let ip =
        unsafe { create(path.as_mut_ptr().cast(), InodeType::Device as c_short, major, minor) };
    if ip.is_null() {
        return (-1i64).cast_unsigned();
    }
    unsafe { (*ip).unlock_put() };
    0
}

pub(super) unsafe extern "C" fn chdir() -> u64 {
    use crate::{log::OpGuard, param::MAXPATH};

    let mut path = MaybeUninit::<[c_char; MAXPATH]>::uninit();
    let p = unsafe { myproc().as_mut().unwrap() };

    let _op_guard = OpGuard::new();
    if unsafe { argstr(0, path.as_mut()) } < 0 {
        return (-1i64).cast_unsigned();
    }
    let ip = unsafe { Inode::namei(path.as_mut_ptr().cast()) };
    if ip.is_null() {
        return (-1i64).cast_unsigned();
    }
    unsafe {
        (*ip).lock();
        if (*ip).type_ != InodeType::Dir {
            (*ip).unlock_put();
            return (-1i64).cast_unsigned();
        }
        (*ip).unlock();
        (*p.cwd).put();
        p.cwd = ip;
    }
    0
}
