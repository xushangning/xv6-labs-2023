use bitflags::bitflags;

bitflags! {
    #[derive(PartialEq)]
    pub(crate) struct OMode: i32 {
        const WRONLY = 0x001;
        const RDWR   = 0x002;
        const CREATE = 0x200;
        const TRUNC  = 0x400;
    }
}

impl OMode {
    pub(crate) const RDONLY: Self = Self::empty();
}
