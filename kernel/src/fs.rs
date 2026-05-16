use core::ffi::{c_int, c_short, c_uint};

use crate::{stat::InodeType, sys::sleeplock};

const NDIRECT: usize = 12;

/// in-memory copy of an inode
#[repr(C)]
pub struct Inode {
    /// Device number
    pub dev: c_uint,
    /// Inode number
    pub inum: c_uint,
    /// Reference count
    pub ref_: c_int,
    /// protects everything below here
    pub lock: sleeplock,
    /// inode has been read from disk?
    pub valid: c_int,

    pub type_: InodeType,
    pub major: c_short,
    pub minor: c_short,
    pub nlink: c_short,
    pub size: c_uint,
    pub addrs: [c_uint; NDIRECT + 1],
}

/// Read data from inode.
/// Caller must hold ip->lock.
/// If user_dst==1, then dst is a user virtual address;
/// otherwise, dst is a kernel address.
pub(super) unsafe fn readi(
    ip: *mut crate::sys::inode,
    user_dst: bool,
    dst: usize,
    off: usize,
    n: usize,
) -> Result<usize, ()> {
    let ret = unsafe {
        crate::sys::readi(
            ip,
            user_dst.into(),
            dst.try_into().unwrap(),
            off.try_into().unwrap(),
            n.try_into().unwrap(),
        )
    };
    usize::try_from(ret).map_err(|_| ())
}
