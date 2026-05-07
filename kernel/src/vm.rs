use alloc::boxed::Box;
use core::ops::Range;

use bitflags::bitflags;

use crate::{
    kalloc::Page,
    riscv::{PGSIZE, pa2pte},
};

#[repr(transparent)]
struct Pte(usize);

#[repr(transparent)]
pub struct PageTable(PageTableLevel);

#[repr(C, align(4096))]
struct PageTableLevel([Pte; 512]);

impl Pte {
    fn new(pa: *const Page, flags: PteFlags) -> Self {
        Self(pa2pte(pa) | (flags | PteFlags::V).bits())
    }

    fn flags(&self) -> PteFlags {
        PteFlags::from_bits_retain(self.0)
    }

    fn valid(&self) -> bool {
        self.flags().intersects(PteFlags::V)
    }
}

bitflags! {
    #[derive(Copy, Clone)]
    pub(super) struct PteFlags: usize {
        /// valid
        const V = 1;
        const R = 1 << 1;
        const W = 1 << 2;
        const X = 1 << 3;
        /// user can access
        const U = 1 << 4;

        // for reserved bits
        const _ = !0;
    }
}

impl PageTable {
    /// Create PTEs for virtual addresses starting at va that refer to
    /// physical addresses starting at pa.
    /// va and size MUST be page-aligned.
    /// Returns 0 on success, -1 if walk() couldn't
    /// allocate a needed page-table page.
    pub(super) fn insert(
        &mut self,
        va: Range<usize>,
        mut pa: *const Page,
        perm: PteFlags,
    ) -> Result<(), ()> {
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
                    .cast::<Pte>()
                    .as_mut()
                    .ok_or(())?
            };
            if pte.valid() {
                panic!("mappages: remap");
            }
            *pte = Pte::new(pa, perm);
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
    if src.len() >= PGSIZE {
        panic!("uvmfirst: more than a page");
    }
    unsafe {
        let mem = Box::leak(Box::<Page>::new_zeroed().assume_init());
        pagetable
            .insert(
                0..PGSIZE,
                mem,
                PteFlags::W | PteFlags::R | PteFlags::X | PteFlags::U,
            )
            .unwrap();
        mem.as_mut_ptr()
            .copy_from_nonoverlapping(src.as_ptr(), src.len());
    }
}
