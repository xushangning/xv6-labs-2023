use alloc::boxed::Box;
use core::{
    ffi::{c_char, c_int, c_uint},
    mem::MaybeUninit,
    ptr,
};

use crate::{
    file::{File, FileKind},
    proc::Condvar,
    rc::{Rc, UniqueRc},
    spinlock::Mutex,
};

#[repr(C)]
struct PipeData {
    data: [c_char; 512],
    /// number of bytes read
    nread: Condvar<c_uint>,
    /// number of bytes written
    nwrite: Condvar<c_uint>,
    /// read fd is still open
    readopen: bool,
    /// write fd is still open
    writeopen: bool,
}

#[repr(transparent)]
pub struct Pipe(Mutex<PipeData>);

pub(super) fn alloc() -> Result<(Rc<File>, Rc<File>), ()> {
    let mut f0 = crate::file::alloc().ok_or(())?;
    let Some(mut f1) = crate::file::alloc() else {
        crate::file::close(UniqueRc::into_rc(f0));
        return Err(());
    };

    let pi = Box::into_non_null(
        Box::try_new(Pipe(Mutex::new(
            c"pipe",
            PipeData {
                data: [0; _],
                nread: Condvar::new(0),
                nwrite: Condvar::new(0),
                readopen: true,
                writeopen: true,
            },
        )))
        .map_err(|_| ())?,
    );

    *f0 = File {
        kind: FileKind::Pipe(pi),
        readable: true,
        writable: false,
    };
    *f1 = File {
        kind: FileKind::Pipe(pi),
        readable: false,
        writable: true,
    };
    Ok((UniqueRc::into_rc(f0), UniqueRc::into_rc(f1)))
}

pub(super) fn close(pi: &Pipe, writable: bool) {
    let both_closed = {
        let mut pi = pi.0.lock();
        if writable {
            pi.writeopen = false;
            pi.nread.notify_all();
        } else {
            pi.readopen = false;
            pi.nwrite.notify_all();
        }
        !pi.readopen && !pi.writeopen
    };
    if both_closed {
        unsafe { drop(Box::from_raw((ptr::from_ref(pi)).cast_mut())) };
    }
}

pub(super) fn write(pi: &Pipe, addr: u64, n: c_int) -> c_int {
    let pr = unsafe { crate::sys::myproc() };
    let mut pi = pi.0.lock();
    let mut i: c_int = 0;

    while i < n {
        if !pi.readopen || unsafe { crate::sys::killed(pr) } != 0 {
            return -1;
        }
        //DOC: pipewrite-full
        if pi.nwrite.0 == pi.nread.0 + (pi.data.len() as u32) {
            pi.nread.notify_all();
            let chan: *const Condvar<u32> = ptr::from_ref(&pi.nwrite);
            pi = Condvar::wait(chan, pi);
        } else {
            let mut ch = MaybeUninit::uninit();
            if unsafe {
                crate::sys::copyin(
                    (*pr).pagetable.as_mut().unwrap().as_mut(),
                    ch.as_mut_ptr(),
                    addr + i as u64,
                    1,
                )
            } == -1
            {
                break;
            }
            let idx = pi.nwrite.0 as usize % pi.data.len();
            pi.data[idx] = unsafe { ch.assume_init() };
            pi.nwrite.0 += 1;
            i += 1;
        }
    }
    pi.nread.notify_all();

    i
}

pub(super) fn read(pi: &Pipe, addr: u64, n: c_int) -> c_int {
    let pr = unsafe { crate::sys::myproc() };

    let mut pi = pi.0.lock();
    //DOC: pipe-empty
    while pi.nread.0 == pi.nwrite.0 && pi.writeopen {
        if unsafe { crate::sys::killed(pr) } != 0 {
            return -1;
        }
        pi = Condvar::wait(&raw const pi.nread, pi); //DOC: piperead-sleep
    }
    let mut i: c_int = 0;
    while i < n {
        //DOC: piperead-copy
        if pi.nread.0 == pi.nwrite.0 {
            break;
        }
        let ch = pi.data[pi.nread.0 as usize % pi.data.len()];
        pi.nread.0 += 1;
        if unsafe {
            crate::vm::copyout(
                (*pr).pagetable.as_mut().unwrap().as_mut(),
                addr as usize + i as usize,
                bytemuck::bytes_of(&ch),
            )
            .is_err()
        } {
            break;
        }
        i += 1;
    }
    pi.nwrite.notify_all(); //DOC: piperead-wakeup
    i
}
