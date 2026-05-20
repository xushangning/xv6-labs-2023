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


impl Inode {
    fn iblock(inum: c_uint) -> c_uint {
        let ipb = (BSIZE / core::mem::size_of::<crate::sys::dinode>()) as c_uint;
        unsafe { inum / ipb + sb.inodestart }
    }

    // Unlock the given inode.
    pub unsafe fn unlock(&mut self) {
        if unsafe { crate::sys::holdingsleep(&mut self.lock) } == 0 || self.ref_ < 1 {
            panic!("iunlock");
        }
        unsafe { crate::sys::releasesleep(&mut self.lock) }
    }

    // Lock the given inode.
    // Reads the inode from disk if necessary.
    pub unsafe fn lock(&mut self) {
        if self.ref_ < 1 {
            panic!("ilock");
        }
        unsafe {
            let ipb = BSIZE / core::mem::size_of::<crate::sys::dinode>();
            crate::sys::acquiresleep(&mut self.lock);
            if self.valid == 0 {
                let bp = crate::sys::bread(self.dev, Self::iblock(self.inum));
                let dip = ((*bp).data.as_ptr() as *const crate::sys::dinode)
                    .add(self.inum as usize % ipb);
                let t = (*dip).type_;
                self.major = (*dip).major;
                self.minor = (*dip).minor;
                self.nlink = (*dip).nlink;
                self.size = (*dip).size;
                self.addrs.copy_from_slice(&(*dip).addrs);
                crate::sys::brelse(bp);
                self.valid = 1;
                if t == 0 {
                    panic!("ilock: no type");
                }
                self.type_ = core::mem::transmute(t);
            }
        }
    }

    // Increment reference count for ip.
    // Returns ip to enable ip = idup(ip1) idiom.
    pub unsafe fn dup(&mut self) -> *mut Self {
        unsafe {
            let lk = crate::sys::itable_lock();
            crate::sys::acquire(lk);
            self.ref_ += 1;
            crate::sys::release(lk);
        }
        self
    }

    // Common idiom: unlock, then put.
    pub unsafe fn unlock_put(&mut self) {
        unsafe {
            self.unlock();
            self.put();
        }
    }

    // Drop a reference to an in-memory inode.
    // If that was the last reference, the inode table entry can
    // be recycled.
    // If that was the last reference and the inode has no links
    // to it, free the inode (and its content) on disk.
    // All calls to iput() must be inside a transaction in
    // case it has to free the inode.
    pub unsafe fn put(&mut self) {
        unsafe {
            let lk = crate::sys::itable_lock();
            crate::sys::acquire(lk);
            if self.ref_ == 1 && self.valid != 0 && self.nlink == 0 {
                // inode has no links and no other references: truncate and free.
                crate::sys::acquiresleep(&mut self.lock);
                crate::sys::release(lk);
                self.trunc();
                self.type_ = InodeType::Unknown;
                crate::sys::iupdate(self);
                self.valid = 0;
                crate::sys::releasesleep(&mut self.lock);
                crate::sys::acquire(lk);
            }
            self.ref_ -= 1;
            crate::sys::release(lk);
        }
    }

    // Truncate inode (discard contents).
    // Caller must hold ip->lock.
    pub unsafe fn trunc(&mut self) {
        unsafe {
            for i in 0..NDIRECT {
                if self.addrs[i] != 0 {
                    crate::sys::bfree(self.dev as c_int, self.addrs[i]);
                    self.addrs[i] = 0;
                }
            }
            if self.addrs[NDIRECT] != 0 {
                let bp = crate::sys::bread(self.dev, self.addrs[NDIRECT]);
                let a = (*bp).data.as_ptr() as *const c_uint;
                for j in 0..NINDIRECT {
                    let addr = *a.add(j);
                    if addr != 0 {
                        crate::sys::bfree(self.dev as c_int, addr);
                    }
                }
                crate::sys::brelse(bp);
                crate::sys::bfree(self.dev as c_int, self.addrs[NDIRECT]);
                self.addrs[NDIRECT] = 0;
            }
            self.size = 0;
            crate::sys::iupdate(self);
        }
    }

    // Read data from inode.
    // Caller must hold ip->lock.
    // If user_dst==1, then dst is a user virtual address;
    // otherwise, dst is a kernel address.
    pub unsafe fn read(
        &mut self,
        user_dst: bool,
        mut dst: usize,
        mut off: usize,
        n: usize,
    ) -> Result<usize, ()> {
        if off > self.size as usize || off + n < off {
            return Ok(0);
        }
        let n = if off + n > self.size as usize {
            self.size as usize - off
        } else {
            n
        };
        let mut tot = 0;
        while tot < n {
            let addr =
                unsafe { crate::sys::bmap(self, (off / BSIZE) as c_uint) };
            if addr == 0 {
                break;
            }
            let bp = unsafe { crate::sys::bread(self.dev, addr) };
            let m = core::cmp::min(n - tot, BSIZE - off % BSIZE);
            let ok = unsafe {
                crate::sys::either_copyout(
                    user_dst as c_int,
                    dst as u64,
                    (*bp).data.as_ptr().add(off % BSIZE) as *mut core::ffi::c_void,
                    m as u64,
                ) != -1
            };
            unsafe { crate::sys::brelse(bp) };
            if !ok {
                return Err(());
            }
            tot += m;
            off += m;
            dst += m;
        }
        Ok(tot)
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
