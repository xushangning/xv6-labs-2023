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
