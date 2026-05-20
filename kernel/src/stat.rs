#[allow(dead_code)]
#[repr(i16)]
pub(crate) enum InodeType {
    Unknown = 0,
    Dir = 1,
    File = 2,
    Device = 3,
}
