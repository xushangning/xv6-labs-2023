use core::ffi::CStr;

pub(crate) fn panic(s: &CStr) {
    unsafe {
        crate::sys::panic(s.as_ptr().cast_mut());
    }
}
