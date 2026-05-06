use core::{
    alloc::{GlobalAlloc, Layout},
    ops::{Deref, DerefMut},
};

use crate::riscv::PGSIZE;

#[repr(C, align(4096))]
pub(crate) struct Page([u8; PGSIZE]);

impl Deref for Page {
    type Target = [u8; PGSIZE];
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for Page {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

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
