use core::ptr;

use riscv::register::satp::{self, Satp};

use crate::kalloc::Page;

pub(crate) fn make_satp(pagetable: usize) -> Satp {
    let mut ret = Satp::from_bits(0);
    ret.set_ppn(pagetable >> PGSHIFT);
    ret.set_mode(satp::Mode::Sv39);
    ret
}

pub(crate) mod intr {
    use riscv::register::sstatus;

    /// are device interrupts enabled?
    #[inline]
    pub(crate) fn get() -> bool {
        sstatus::read().sie()
    }
}

pub(crate) mod tp {
    #[inline]
    pub(crate) unsafe fn read() -> usize {
        let value: usize;
        unsafe {
            core::arch::asm!("mv {}, tp", out(reg) value, options(nomem, nostack, preserves_flags));
        }
        value
    }

    #[inline]
    pub(crate) unsafe fn write(value: usize) {
        unsafe {
            core::arch::asm!("mv tp, {}", in(reg) value, options(nomem, nostack, preserves_flags));
        }
    }
}

/// bytes per page
pub(crate) const PGSIZE: usize = 4096;
/// bits of offset within a page
pub(crate) const PGSHIFT: usize = 12;

pub(crate) const fn pgroundup(sz: usize) -> usize {
    (sz + PGSIZE - 1) & !(PGSIZE - 1)
}

/// shift a physical address to the right place for a PTE.
pub(crate) fn pa2pte(pa: *const Page) -> usize {
    (pa.expose_provenance() >> PGSHIFT) << 10
}

pub(crate) fn pte2pa(pte: usize) -> *const Page {
    ptr::with_exposed_provenance((pte >> 10) << PGSHIFT)
}

/// extract the three 9-bit page table indices from a virtual address.
pub(crate) const fn px(level: usize, va: usize) -> usize {
    const MASK: usize = 0x1FF;
    let shift = PGSHIFT + 9 * level;
    (va >> shift) & MASK
}

/// one beyond the highest possible virtual address.
/// MAXVA is actually one bit less than the max allowed by
/// Sv39, to avoid having to sign-extend virtual addresses
/// that have the high bit set.
pub(crate) const MAXVA: usize = 1 << (9 + 9 + 9 + PGSHIFT - 1);
