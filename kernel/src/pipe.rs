use alloc::boxed::Box;
use core::ffi::{c_char, c_int, c_uint};

use crate::{
    file::{File, FileType},
    sys::{fileclose, initlock},
};

#[repr(C)]
pub struct Pipe {
    lock: crate::sys::spinlock,
    data: [c_char; 512],
    /// number of bytes read
    nread: c_uint,
    /// number of bytes written
    nwrite: c_uint,
    /// read fd is still open
    readopen: c_int,
    /// write fd is still open
    writeopen: c_int,
}

pub(super) fn alloc() -> Result<(*mut File, *mut File), ()> {
    let f0 = crate::file::alloc().ok_or(())?.as_ptr();
    let Some(f1) = crate::file::alloc() else {
        unsafe {
            fileclose(f0);
        }
        return Err(());
    };
    let f1 = f1.as_ptr();

    let mut pi = Box::<Pipe>::try_new_uninit().map_err(|_| ())?;
    unsafe {
        let pi = pi.as_mut_ptr();
        (*pi).readopen = 1;
        (*pi).writeopen = 1;
        (*pi).nwrite = 0;
        (*pi).nread = 0;
        initlock(&mut (*pi).lock, c"pipe".as_ptr().cast_mut());
        (*f0).type_ = FileType::Pipe;
        (*f0).readable = 1;
        (*f0).writable = 0;
        (*f0).pipe = pi;
        (*f1).type_ = FileType::Pipe;
        (*f1).readable = 0;
        (*f1).writable = 1;
        (*f1).pipe = pi;
    }
    _ = Box::into_raw(pi);
    Ok((f0, f1))
}
