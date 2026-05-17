#![no_std]
#![feature(allocator_api)]
#![feature(arbitrary_self_types_pointers)]
#![feature(drop_guard)]
#![feature(maybe_uninit_as_bytes)]

extern crate alloc;

use core::{
    hint,
    sync::atomic::{AtomicBool, Ordering},
};

mod console;
mod exec;
mod fcntl;
mod file;
mod fs;
mod kalloc;
mod kernelvec;
mod log;
mod memlayout;
mod param;
mod pipe;
mod printf;
mod proc;
mod riscv;
mod spinlock;
mod start;
mod stat;
mod sys;
mod syscall;
mod trampoline;
mod trap;
mod uart;
mod vm;

pub use kalloc::Allocator;
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
            trap::inithart(); // install kernel trap vector
            sys::plicinit(); // set up interrupt controller
            sys::plicinithart(); // ask PLIC for device interrupts
            sys::binit(); // buffer cache
            sys::iinit(); // inode table
            sys::virtio_disk_init(); // emulated hard disk
            proc::userinit(); // first user process
            STARTED.store(true, Ordering::Release);
        } else {
            while !STARTED.load(Ordering::Acquire) {
                hint::spin_loop();
            }
            println!("hart {} starting", proc::cpuid());
            sys::kvminithart(); // turn on paging
            trap::inithart(); // install kernel trap vector
            sys::plicinithart(); // ask PLIC for device interrupts
        }

        sys::scheduler();
    }
}
