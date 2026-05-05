use core::alloc::{GlobalAlloc, Layout};

use crate::riscv::PGSIZE;

pub struct Allocator;

unsafe impl GlobalAlloc for Allocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if layout.size() <= PGSIZE && layout.align() <= PGSIZE {
            unsafe { crate::sys::kalloc().cast() }
        } else {
            core::ptr::null_mut()
        }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, _layout: Layout) {
        unsafe { crate::sys::kfree(ptr.cast()) }
    }
}
