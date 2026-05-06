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

    /// enable device interrupts
    #[inline]
    pub(crate) unsafe fn on() {
        unsafe { sstatus::set_sie() }
    }

    /// disable device interrupts
    #[inline]
    pub(crate) unsafe fn off() {
        unsafe { sstatus::clear_sie() }
    }

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

/// shift a physical address to the right place for a PTE.
pub(crate) fn pa2pte(pa: *const Page) -> usize {
    (pa.expose_provenance() >> PGSHIFT) << 10
}

/// one beyond the highest possible virtual address.
/// MAXVA is actually one bit less than the max allowed by
/// Sv39, to avoid having to sign-extend virtual addresses
/// that have the high bit set.
pub(crate) const MAXVA: usize = 1 << (9 + 9 + 9 + PGSHIFT - 1);
