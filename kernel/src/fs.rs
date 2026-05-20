use core::ffi::{c_char, c_int, c_short, c_uint};

use crate::{stat::InodeType, sys::sleeplock};

pub(super) const BSIZE: usize = 1024;
const NDIRECT: usize = 12;
const NINDIRECT: usize = BSIZE / core::mem::size_of::<c_uint>();
const MAXFILE: usize = NDIRECT + NINDIRECT;
pub(super) const DIRSIZ: usize = 14;
const NINODE: usize = 50;

#[repr(C)]
struct Itable {
    lock: crate::sys::spinlock,
    inode: [Inode; NINODE],
}

unsafe extern "C" {
    static mut sb: crate::sys::superblock;
    static mut itable: Itable;
    fn bfree(dev: c_int, b: c_uint);
    fn bmap(ip: *mut Inode, bn: c_uint) -> c_uint;
    fn namex(path: *mut c_char, nameiparent: c_int, name: *mut c_char) -> *mut Inode;
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
            crate::sys::acquire(&raw mut itable.lock);
            self.ref_ += 1;
            crate::sys::release(&raw mut itable.lock);
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
            crate::sys::acquire(&raw mut itable.lock);
            if self.ref_ == 1 && self.valid != 0 && self.nlink == 0 {
                // inode has no links and no other references: truncate and free.
                crate::sys::acquiresleep(&mut self.lock);
                crate::sys::release(&raw mut itable.lock);
                self.trunc();
                self.type_ = InodeType::Unknown;
                crate::sys::iupdate(self);
                self.valid = 0;
                crate::sys::releasesleep(&mut self.lock);
                crate::sys::acquire(&raw mut itable.lock);
            }
            self.ref_ -= 1;
            crate::sys::release(&raw mut itable.lock);
        }
    }

    // Truncate inode (discard contents).
    // Caller must hold ip->lock.
    pub unsafe fn trunc(&mut self) {
        unsafe {
            for i in 0..NDIRECT {
                if self.addrs[i] != 0 {
                    bfree(self.dev as c_int, self.addrs[i]);
                    self.addrs[i] = 0;
                }
            }
            if self.addrs[NDIRECT] != 0 {
                let bp = crate::sys::bread(self.dev, self.addrs[NDIRECT]);
                let a = (*bp).data.as_ptr() as *const c_uint;
                for j in 0..NINDIRECT {
                    let addr = *a.add(j);
                    if addr != 0 {
                        bfree(self.dev as c_int, addr);
                    }
                }
                crate::sys::brelse(bp);
                bfree(self.dev as c_int, self.addrs[NDIRECT]);
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
            let addr = unsafe { bmap(self, (off / BSIZE) as c_uint) };
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

    // Write data to inode.
    // Caller must hold ip->lock.
    // If user_src==1, then src is a user virtual address;
    // otherwise, src is a kernel address.
    // Returns the number of bytes successfully written.
    // If the return value is less than the requested n,
    // there was an error of some kind.
    pub unsafe fn write(
        &mut self,
        user_src: bool,
        mut src: u64,
        mut off: usize,
        n: usize,
    ) -> Result<usize, ()> {
        if off > self.size as usize || off + n < off {
            return Err(());
        }
        if off + n > MAXFILE * BSIZE {
            return Err(());
        }
        let mut tot = 0;
        while tot < n {
            let addr = unsafe { bmap(self, (off / BSIZE) as c_uint) };
            if addr == 0 {
                break;
            }
            let bp = unsafe { crate::sys::bread(self.dev, addr) };
            let m = core::cmp::min(n - tot, BSIZE - off % BSIZE);
            let ok = unsafe {
                crate::sys::either_copyin(
                    (*bp).data.as_mut_ptr().add(off % BSIZE) as *mut core::ffi::c_void,
                    user_src as c_int,
                    src,
                    m as u64,
                ) != -1
            };
            if ok {
                unsafe {
                    crate::sys::log_write(bp);
                }
            }
            unsafe { crate::sys::brelse(bp) };
            if !ok {
                break;
            }
            tot += m;
            off += m;
            src += m as u64;
        }
        if off > self.size as usize {
            self.size = off as c_uint;
        }
        unsafe { crate::sys::iupdate(self) };
        Ok(tot)
    }

    // Look up and return the inode for a path name.
    // Must be called inside a transaction since it calls iput().
    pub unsafe fn namei(path: *mut c_char) -> *mut Self {
        let mut name = [0u8; DIRSIZ];
        unsafe { namex(path, 0, name.as_mut_ptr().cast()) }
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
