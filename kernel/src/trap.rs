use riscv::register::{
    satp, sepc,
    sstatus::{self, SPP},
    stvec,
};

unsafe extern "C" {
    fn trampoline();
    fn uservec();
    fn userret();
    fn usertrap();
}

/// return to user space
#[unsafe(no_mangle)]
unsafe extern "C" fn usertrapret() -> ! {
    use crate::memlayout::TRAMPOLINE;

    let p = unsafe { crate::sys::myproc().as_mut().unwrap() };

    unsafe {
        crate::riscv::intr::off();
    }

    let trampoline_uservec =
        TRAMPOLINE + ((uservec as *const ()).addr() - (trampoline as *const ()).addr());
    unsafe {
        stvec::write(stvec::Stvec::new(
            trampoline_uservec,
            stvec::TrapMode::Direct,
        ));
    }

    let trapframe = unsafe { &mut *p.trapframe };
    trapframe.kernel_satp = satp::read().bits().try_into().unwrap();
    trapframe.kernel_sp = p.kstack + u64::try_from(crate::riscv::PGSIZE).unwrap();
    trapframe.kernel_trap = (usertrap as *const ()).addr().try_into().unwrap();
    trapframe.kernel_hartid = unsafe { crate::riscv::tp::read().try_into().unwrap() };

    unsafe {
        let mut x = sstatus::read();
        x.set_spp(SPP::User);
        x.set_spie(true);
        sstatus::write(x);
    }

    unsafe { sepc::write(trapframe.epc.try_into().unwrap()) };

    let satp = crate::riscv::make_satp(p.pagetable.addr());

    let trampoline_userret =
        TRAMPOLINE + ((userret as *const ()).addr() - (trampoline as *const ()).addr());
    unsafe {
        core::mem::transmute::<_, unsafe extern "C" fn(usize) -> !>(trampoline_userret)(satp.bits())
    };
}
