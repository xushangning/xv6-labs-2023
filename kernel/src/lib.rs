#![no_std]

use core::{
    ffi::{c_char, c_int},
    hint,
    sync::atomic::{AtomicBool, Ordering},
};

pub(crate) mod memlayout;
pub(crate) mod param;
mod proc;
pub(crate) mod riscv;
mod start;

static STARTED: AtomicBool = AtomicBool::new(false);

/// start() jumps here in supervisor mode on all CPUs.
extern "C" fn main() {
    unsafe extern "C" {
        fn consoleinit();
        fn printfinit();
        fn printf(fmt: *const c_char, ...);
        fn kinit();
        fn kvminit();
        fn kvminithart();
        fn procinit();
        fn trapinit();
        fn trapinithart();
        fn plicinit();
        fn plicinithart();
        fn binit();
        fn iinit();
        fn fileinit();
        fn virtio_disk_init();
        fn userinit();
        fn scheduler();
    }

    unsafe {
        if proc::cpuid() == 0 {
            consoleinit();
            printfinit();
            printf(c"\n".as_ptr());
            printf(c"xv6 kernel is booting\n".as_ptr());
            printf(c"\n".as_ptr());
            kinit(); // physical page allocator
            kvminit(); // create kernel page table
            kvminithart(); // turn on paging
            procinit(); // process table
            trapinit(); // trap vectors
            trapinithart(); // install kernel trap vector
            plicinit(); // set up interrupt controller
            plicinithart(); // ask PLIC for device interrupts
            binit(); // buffer cache
            iinit(); // inode table
            fileinit(); // file table
            virtio_disk_init(); // emulated hard disk
            userinit(); // first user process
            STARTED.store(true, Ordering::Release);
        } else {
            while !STARTED.load(Ordering::Acquire) {
                hint::spin_loop();
            }
            printf(c"hart %d starting\n".as_ptr(), proc::cpuid() as c_int);
            kvminithart(); // turn on paging
            trapinithart(); // install kernel trap vector
            plicinithart(); // ask PLIC for device interrupts
        }

        scheduler();
    }
}
