use core::ops::{Deref, DerefMut};

#[repr(C)]
pub(crate) struct RcInner<T: ?Sized> {
    strong: usize,
    value: T,
}

impl<T: ?Sized> RcInner<T> {
    pub(crate) fn strong(this: &Self) -> usize {
        this.strong
    }

    pub(crate) fn inc_strong(this: &mut Self) {
        this.strong += 1;
    }

    pub(crate) fn dec_strong(this: &mut Self) {
        this.strong -= 1;
    }
}

impl<T> RcInner<T> {
    pub(crate) const fn new(value: T) -> RcInner<T> {
        RcInner { strong: 0, value }
    }
}

impl<T: ?Sized> Deref for RcInner<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.value
    }
}

impl<T: ?Sized> DerefMut for RcInner<T> {
    fn deref_mut(&mut self) -> &mut T {
        &mut self.value
    }
}
