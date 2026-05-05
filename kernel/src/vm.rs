use crate::{riscv::PGSIZE, sys::pagetable_t};

/// Load the user initcode into address 0 of pagetable,
/// for the very first process.
/// src must be less than a page.
pub(super) fn uvmfirst(pagetable: pagetable_t, src: &[u8]) {
    use crate::sys::{PTE_R, PTE_U, PTE_W, PTE_X};

    if src.len() >= PGSIZE {
        panic!("uvmfirst: more than a page");
    }
    unsafe {
        let mem = crate::sys::kalloc().cast::<u8>();
        mem.write_bytes(0, PGSIZE);
        crate::sys::mappages(
            pagetable,
            0,
            PGSIZE.try_into().unwrap(),
            mem.addr().try_into().unwrap(),
            (PTE_W | PTE_R | PTE_X | PTE_U).cast_signed(),
        );
        mem.copy_from_nonoverlapping(src.as_ptr(), src.len());
    }
}
