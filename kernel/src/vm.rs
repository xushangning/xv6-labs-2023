use alloc::boxed::Box;
use core::ops::Range;

use crate::{kalloc::Page, riscv::PGSIZE};

#[repr(transparent)]
pub struct PageTable(PageTableLevel);

#[repr(C, align(4096))]
struct PageTableLevel([usize; 512]);

impl PageTable {
    /// Create PTEs for virtual addresses starting at va that refer to
    /// physical addresses starting at pa.
    /// va and size MUST be page-aligned.
    /// Returns 0 on success, -1 if walk() couldn't
    /// allocate a needed page-table page.
    fn insert(&mut self, va: Range<usize>, mut pa: *const Page, perm: u64) -> Result<(), ()> {
        use crate::sys::PTE_V;

        if va.start % PGSIZE != 0 {
            panic!("mappages: va not aligned");
        }
        if va.end % PGSIZE != 0 {
            panic!("mappages: size not aligned");
        }
        if va.start == va.end {
            panic!("mappages: size");
        }

        for a in (va.start..va.end).step_by(PGSIZE) {
            let pte = unsafe {
                crate::sys::walk(self.0.0.as_mut_ptr().cast(), a.try_into().unwrap(), 1)
                    .as_mut()
                    .ok_or(())?
            };
            if *pte & PTE_V as u64 != 0 {
                panic!("mappages: remap");
            }
            *pte = crate::riscv::pa2pte(pa) as u64 | perm | PTE_V as u64;
            unsafe {
                pa = pa.add(1);
            }
        }
        Ok(())
    }
}

/// Load the user initcode into address 0 of pagetable,
/// for the very first process.
/// src must be less than a page.
pub(super) fn uvmfirst(pagetable: &mut PageTable, src: &[u8]) {
    use crate::sys::{PTE_R, PTE_U, PTE_W, PTE_X};

    if src.len() >= PGSIZE {
        panic!("uvmfirst: more than a page");
    }
    unsafe {
        let mem = Box::leak(Box::<Page>::new_zeroed().assume_init());
        pagetable
            .insert(0..PGSIZE, mem, (PTE_W | PTE_R | PTE_X | PTE_U).into())
            .unwrap();
        mem.as_mut_ptr()
            .copy_from_nonoverlapping(src.as_ptr(), src.len());
    }
}
