use core::{
    mem::ManuallyDrop,
    ops::{Deref, DerefMut},
    ptr::NonNull,
};

#[repr(C)]
pub(crate) struct RcInner<T: ?Sized> {
    strong: usize,
    value: T,
}

impl<T: ?Sized> RcInner<T> {
    pub(crate) fn strong(&self) -> usize {
        self.strong
    }

    fn inc_strong(&mut self) {
        self.strong += 1;
    }

    fn dec_strong(&mut self) {
        self.strong -= 1;
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

#[repr(transparent)]
pub(crate) struct Rc<T: ?Sized>(NonNull<RcInner<T>>);

impl<T: ?Sized> Rc<T> {
    fn inner(&self) -> &RcInner<T> {
        unsafe { self.0.as_ref() }
    }

    fn inner_mut(&mut self) -> &mut RcInner<T> {
        unsafe { self.0.as_mut() }
    }

    pub(crate) fn strong_count(this: &Self) -> usize {
        this.inner().strong()
    }

    pub(crate) fn get_mut(this: &mut Self) -> Option<&mut T> {
        if Self::strong_count(this) == 1 {
            Some(this.inner_mut())
        } else {
            None
        }
    }
}

impl<T: ?Sized> Clone for Rc<T> {
    fn clone(&self) -> Self {
        let mut ret = Rc(self.0);
        ret.inner_mut().inc_strong();
        ret
    }
}

impl<T: ?Sized> Deref for Rc<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.inner()
    }
}

impl<T: ?Sized> Drop for Rc<T> {
    fn drop(&mut self) {
        self.inner_mut().dec_strong();
        if self.inner().strong() == 0 {
            unsafe { self.0.drop_in_place() }
        }
    }
}

#[repr(transparent)]
pub(crate) struct UniqueRc<T: ?Sized>(NonNull<RcInner<T>>);

impl<T: ?Sized> UniqueRc<T> {
    fn inner(&self) -> &RcInner<T> {
        unsafe { self.0.as_ref() }
    }

    fn inner_mut(&mut self) -> &mut RcInner<T> {
        unsafe { self.0.as_mut() }
    }

    pub(crate) fn new(inner: &mut RcInner<T>) -> Self {
        inner.strong = 1;
        UniqueRc(NonNull::from(inner))
    }

    pub(crate) fn into_rc(this: Self) -> Rc<T> {
        Rc(ManuallyDrop::new(this).0)
    }
}

impl<T: ?Sized> Deref for UniqueRc<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.inner()
    }
}

impl<T: ?Sized> DerefMut for UniqueRc<T> {
    fn deref_mut(&mut self) -> &mut T {
        self.inner_mut()
    }
}

impl<T: ?Sized> Drop for UniqueRc<T> {
    fn drop(&mut self) {
        self.inner_mut().dec_strong();
        unsafe {
            self.0.drop_in_place();
        }
    }
}
