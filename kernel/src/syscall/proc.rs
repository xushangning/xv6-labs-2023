pub(super) unsafe extern "C" fn fork() -> u64 {
    crate::proc::fork().cast_unsigned().into()
}

pub(super) unsafe extern "C" fn wait() -> u64 {
    let mut p: u64 = 0;
    unsafe { crate::sys::argaddr(0, &mut p) };
    crate::proc::wait(p).cast_unsigned().into()
}
