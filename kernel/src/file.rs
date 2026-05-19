//! Support functions for system calls that involve file descriptors.

use core::{
    ffi::{c_int, c_short, c_uint},
    mem::{self, MaybeUninit},
    ops::DerefMut,
    ptr::NonNull,
    slice,
};

use crate::{
    log::OpGuard,
    param::{NDEV, NFILE},
    rc::RcInner,
    spinlock::Mutex,
};

pub(super) trait Device {
    fn read(&self, user_dst: bool, dst: u64, n: c_int) -> c_int;
    fn write(&self, user_dst: bool, src: u64, n: c_int) -> c_int;
}

type DevSw = [Option<&'static dyn Device>; NDEV];

/// map major device number to device functions.
const DEVSW: DevSw = {
    const CONSOLE: usize = 1;

    let mut arr: DevSw = [None; _];
    arr[CONSOLE] = Some(&crate::console::CONS);
    arr
};

#[repr(C)]
#[derive(Default)]
pub enum FileKind {
    #[default]
    None,
    Pipe(NonNull<crate::pipe::Pipe>),
    Inode {
        ip: NonNull<crate::sys::inode>,
        off: c_uint,
    },
    Device {
        ip: NonNull<crate::sys::inode>,
        major: c_short,
    },
}

#[repr(C)]
#[derive(Default)]
pub struct File {
    pub kind: FileKind,
    pub readable: bool,
    pub writable: bool,
}

unsafe impl Send for File {}

static FTABLE: Mutex<[RcInner<File>; NFILE]> = Mutex::new(
    c"ftable",
    [const { RcInner::new(unsafe { mem::zeroed() }) }; _],
);

/// Allocate a file structure.
pub(super) fn alloc() -> Option<NonNull<RcInner<File>>> {
    for f in FTABLE.lock().iter_mut() {
        if RcInner::strong(f) == 0 {
            RcInner::inc_strong(f);
            return Some(NonNull::from_mut(f));
        }
    }
    None
}

/// Increment ref count for file f.
pub(super) fn dup(mut f: NonNull<RcInner<File>>) -> NonNull<RcInner<File>> {
    {
        let _guard = FTABLE.lock();
        let f = unsafe { f.as_mut() };
        assert!(RcInner::strong(f) >= 1, "filedup");
        RcInner::inc_strong(f);
    }
    f
}

/// Close file f. (Decrement ref count, close when reaches 0.)
pub(super) fn close(mut f: NonNull<RcInner<File>>) {
    let ff = {
        let _guard = FTABLE.lock();
        let f = unsafe { f.as_mut() };
        assert!(RcInner::strong(f) >= 1, "fileclose");
        RcInner::dec_strong(f);
        if RcInner::strong(f) > 0 {
            return;
        }
        mem::take(f.deref_mut())
    };
    drop(ff);
}

impl Drop for File {
    fn drop(&mut self) {
        match self.kind {
            FileKind::Pipe(pipe) => crate::pipe::close(unsafe { pipe.as_ref() }, self.writable),
            FileKind::Inode { ip, .. } | FileKind::Device { ip, .. } => {
                let _op_guard = OpGuard::new();
                unsafe { crate::sys::iput(ip.as_ptr()) };
            }
            FileKind::None => {}
        }
    }
}

impl File {
    /// Get metadata about file f.
    /// addr is a user virtual address, pointing to a struct stat.
    pub(super) fn stat(&self, addr: usize) -> Result<(), ()> {
        let ip = match self.kind {
            FileKind::Inode { ip, .. } | FileKind::Device { ip, .. } => ip,
            _ => return Err(()),
        };
        let mut st = MaybeUninit::<crate::sys::stat>::uninit();
        unsafe {
            crate::sys::ilock(ip.as_ptr());
            crate::sys::stati(ip.as_ptr(), st.as_mut_ptr());
            crate::sys::iunlock(ip.as_ptr());
        }
        let p = unsafe { crate::sys::myproc() };
        unsafe {
            crate::vm::copyout(
                (*p).pagetable.as_mut().unwrap().as_mut(),
                addr,
                slice::from_raw_parts(st.as_mut_ptr().cast(), mem::size_of::<crate::sys::stat>()),
            )
        }
    }

    /// Read from file f.
    /// addr is a user virtual address.
    pub(super) fn read(&mut self, addr: u64, n: c_int) -> c_int {
        if !self.readable {
            return -1;
        }
        match &mut self.kind {
            FileKind::Pipe(pipe) => crate::pipe::read(unsafe { pipe.as_ref() }, addr, n),
            FileKind::Device { major, .. } => {
                let Ok(major) = usize::try_from(*major) else {
                    return -1;
                };
                match DEVSW.get(major) {
                    Some(d) => match d.as_ref() {
                        Some(d) => d.read(true, addr, n),
                        None => -1,
                    },
                    None => -1,
                }
            }
            FileKind::Inode { ip, off } => unsafe {
                crate::sys::ilock(ip.as_ptr());
                let r = crate::sys::readi(ip.as_ptr(), 1, addr, *off, n.try_into().unwrap());
                if r > 0 {
                    *off += r.cast_unsigned();
                }
                crate::sys::iunlock(ip.as_ptr());
                r
            },
            FileKind::None => panic!("fileread"),
        }
    }

    /// Write to file f.
    /// addr is a user virtual address.
    pub(super) fn write(&mut self, addr: u64, n: c_int) -> c_int {
        if !self.writable {
            return -1;
        }
        match &mut self.kind {
            FileKind::Pipe(pipe) => crate::pipe::write(unsafe { pipe.as_ref() }, addr, n),
            FileKind::Device { major, .. } => {
                let Ok(major) = usize::try_from(*major) else {
                    return -1;
                };
                match DEVSW.get(major) {
                    Some(d) => match d.as_ref() {
                        Some(d) => d.write(true, addr, n),
                        None => -1,
                    },
                    None => -1,
                }
            }
            FileKind::Inode { ip, off } => {
                // write a few blocks at a time to avoid exceeding
                // the maximum log transaction size, including
                // i-node, indirect block, allocation blocks,
                // and 2 blocks of slop for non-aligned writes.
                // this really belongs lower down, since writei()
                // might be writing a device like the console.
                let max = (((crate::sys::MAXOPBLOCKS as c_int) - 1 - 1 - 2) / 2)
                    * (crate::sys::BSIZE as c_int);
                let mut i: c_int = 0;
                while i < n {
                    let mut n1 = n - i;
                    if n1 > max {
                        n1 = max;
                    }
                    let r: c_int = unsafe {
                        let _op_guard = OpGuard::new();
                        crate::sys::ilock(ip.as_ptr());
                        let r = crate::sys::writei(
                            ip.as_ptr(),
                            1,
                            addr + i as u64,
                            *off,
                            n1.try_into().unwrap(),
                        );
                        if r > 0 {
                            *off += r.cast_unsigned();
                        }
                        crate::sys::iunlock(ip.as_ptr());
                        r
                    };
                    if r != n1 {
                        // error from writei
                        break;
                    }
                    i += r;
                }
                if i == n { n } else { -1 }
            }
            FileKind::None => panic!("filewrite"),
        }
    }
}
