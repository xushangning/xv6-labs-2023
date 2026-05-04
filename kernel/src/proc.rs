use core::ffi::c_int;

use crate::sys::{proc_, spinlock};

unsafe extern "C" {
    fn allocproc() -> *mut proc_;
    fn freeproc(p: *mut proc_);
    static mut wait_lock: spinlock;
}

/// Must be called with interrupts disabled,
/// to prevent race with process being moved
/// to a different CPU.
pub(super) unsafe fn cpuid() -> usize {
    unsafe { crate::riscv::tp::read() }
}

/// Create a new process, copying the parent.
/// Sets up child kernel stack to return as if from fork() system call.
pub(super) fn fork() -> c_int {
    use crate::sys::{
        NOFILE, acquire, filedup, idup, myproc, procstate_RUNNABLE, release, safestrcpy, uvmcopy,
    };

    let p = unsafe { myproc().as_mut().unwrap() };

    // Allocate process.
    let Some(np) = (unsafe { allocproc().as_mut() }) else {
        return -1;
    };

    // Copy user memory from parent to child.
    if unsafe { uvmcopy(p.pagetable, np.pagetable, p.sz) } < 0 {
        unsafe {
            freeproc(np);
            release(&mut np.lock);
        }
        return -1;
    }
    np.sz = p.sz;

    // copy saved user registers.
    unsafe {
        *np.trapframe = *p.trapframe;

        // Cause fork to return 0 in the child.
        (*np.trapframe).a0 = 0;

        // increment reference counts on open file descriptors.
        for i in 0..NOFILE as usize {
            if !p.ofile[i].is_null() {
                np.ofile[i] = filedup(p.ofile[i]);
            }
        }
        np.cwd = idup(p.cwd);

        safestrcpy(
            np.name.as_mut_ptr(),
            p.name.as_ptr(),
            np.name.len() as c_int,
        );
    }

    let pid = np.pid;

    unsafe {
        release(&mut np.lock);

        acquire(&raw mut wait_lock);
        np.parent = p;
        release(&raw mut wait_lock);

        acquire(&raw mut np.lock);
        np.state = procstate_RUNNABLE;
        release(&raw mut np.lock);
    }

    pid
}
