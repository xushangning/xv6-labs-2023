use core::ffi::{c_int, c_short, c_uint};

use crate::{stat::InodeType, sys::sleeplock};

pub(super) const BSIZE: usize = 1024;
const NDIRECT: usize = 12;
const NINDIRECT: usize = BSIZE / core::mem::size_of::<c_uint>();
const MAXFILE: usize = NDIRECT + NINDIRECT;
pub(super) const DIRSIZ: usize = 14;

unsafe extern "C" {
    static mut sb: crate::sys::superblock;
}

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

impl Inode {
    fn iblock(inum: c_uint) -> c_uint {
        let ipb = (BSIZE / core::mem::size_of::<crate::sys::dinode>()) as c_uint;
        unsafe { inum / ipb + sb.inodestart }
    }

    // Copy stat information from inode.
    // Caller must hold ip->lock.
    pub unsafe fn stat(&mut self, st: *mut crate::sys::stat) {
        let st = unsafe { &mut *st };
        st.dev = self.dev as c_int;
        st.ino = self.inum;
        st.type_ = self.type_ as c_short;
        st.nlink = self.nlink;
        st.size = self.size as u64;
    }
}
