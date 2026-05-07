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
