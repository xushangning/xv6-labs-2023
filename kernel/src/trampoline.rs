use crate::memlayout::TRAPFRAME;

unsafe extern "C" {
    pub(super) fn trampoline();
}

core::arch::global_asm!(include_str!("../trampoline.S"), TRAPFRAME = const TRAPFRAME);
