use alloc::boxed::Box;
use core::{
    ffi::{c_char, c_int, c_void},
    mem::{self, DropGuard, ManuallyDrop, MaybeUninit},
    ptr,
    sync::atomic::{AtomicBool, AtomicI32, Ordering},
};

use crate::{
    memlayout::{TRAMPOLINE, TRAPFRAME},
    riscv::PGSIZE,
    spinlock::MutexGuard,
    sys::{
        acquire, myproc, procstate_RUNNABLE, procstate_SLEEPING, procstate_UNUSED,
        procstate_ZOMBIE, release, safestrcpy, spinlock, wakeup,
    },
    vm::{PageTable, ProcVm, PteFlags, Vm},
};

#[repr(C)]
pub(super) struct Proc {
    pub lock: spinlock,
    pub state: crate::sys::procstate,
    pub chan: *mut c_void,
    pub killed: c_int,
    pub xstate: c_int,
    pub pid: c_int,
    pub parent: *mut Proc,
    pub kstack: u64,
    pub sz: u64,
    pub pagetable: Option<Box<PageTable>>,
    pub trapframe: Option<Box<MaybeUninit<crate::sys::trapframe>>>,
    pub context: crate::sys::context,
    pub ofile: [*mut crate::sys::file; 16],
    pub cwd: *mut crate::sys::inode,
    pub name: [c_char; 16],
}

unsafe extern "C" {
    static mut proc: [Proc; 64];

    static mut initproc: *mut Proc;

    static mut wait_lock: spinlock;
}

/// Must be called with interrupts disabled,
/// to prevent race with process being moved
/// to a different CPU.
pub(super) unsafe fn cpuid() -> usize {
    unsafe { crate::riscv::tp::read() }
}

fn allocpid() -> c_int {
    static NEXT_PID: AtomicI32 = AtomicI32::new(1);

    NEXT_PID.fetch_add(1, Ordering::Relaxed)
}

/// Look in the process table for an UNUSED proc.
/// If found, initialize state required to run in the kernel,
/// and return with p->lock held.
/// If there are no free procs, or a memory allocation fails, return 0.
fn allocproc() -> *mut Proc {
    use crate::sys::procstate_USED;

    for p in unsafe { &mut *&raw mut proc } {
        unsafe {
            acquire(&mut p.lock);
        }
        if p.state == procstate_UNUSED {
            p.pid = allocpid();
            p.state = procstate_USED;

            unsafe {
                // Allocate a trapframe page.
                p.trapframe = Box::try_new_uninit().ok();
                if p.trapframe.is_none() {
                    ptr::drop_in_place(p);
                    release(&mut p.lock);
                    return ptr::null_mut();
                }

                // An empty user page table.
                let Ok(proc_vm) = ProcVm::new(p) else {
                    ptr::drop_in_place(p);
                    release(&mut p.lock);
                    return ptr::null_mut();
                };
                p.pagetable = Some(proc_vm.leak());

                // Set up new context to start executing at forkret,
                // which returns to user space.
                (&raw mut p.context).write_bytes(0, 1);
                p.context.ra = (forkret as *const ()).addr().try_into().unwrap();
                p.context.sp = p.kstack + PGSIZE as u64;

                return p;
            }
        } else {
            unsafe {
                release(&mut p.lock);
            }
        }
    }
    ptr::null_mut()
}

