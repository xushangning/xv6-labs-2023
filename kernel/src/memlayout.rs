//! Physical memory layout
//!
//! qemu -machine virt is set up like this,
//! based on qemu's hw/riscv/virt.c:
//!
//! 00001000 -- boot ROM, provided by qemu
//! 02000000 -- CLINT
//! 0C000000 -- PLIC
//! 10000000 -- uart0
//! 10001000 -- virtio disk
//! 80000000 -- boot ROM jumps here in machine mode
//!             -kernel loads the kernel here
//! unused RAM after 80000000.
//!
//! the kernel uses physical memory thus:
//! 80000000 -- entry.S, then kernel text and data
//! end -- start of kernel page allocation area
//! PHYSTOP -- end RAM used by the kernel

use crate::riscv::{MAXVA, PGSIZE};

pub(crate) mod uart0 {
    pub(crate) const IRQ: i32 = 10;
}

/// virtio mmio interface
pub(crate) mod virtio0 {
    pub(crate) const IRQ: i32 = 1;
}

/// core local interruptor (CLINT), which contains the timer.
pub(crate) mod clint {
    use core::ptr;

    const BASE: usize = 0x2000000;

    pub(crate) const MTIMECMP: *mut usize = ptr::with_exposed_provenance_mut(BASE + 0x4000);

    /// cycles since boot.
    pub(crate) const MTIME: *const usize = ptr::with_exposed_provenance(BASE + 0xBFF8);
}

// map the trampoline page to the highest address,
// in both user and kernel space.
pub(crate) const TRAMPOLINE: usize = MAXVA - PGSIZE;

// User memory layout.
// Address zero first:
//   text
//   original data and bss
//   fixed-size stack
//   expandable heap
//   ...
//   TRAPFRAME (p->trapframe, used by the trampoline)
//   TRAMPOLINE (the same page as in the kernel)
pub(crate) const TRAPFRAME: usize = TRAMPOLINE - PGSIZE;
