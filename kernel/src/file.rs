//! Support functions for system calls that involve file descriptors.

use core::{
    ffi::{c_char, c_int, c_short, c_uint},
    ptr::NonNull,
};

use crate::{
    log::OpGuard,
    param::{NDEV, NFILE},
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
#[allow(dead_code)]
pub enum FileType {
    None = 0,
    Pipe = 1,
    Inode = 2,
    Device = 3,
}

#[repr(C)]
struct Ftable {
    lock: crate::sys::spinlock,
    file: [File; NFILE],
}

unsafe extern "C" {
    static mut ftable: Ftable;
}

#[repr(C)]
pub struct File {
    pub type_: FileType,
    pub ref_: c_int,
    pub readable: c_char,
    pub writable: c_char,
    pub pipe: *mut crate::sys::pipe,
    pub ip: *mut crate::sys::inode,
    pub off: c_uint,
    pub major: c_short,
}

/// Allocate a file structure.
pub(super) fn alloc() -> Option<NonNull<File>> {
    let p = &raw mut ftable;
    unsafe {
        crate::sys::acquire(&raw mut (*p).lock);
        for f in &mut (*p).file {
            if f.ref_ == 0 {
                f.ref_ = 1;
                crate::sys::release(&raw mut (*p).lock);
                return Some(NonNull::from_mut(f));
            }
        }
        crate::sys::release(&raw mut (*p).lock);
    }
    None
}

impl File {
    /// Read from file f.
    /// addr is a user virtual address.
    pub(super) fn read(&mut self, addr: u64, n: c_int) -> c_int {
        if self.readable == 0 {
            return -1;
        }
        match self.type_ {
            FileType::Pipe => unsafe { crate::sys::piperead(self.pipe, addr, n) },
            FileType::Device => {
                let Ok(major) = usize::try_from(self.major) else {
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
            FileType::Inode => unsafe {
                crate::sys::ilock(self.ip);
                let r = crate::sys::readi(self.ip, 1, addr, self.off, n.try_into().unwrap());
                if r > 0 {
                    self.off += r.cast_unsigned();
                }
                crate::sys::iunlock(self.ip);
                r
            },
            FileType::None => panic!("fileread"),
        }
    }

    /// Write to file f.
    /// addr is a user virtual address.
    pub(super) fn write(&mut self, addr: u64, n: c_int) -> c_int {
        if self.writable == 0 {
            return -1;
        }
        match self.type_ {
            FileType::Pipe => unsafe { crate::sys::pipewrite(self.pipe, addr, n) },
            FileType::Device => {
                let Ok(major) = usize::try_from(self.major) else {
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
            FileType::Inode => {
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
                        crate::sys::ilock(self.ip);
                        let r = crate::sys::writei(
                            self.ip,
                            1,
                            addr + i as u64,
                            self.off,
                            n1.try_into().unwrap(),
                        );
                        if r > 0 {
                            self.off += r.cast_unsigned();
                        }
                        crate::sys::iunlock(self.ip);
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
            FileType::None => panic!("filewrite"),
        }
    }
}
