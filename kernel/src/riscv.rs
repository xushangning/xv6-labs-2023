pub(crate) mod tp {
    #[inline]
    pub(crate) unsafe fn write(value: usize) {
        unsafe {
            core::arch::asm!("mv tp, {}", in(reg) value, options(nomem, nostack, preserves_flags));
        }
    }
}
