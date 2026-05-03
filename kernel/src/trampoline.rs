use crate::memlayout::TRAPFRAME;

core::arch::global_asm!(include_str!("../trampoline.S"), TRAPFRAME = const TRAPFRAME);
