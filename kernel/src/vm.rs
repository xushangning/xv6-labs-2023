use alloc::{alloc::AllocError, boxed::Box};
use core::{
    mem::{self, DropGuard},
    ops::{Deref, DerefMut, Range},
    ptr,
};

use bitflags::bitflags;

use crate::{
    kalloc::Page,
    riscv::{PGSIZE, pa2pte, pte2pa},
};

#[repr(transparent)]
struct Pte(usize);

#[repr(transparent)]
pub struct PageTable(PageTableLevel);

#[repr(C, align(4096))]
struct PageTableLevel([Pte; 512]);

#[repr(C)]
pub(super) struct Vm {
    pub(super) pagetable: Box<PageTable>,
    pub(super) sz: usize,
}

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

    fn pa(&self) -> *const Page {
        pte2pa(self.0)
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
    /// Return the address of the PTE in page table pagetable
    /// that corresponds to virtual address va.  If insert!=0,
    /// create any required page-table pages.
    ///
    /// The risc-v Sv39 scheme has three levels of page-table
    /// pages. A page-table page contains 512 64-bit PTEs.
    /// A 64-bit virtual address is split into five fields:
    ///   39..63 -- must be zero.
    ///   30..38 -- 9 bits of level-2 index.
    ///   21..29 -- 9 bits of level-1 index.
    ///   12..20 -- 9 bits of level-0 index.
    ///    0..11 -- 12 bits of byte offset within the page.
    fn get_or_choose_insert(&mut self, va: usize, insert: bool) -> Option<&mut Pte> {
        use crate::riscv::{MAXVA, px};

        if va >= MAXVA {
            panic!("walk");
        }

        let mut pagetable = &mut self.0;
        for level in (1..=2).rev() {
            let pte = &mut pagetable[px(level, va)];
            if pte.valid() {
                pagetable = unsafe {
                    pte.pa()
                        .cast::<PageTableLevel>()
                        .cast_mut()
                        .as_mut()
                        .unwrap()
                };
            } else {
                if !insert {
                    return None;
                }
                pagetable = Box::leak(unsafe { Box::try_new_zeroed().ok()?.assume_init() });
                *pte = Pte::new(ptr::from_mut(pagetable).cast(), PteFlags::empty());
            }
        }
        Some(&mut pagetable[px(0, va)])
    }

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

        for a in va.step_by(PGSIZE) {
            let pte = self.get_or_choose_insert(a, true).ok_or(())?;
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

    /// Remove npages of mappings starting from va. va must be
    /// page-aligned. The mappings must exist.
    /// Optionally free the physical memory.
    pub(super) fn remove(&mut self, va: Range<usize>, do_free: bool) {
        if va.start % PGSIZE != 0 {
            panic!("uvmunmap: not aligned");
        }

        for a in va.step_by(PGSIZE) {
            let Some(pte) = self.get_or_choose_insert(a, false) else {
                panic!("uvmunmap: walk")
            };
            if !pte.valid() {
                panic!("uvmunmap: not mapped");
            }
            if PteFlags::V.contains(pte.flags()) {
                panic!("uvmunmap: not a leaf");
            }
            if do_free {
                unsafe {
                    mem::drop(Box::from_raw(pte.pa().cast::<Page>().cast_mut()));
                }
            }
            *pte = Pte(0);
        }
    }
}

pub(super) fn uvmcreate() -> Result<Box<PageTable>, AllocError> {
    Box::try_new_zeroed().map(|b| unsafe { b.assume_init() })
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

#[repr(transparent)]
pub(super) struct ProcVm(pub(super) Vm);

impl ProcVm {
    /// Allocate PTEs and physical memory to grow process from oldsz to
    /// newsz, which need not be page aligned.  Returns new size or 0 on error.
    pub(super) fn extend_with(&mut self, newsz: usize, xperm: PteFlags) -> Result<(), ()> {
        use crate::riscv::pgroundup;

        if newsz < self.0.sz {
            return Ok(());
        }

        let oldsz = pgroundup(self.0.sz);
        for a in (oldsz..newsz).step_by(PGSIZE) {
            let mut this = DropGuard::new(&mut *self, |this| this.truncate(oldsz));
            let mem = Box::<Page>::try_new_zeroed().map_err(|_| ())?;
            this.0.pagetable.insert(
                a..a + PGSIZE,
                mem.as_ptr(),
                PteFlags::R | PteFlags::U | xperm,
            )?;
            this.0.sz = a;
            _ = Box::leak(mem);
            DropGuard::dismiss(this);
        }
        self.0.sz = newsz;
        Ok(())
    }

    /// Deallocate user pages to bring the process size from oldsz to
    /// newsz.  oldsz and newsz need not be page-aligned, nor does newsz
    /// need to be less than oldsz.  oldsz can be larger than the actual
    /// process size.  Returns the new process size.
    pub(super) fn truncate(&mut self, newsz: usize) {
        use crate::riscv::pgroundup;

        if newsz >= self.0.sz {
            return;
        }

        self.0.pagetable.remove(pgroundup(newsz)..self.0.sz, true);
        self.0.sz = newsz;
    }

    pub(super) fn resize(&mut self, newsz: usize, xperm: PteFlags) -> Result<(), ()> {
        if newsz > self.0.sz {
            self.extend_with(newsz, xperm)?;
        } else if newsz < self.0.sz {
            self.truncate(newsz);
        }
        Ok(())
    }
}

impl Drop for PageTableLevel {
    /// Recursively free page-table pages.
    /// All leaf mappings must already have been removed.
    fn drop(&mut self) {
        // there are 2^9 = 512 PTEs in a page table.
        for pte in &mut self.0 {
            if pte.valid() {
                if pte
                    .flags()
                    .intersects(PteFlags::R | PteFlags::W | PteFlags::X)
                {
                    panic!("freewalk: leaf");
                }
                // this PTE points to a lower-level page table.
                mem::drop(unsafe { Box::from_raw(pte.pa().cast::<PageTableLevel>().cast_mut()) });
            }
        }
    }
}

impl Vm {
    pub(super) fn new(pagetable: Box<PageTable>) -> Self {
        Self { pagetable, sz: 0 }
    }
}

impl Drop for Vm {
    /// Free user memory pages,
    /// then free page-table pages.
    fn drop(&mut self) {
        if self.sz > 0 {
            self.pagetable.remove(0..self.sz, true);
        }
    }
}

/// Copy from kernel to user.
/// Copy len bytes from src to virtual address dstva in a given page table.
/// Return 0 on success, -1 on error.
pub(super) unsafe fn copyout(
    pagetable: &mut PageTable,
    dstva: usize,
    src: &[u8],
) -> Result<(), ()> {
    let ret = unsafe {
        crate::sys::copyout(
            pagetable,
            dstva.try_into().unwrap(),
            src.as_ptr().cast_mut(),
            src.len().try_into().unwrap(),
        )
    };
    if ret >= 0 { Ok(()) } else { Err(()) }
}

impl Deref for PageTableLevel {
    type Target = [Pte; 512];

    fn deref(&self) -> &[Pte; 512] {
        &self.0
    }
}

impl DerefMut for PageTableLevel {
    fn deref_mut(&mut self) -> &mut [Pte; 512] {
        &mut self.0
    }
}
