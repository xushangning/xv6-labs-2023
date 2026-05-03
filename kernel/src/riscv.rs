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

/// one beyond the highest possible virtual address.
/// MAXVA is actually one bit less than the max allowed by
/// Sv39, to avoid having to sign-extend virtual addresses
/// that have the high bit set.
pub(crate) const MAXVA: usize = 1 << (9 + 9 + 9 + PGSHIFT - 1);
