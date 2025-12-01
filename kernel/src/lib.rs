#![no_std]

use core::{
    ffi::c_int,
    hint,
    sync::atomic::{AtomicBool, Ordering},
};

pub(crate) mod memlayout;
pub(crate) mod param;
mod proc;
pub(crate) mod riscv;
mod start;
pub(crate) mod sys;

static STARTED: AtomicBool = AtomicBool::new(false);

/// start() jumps here in supervisor mode on all CPUs.
extern "C" fn main() {
    use crate::sys::printf;

    unsafe {
        if proc::cpuid() == 0 {
            sys::consoleinit();
            sys::printfinit();
            printf(c"\n".as_ptr().cast_mut());
            printf(c"xv6 kernel is booting\n".as_ptr().cast_mut());
            printf(c"\n".as_ptr().cast_mut());
            sys::kinit(); // physical page allocator
            sys::kvminit(); // create kernel page table
            sys::kvminithart(); // turn on paging
            sys::procinit(); // process table
            sys::trapinit(); // trap vectors
            sys::trapinithart(); // install kernel trap vector
            sys::plicinit(); // set up interrupt controller
            sys::plicinithart(); // ask PLIC for device interrupts
            sys::binit(); // buffer cache
            sys::iinit(); // inode table
            sys::fileinit(); // file table
            sys::virtio_disk_init(); // emulated hard disk
            sys::userinit(); // first user process
            STARTED.store(true, Ordering::Release);
        } else {
            while !STARTED.load(Ordering::Acquire) {
                hint::spin_loop();
            }
            printf(
                c"hart %d starting\n".as_ptr().cast_mut(),
                proc::cpuid() as c_int,
            );
            sys::kvminithart(); // turn on paging
            sys::trapinithart(); // install kernel trap vector
            sys::plicinithart(); // ask PLIC for device interrupts
        }

        sys::scheduler();
    }
}
