use core::{
    cell::UnsafeCell,
    ffi::CStr,
    ops::{Deref, DerefMut},
    ptr,
};

use crate::sys::spinlock;

/// Mutual exclusion spin locks.
#[repr(C)]
pub(super) struct Mutex<T: ?Sized> {
    inner: UnsafeCell<spinlock>,
    data: UnsafeCell<T>,
}

pub(super) struct MutexGuard<'a, T: ?Sized + 'a> {
    pub(super) lock: &'a Mutex<T>,
}

unsafe impl<T: ?Sized + Send> Sync for Mutex<T> {}

impl<T> Mutex<T> {
    pub(super) const fn new(name: &'static CStr, t: T) -> Self {
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
    pub(super) fn lock(&self) -> MutexGuard<'_, T> {
        unsafe {
            crate::sys::acquire(self.inner.get());
        }
        MutexGuard { lock: self }
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
