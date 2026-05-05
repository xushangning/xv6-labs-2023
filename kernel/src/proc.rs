use core::{
    ffi::c_int,
    ptr,
    sync::atomic::{AtomicBool, AtomicI32, Ordering},
};

use crate::{
    riscv::PGSIZE,
    sys::{acquire, proc_, procstate_RUNNABLE, procstate_UNUSED, release, safestrcpy, spinlock},
};

unsafe extern "C" {
    static mut proc: [proc_; 64];

    static mut initproc: *mut proc_;

    static mut wait_lock: spinlock;
}

fn allocpid() -> c_int {
    static NEXT_PID: AtomicI32 = AtomicI32::new(1);

    NEXT_PID.fetch_add(1, Ordering::Relaxed)
}

/// Look in the process table for an UNUSED proc.
/// If found, initialize state required to run in the kernel,
/// and return with p->lock held.
/// If there are no free procs, or a memory allocation fails, return 0.
fn allocproc() -> *mut proc_ {
    use crate::sys::procstate_USED;

    unsafe {
        for p in &mut *&raw mut proc {
            acquire(&mut p.lock);
            if p.state == procstate_UNUSED {
                p.pid = allocpid();
                p.state = procstate_USED;

                // Allocate a trapframe page.
                p.trapframe = crate::sys::kalloc().cast();
                if p.trapframe.is_null() {
                    ptr::drop_in_place(p);
                    release(&mut p.lock);
                    return ptr::null_mut();
                }

                // An empty user page table.
                p.pagetable = crate::sys::proc_pagetable(p);
                if p.pagetable.is_null() {
                    ptr::drop_in_place(p);
                    release(&mut p.lock);
                    return ptr::null_mut();
                }

                // Set up new context to start executing at forkret,
                // which returns to user space.
                (&raw mut p.context).write_bytes(0, 1);
                p.context.ra = (forkret as *const ()).addr().try_into().unwrap();
                p.context.sp = p.kstack + PGSIZE as u64;

                return p;
            } else {
                release(&mut p.lock);
            }
        }
    }
    ptr::null_mut()
}

/// free a proc structure and the data hanging from it,
/// including user pages.
/// p->lock must be held.
impl Drop for proc_ {
    fn drop(&mut self) {
        if !self.trapframe.is_null() {
            unsafe {
                crate::sys::kfree(self.trapframe.cast());
            }
        }
        self.trapframe = ptr::null_mut();
        if !self.pagetable.is_null() {
            unsafe {
                crate::sys::proc_freepagetable(self.pagetable, self.sz);
            }
        }
        self.pagetable = ptr::null_mut();
        self.sz = 0;
        self.pid = 0;
        self.parent = ptr::null_mut();
        self.name[0] = 0;
        self.chan = ptr::null_mut();
        self.killed = 0;
        self.xstate = 0;
        self.state = procstate_UNUSED;
    }
}

/// Must be called with interrupts disabled,
/// to prevent race with process being moved
/// to a different CPU.
pub(super) unsafe fn cpuid() -> usize {
    unsafe { crate::riscv::tp::read() }
}

/// Set up first user process.
pub(super) fn userinit() {
    /// a user program that calls exec("/init")
    /// assembled from ../user/initcode.S
    /// od -t xC ../user/initcode
    const INITCODE: &[u8] = &[
        0x17, 0x05, 0x00, 0x00, 0x13, 0x05, 0x45, 0x02, 0x97, 0x05, 0x00, 0x00, 0x93, 0x85, 0x35,
        0x02, 0x93, 0x08, 0x70, 0x00, 0x73, 0x00, 0x00, 0x00, 0x93, 0x08, 0x20, 0x00, 0x73, 0x00,
        0x00, 0x00, 0xef, 0xf0, 0x9f, 0xff, 0x2f, 0x69, 0x6e, 0x69, 0x74, 0x00, 0x00, 0x24, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    ];

    unsafe {
        let p = allocproc().as_mut().unwrap();
        initproc = p;

        // allocate one user page and copy initcode's instructions
        // and data into it.
        crate::sys::uvmfirst(
            p.pagetable,
            INITCODE.as_ptr().cast_mut(),
            INITCODE.len().try_into().unwrap(),
        );
        p.sz = PGSIZE.try_into().unwrap();

        // prepare for the very first "return" from kernel to user.
        (*p.trapframe).epc = 0; // user program counter
        (*p.trapframe).sp = PGSIZE.try_into().unwrap(); // user stack pointer

        safestrcpy(
            p.name.as_mut_ptr(),
            c"initcode".as_ptr(),
            p.name.len().try_into().unwrap(),
        );
        p.cwd = crate::sys::namei(c"/".as_ptr().cast_mut());

        p.state = procstate_RUNNABLE;

        release(&mut p.lock);
    }
}

/// Create a new process, copying the parent.
/// Sets up child kernel stack to return as if from fork() system call.
pub(super) fn fork() -> c_int {
    use crate::sys::{NOFILE, filedup, idup, myproc, uvmcopy};

    let p = unsafe { myproc().as_mut().unwrap() };

    // Allocate process.
    let Some(np) = (unsafe { allocproc().as_mut() }) else {
        return -1;
    };

    // Copy user memory from parent to child.
    if unsafe { uvmcopy(p.pagetable, np.pagetable, p.sz) } < 0 {
        unsafe {
            ptr::drop_in_place(np);
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

/// A fork child's very first scheduling by scheduler()
/// will swtch to forkret.
extern "C" fn forkret() {
    static FIRST: AtomicBool = AtomicBool::new(true);

    // Still holding p->lock from scheduler.
    unsafe {
        release(&mut (*crate::sys::myproc()).lock);
    }

    if FIRST.load(Ordering::Relaxed) {
        // File system initialization must be run in the context of a
        // regular process (e.g., because it calls sleep), and thus cannot
        // be run from main().
        unsafe {
            crate::sys::fsinit(crate::sys::ROOTDEV.cast_signed());
        }

        FIRST.store(false, Ordering::Relaxed);
    }

    crate::trap::usertrapret();
}
