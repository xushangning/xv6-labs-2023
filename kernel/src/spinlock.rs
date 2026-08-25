use core::{
    cell::UnsafeCell,
    ffi::CStr,
    ops::{Deref, DerefMut},
    ptr,
};

use crate::sys::spinlock;

/// Mutual exclusion spin locks.
#[repr(C)]
pub(crate) struct Mutex<T: ?Sized> {
    inner: UnsafeCell<spinlock>,
    data: UnsafeCell<T>,
}

pub(crate) struct MutexGuard<'a, T: ?Sized + 'a> {
    pub(crate) lock: &'a Mutex<T>,
}

unsafe impl<T: ?Sized + Send> Sync for Mutex<T> {}

impl<T> Mutex<T> {
    pub(crate) const fn new(name: &'static CStr, t: T) -> Self {
        Self {
            inner: UnsafeCell::new(spinlock {
                name: name.as_ptr().cast_mut(),
                locked: 0,
                cpu: ptr::null_mut(),
            }),
            data: UnsafeCell::new(t),
        }
    }
}

impl<T: ?Sized> Mutex<T> {
    pub(crate) fn lock(&self) -> MutexGuard<'_, T> {
        unsafe {
            crate::sys::acquire(self.inner.get());
        }
        MutexGuard { lock: self }
    }

    /// Raw pointer to the underlying spinlock, for code that must release a
    /// lock acquired on its behalf by C code, without ever holding a
    /// `MutexGuard` for it.
    pub(crate) fn raw(&self) -> *mut spinlock {
        self.inner.get()
    }
}

impl<T: ?Sized> Drop for MutexGuard<'_, T> {
    #[inline]
    fn drop(&mut self) {
        unsafe {
            crate::sys::release(self.lock.inner.get());
        }
    }
}

impl<T: ?Sized> Deref for MutexGuard<'_, T> {
    type Target = T;

    fn deref(&self) -> &T {
        unsafe { &*self.lock.data.get() }
    }
}

impl<T: ?Sized> DerefMut for MutexGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        unsafe { &mut *self.lock.data.get() }
    }
}