/// free a proc structure and the data hanging from it,
/// including user pages.
/// p->lock must be held.
impl Drop for Proc {
    fn drop(&mut self) {
        self.trapframe = None;
        if let Some(pt) = self.pagetable.take() {
            mem::drop(ProcVm(Vm {
                pagetable: pt,
                sz: self.sz.try_into().unwrap(),
            }));
        }
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

impl ProcVm {
    /// Create a user page table for a given process, with no user memory,
    /// but with trampoline and trapframe pages.
    pub(super) fn new(p: &Proc) -> Result<Self, ()> {
        let mut vm = Vm::new(crate::vm::uvmcreate().map_err(|_| ())?);

        // map the trampoline code (for system call return)
        // at the highest user virtual address.
        // only the supervisor uses it, on the way
        // to/from user space, so not PTE_U.
        vm.pagetable.insert(
            TRAMPOLINE..TRAMPOLINE + PGSIZE,
            crate::trampoline::trampoline as *const _,
            PteFlags::R | PteFlags::X,
        )?;

        // map the trapframe page just below the trampoline page, for
        // trampoline.S.
        match vm.pagetable.insert(
            TRAPFRAME..TRAPFRAME + PGSIZE,
            p.trapframe.as_ref().unwrap().as_ptr().cast(),
            PteFlags::R | PteFlags::W,
        ) {
            Ok(_) => Ok(Self(vm)),
            Err(_) => {
                vm.pagetable.remove(TRAMPOLINE..TRAMPOLINE + PGSIZE, false);
                Err(())
            }
        }
    }

    pub(super) fn leak(self) -> Box<PageTable> {
        let mut vm = ManuallyDrop::new(self);
        unsafe { (&raw mut vm.0.pagetable).read() }
    }
}

impl Drop for ProcVm {
    /// Free a process's page table, and free the
    /// physical memory it refers to.
    fn drop(&mut self) {
        self.0
            .pagetable
            .remove(TRAMPOLINE..TRAMPOLINE + PGSIZE, false);
        self.0
            .pagetable
            .remove(TRAPFRAME..TRAPFRAME + PGSIZE, false);
    }
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
        crate::vm::uvmfirst(p.pagetable.as_mut().unwrap(), INITCODE);
        p.sz = PGSIZE.try_into().unwrap();

        // prepare for the very first "return" from kernel to user.
        let trapframe = p.trapframe.as_mut().unwrap().assume_init_mut();
        trapframe.epc = 0; // user program counter
        trapframe.sp = PGSIZE.try_into().unwrap(); // user stack pointer

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

/// Grow or shrink user memory by n bytes.
/// Return 0 on success, -1 on failure.
pub(super) fn growproc(n: c_int) -> c_int {
    use crate::sys::uvmdealloc;

    let p = unsafe { myproc().as_mut().unwrap() };

    let mut sz = p.sz;
    if n > 0 {
        let mut vm = DropGuard::new(
            ProcVm(Vm {
                pagetable: p.pagetable.take().unwrap(),
                sz: p.sz.try_into().unwrap(),
            }),
            |vm| p.pagetable = Some(vm.leak()),
        );
        if vm
            .extend_with(sz as usize + n as usize, PteFlags::W)
            .is_err()
        {
            return -1;
        }
        sz = vm.0.sz.try_into().unwrap();
    } else if n < 0 {
        sz = unsafe {
            uvmdealloc(
                p.pagetable.as_mut().unwrap().as_mut(),
                sz,
                sz.wrapping_sub((-n) as u64),
            )
        };
    }
    p.sz = sz;
    0
}

/// Create a new process, copying the parent.
/// Sets up child kernel stack to return as if from fork() system call.
pub(super) fn fork() -> c_int {
    use crate::sys::{NOFILE, idup, uvmcopy};

    let p = unsafe { myproc().as_mut().unwrap() };

    // Allocate process.
    let Some(np) = (unsafe { allocproc().as_mut() }) else {
        return -1;
    };

    // Copy user memory from parent to child.
    if unsafe {
        uvmcopy(
            p.pagetable.as_mut().unwrap().as_mut(),
            np.pagetable.as_mut().unwrap().as_mut(),
            p.sz,
        )
    } < 0
    {
        unsafe {
            ptr::drop_in_place(np);
            release(&mut np.lock);
        }
        return -1;
    }
    np.sz = p.sz;

    // copy saved user registers.
    unsafe {
        *np.trapframe.as_mut().unwrap().assume_init_mut() =
            *p.trapframe.as_ref().unwrap().assume_init_ref();

        // Cause fork to return 0 in the child.
        np.trapframe.as_mut().unwrap().assume_init_mut().a0 = 0;

        // increment reference counts on open file descriptors.
        for i in 0..NOFILE as usize {
            if !p.ofile[i].is_null() {
                np.ofile[i] = crate::file::dup(p.ofile[i]);
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

/// Pass p's abandoned children to init.
/// Caller must hold wait_lock.
fn reparent(p: *mut Proc) {
    unsafe {
        for pp in &mut *&raw mut proc {
            if pp.parent == p {
                pp.parent = initproc;
                wakeup(initproc.cast());
            }
        }
    }
}

/// Exit the current process.  Does not return.
/// An exited process remains in the zombie state
/// until its parent calls wait().
pub(super) fn exit(status: c_int) -> ! {
    use crate::sys::sched;

    let p = unsafe { myproc().as_mut().unwrap() };

    if ptr::from_mut(p) == unsafe { initproc } {
        panic!("init exiting");
    }

    // Close all open files.
    for of in &mut p.ofile {
        if !of.is_null() {
            crate::file::close(*of);
            *of = ptr::null_mut();
        }
    }

    unsafe {
        crate::sys::begin_op();
        crate::sys::iput(p.cwd);
        crate::sys::end_op();
        p.cwd = ptr::null_mut();

        acquire(&raw mut wait_lock);

        // Give any children to init.
        reparent(p);

        // Parent might be sleeping in wait().
        wakeup(p.parent.cast());

        acquire(&mut p.lock);

        p.xstate = status;
        p.state = procstate_ZOMBIE;

        release(&raw mut wait_lock);

        // Jump into the scheduler, never to return.
        sched();
    }
    panic!("zombie exit");
}

/// Wait for a child process to exit and return its pid.
/// Return -1 if this process has no children.
pub(super) fn wait(addr: usize) -> c_int {
    use crate::sys::{killed, sleep};

    let p = unsafe { myproc().as_mut().unwrap() };

    unsafe {
        acquire(&raw mut wait_lock);

        loop {
            // Scan through table looking for exited children.
            let mut havekids = false;
            for pp in &mut *&raw mut proc {
                if pp.parent == p {
                    // make sure the child isn't still in exit() or swtch().
                    acquire(&mut pp.lock);

                    havekids = true;
                    if pp.state == procstate_ZOMBIE {
                        // Found one.
                        let pid = pp.pid;
                        if addr != 0
                            && crate::vm::copyout(
                                p.pagetable.as_mut().unwrap().as_mut(),
                                addr,
                                bytemuck::bytes_of(&pp.xstate),
                            )
                            .is_err()
                        {
                            release(&mut pp.lock);
                            release(&raw mut wait_lock);
                            return -1;
                        }
                        ptr::drop_in_place(pp);
                        release(&mut pp.lock);
                        release(&raw mut wait_lock);
                        return pid;
                    }
                    release(&mut pp.lock);
                }
            }

            // No point waiting if we don't have any children.
            if !havekids || killed(p) != 0 {
                release(&raw mut wait_lock);
                return -1;
            }

            // Wait for a child to exit.
            sleep(ptr::from_mut(p).cast(), &raw mut wait_lock); //DOC: wait-sleep
        }
    }
}

/// A fork child's very first scheduling by scheduler()
/// will swtch to forkret.
extern "C" fn forkret() {
    static FIRST: AtomicBool = AtomicBool::new(true);

    // Still holding p->lock from scheduler.
    unsafe {
        release(&mut (*myproc()).lock);
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

#[repr(transparent)]
pub(super) struct Condvar<T>(pub T);

impl<T> Condvar<T> {
    pub(super) const fn new(t: T) -> Self {
        Self(t)
    }

    /// Atomically release lock and sleep on chan.
    /// Reacquires lock when awakened.
    pub(super) fn wait<'a, U>(self: *const Self, guard: MutexGuard<'a, U>) -> MutexGuard<'a, U> {
        let p = unsafe { myproc().as_mut().unwrap() };

        // Must acquire p->lock in order to
        // change p->state and then call sched.
        // Once we hold p->lock, we can be
        // guaranteed that we won't miss any wakeup
        // (wakeup locks p->lock),
        // so it's okay to release lk.

        unsafe {
            acquire(&mut p.lock); //DOC: sleeplock1
            let lk = guard.lock;
            mem::drop(guard);

            // Go to sleep.
            p.chan = self.cast_mut().cast();
            p.state = procstate_SLEEPING;

            // Tidy up.
            crate::sys::sched();

            p.chan = ptr::null_mut();

            // Reacquire original lock.
            release(&mut p.lock);
            lk.lock()
        }
    }

    /// Wake up all processes sleeping on chan.
    /// Must be called without any p->lock.
    pub(super) fn notify_all(&self) {
        unsafe {
            for p in &mut *&raw mut proc {
                if ptr::from_mut(p) != myproc() {
                    acquire(&mut p.lock);
                    if p.state == procstate_SLEEPING
                        && p.chan == ptr::from_ref(self).cast_mut().cast()
                    {
                        p.state = procstate_RUNNABLE;
                    }
                    release(&mut p.lock);
                }
            }
        }
    }
}

/// Kill the process with the given pid.
/// The victim won't exit until it tries to return
/// to user space (see usertrap() in trap.c).
pub(super) fn kill(pid: c_int) -> c_int {
    use crate::sys::procstate_SLEEPING;

    unsafe {
        for p in &mut *&raw mut proc {
            acquire(&mut p.lock);
            if p.pid == pid {
                p.killed = 1;
                if p.state == procstate_SLEEPING {
                    // Wake process from sleep().
                    p.state = procstate_RUNNABLE;
                }
                release(&mut p.lock);
                return 0;
            }
            release(&mut p.lock);
        }
    }
    -1
}
