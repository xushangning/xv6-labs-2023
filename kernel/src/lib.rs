#![no_std]

use core::{
    hint,
    sync::atomic::{AtomicBool, Ordering},
};

mod console;
mod kernelvec;
mod memlayout;
mod param;
mod printf;
mod proc;
mod riscv;
mod spinlock;
mod start;
mod sys;
mod syscall;
mod trampoline;
mod trap;
mod uart;

pub use printf::panic;

static STARTED: AtomicBool = AtomicBool::new(false);

/// start() jumps here in supervisor mode on all CPUs.
extern "C" fn main() {
    unsafe {
        if proc::cpuid() == 0 {
            console::init();
            sys::printfinit();
            println!();
            println!("xv6 kernel is booting");
            println!();
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
            proc::userinit(); // first user process
            STARTED.store(true, Ordering::Release);
        } else {
            while !STARTED.load(Ordering::Acquire) {
                hint::spin_loop();
            }
            println!("hart {} starting", proc::cpuid());
            sys::kvminithart(); // turn on paging
            sys::trapinithart(); // install kernel trap vector
            sys::plicinithart(); // ask PLIC for device interrupts
        }

        sys::scheduler();
    }
}
