pub(super) unsafe extern "C" fn fork() -> u64 {
    crate::proc::fork().cast_unsigned().into()
}
