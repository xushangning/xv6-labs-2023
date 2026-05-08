//! Support functions for system calls that involve file descriptors.

use core::ffi::{c_char, c_int, c_short, c_uint};

unsafe extern "C" {
    static devsw: [crate::sys::devsw; crate::sys::NDEV as usize];
}

#[repr(C)]
#[allow(dead_code)]
pub enum FileType {
    None = 0,
    Pipe = 1,
    Inode = 2,
    Device = 3,
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
                let major = self.major;
                if major < 0 || major >= crate::sys::NDEV.try_into().unwrap() {
                    return -1;
                }
                unsafe {
                    match devsw[usize::try_from(major).unwrap()].read {
                        Some(f) => f(1, addr, n),
                        None => -1,
                    }
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
}
