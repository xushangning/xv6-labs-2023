use core::ffi::c_int;

use riscv::register::{
    satp, scause, sepc,
    sstatus::{self, SPP},
    stval, stvec,
};

use crate::{
    riscv::intr,
    sys::{exit, killed, myproc, printf},
};

unsafe extern "C" {
    fn trampoline();
    fn uservec();
    fn userret();

    /// in kernelvec.S, calls kerneltrap().
    fn kernelvec();

    fn clockintr();
}

/// handle an interrupt, exception, or system call from user space.
/// called from trampoline.S
#[unsafe(no_mangle)]
unsafe extern "C" fn usertrap() {
    let mut which_dev = 0;

    if sstatus::read().spp() != SPP::User {
        unsafe { crate::sys::panic(c"usertrap: not from user mode".as_ptr().cast_mut()) };
    }

    // send interrupts and exceptions to kerneltrap(),
    // since we're now in the kernel.
    unsafe {
        stvec::write(stvec::Stvec::new(
            (kernelvec as *const ()).addr(),
            stvec::TrapMode::Direct,
        ));
    }

    let p = unsafe { myproc().as_mut().unwrap() };

    unsafe {
        // save user program counter.
        (*p.trapframe).epc = sepc::read().try_into().unwrap();

        if scause::read().bits() == 8 {
            // system call

            if killed(p) != 0 {
                exit(-1);
            }

            // sepc points to the ecall instruction,
            // but we want to return to the next instruction.
            (*p.trapframe).epc += 4;

            // an interrupt will change sepc, scause, and sstatus,
            // so enable only now that we're done with those registers.
            intr::on();

            crate::sys::syscall();
        } else {
            which_dev = devintr();
            if which_dev != 0 {
                // ok
            } else {
                printf(
                    c"usertrap(): unexpected scause %p pid=%d\n"
                        .as_ptr()
                        .cast_mut(),
                    scause::read().bits(),
                    p.pid,
                );
                printf(
                    c"            sepc=%p stval=%p\n".as_ptr().cast_mut(),
                    sepc::read(),
                    stval::read(),
                );
                crate::sys::setkilled(p);
            }
        }

        if killed(p) != 0 {
            exit(-1);
        }

        // give up the CPU if this is a timer interrupt.
        if which_dev == 2 {
            crate::sys::yield_();
        }

        usertrapret();
    }
}

/// return to user space
#[unsafe(no_mangle)]
unsafe extern "C" fn usertrapret() -> ! {
    use crate::memlayout::TRAMPOLINE;

    let p = unsafe { myproc().as_mut().unwrap() };

    unsafe {
        intr::off();
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

/// Check if it's an external interrupt or software interrupt, and handle it.
/// Returns 2 if timer interrupt, 1 if other device, 0 if not recognized.
fn devintr() -> c_int {
    use crate::memlayout::{UART0_IRQ, VIRTIO0_IRQ};
    use crate::sys::{plic_claim, plic_complete, uartintr, virtio_disk_intr};

    let scause = scause::read();

    if scause.is_interrupt() && scause.code() == 9 {
        // supervisor external interrupt via PLIC
        let irq = unsafe { plic_claim() };

        if irq == UART0_IRQ {
            unsafe { uartintr() };
        } else if irq == VIRTIO0_IRQ {
            unsafe { virtio_disk_intr() };
        } else if irq != 0 {
            unsafe { printf(c"unexpected interrupt irq=%d\n".as_ptr().cast_mut(), irq) };
        }

        if irq != 0 {
            unsafe { plic_complete(irq) };
        }

        1
    } else if scause.is_interrupt() && scause.code() == 1 {
        // software interrupt from machine-mode timer, forwarded by timervec in kernelvec.S
        if unsafe { crate::sys::cpuid() } == 0 {
            unsafe { clockintr() };
        }

        // acknowledge by clearing SSIP bit in sip
        unsafe { crate::riscv::sip::clear_ssip() };

        2
    } else {
        0
    }
}
