use core::ffi::{c_int, c_uint};

use riscv::{
    interrupt::{
        Trap,
        supervisor::{self as interrupt, Exception, Interrupt},
    },
    register::{
        satp, scause, sepc,
        sstatus::{self, SPP},
        stval,
        stvec::{self, Stvec},
    },
};

use crate::{
    println,
    proc::Condvar,
    riscv::intr,
    spinlock::Mutex,
    sys::{exit, killed, myproc, yield_},
    trampoline::trampoline,
};

pub(super) static TICKS: Mutex<Condvar<c_uint>> = Mutex::new(c"time", Condvar(0));

unsafe extern "C" {
    fn uservec();
    fn userret();

    /// in kernelvec.S, calls kerneltrap().
    fn kernelvec();
}

/// set up to take exceptions and traps while in the kernel.
pub(super) fn inithart() {
    unsafe {
        stvec::write(Stvec::new(
            (kernelvec as *const ()).addr(),
            stvec::TrapMode::Direct,
        ));
    }
}

/// handle an interrupt, exception, or system call from user space.
/// called from trampoline.S
#[unsafe(no_mangle)]
unsafe extern "C" fn usertrap() {
    let mut which_dev = 0;

    if sstatus::read().spp() != SPP::User {
        panic!("usertrap: not from user mode");
    }

    // send interrupts and exceptions to kerneltrap(),
    // since we're now in the kernel.
    unsafe {
        stvec::write(Stvec::new(
            (kernelvec as *const ()).addr(),
            stvec::TrapMode::Direct,
        ));
    }

    let p = unsafe { myproc().as_mut().unwrap() };

    unsafe {
        // save user program counter.
        p.trapframe.as_mut().unwrap().assume_init_mut().epc = sepc::read().try_into().unwrap();
    }

    if matches!(
        interrupt::try_cause::<Interrupt, Exception>(),
        Ok(Trap::Exception(Exception::UserEnvCall))
    ) {
        // system call

        unsafe {
            if killed(p) != 0 {
                exit(-1);
            }

            // sepc points to the ecall instruction,
            // but we want to return to the next instruction.
            p.trapframe.as_mut().unwrap().assume_init_mut().epc += 4;

            // an interrupt will change sepc, scause, and sstatus,
            // so enable only now that we're done with those registers.
            interrupt::enable();

            crate::syscall::syscall();
        }
    } else {
        which_dev = devintr();
        if which_dev != 0 {
            // ok
        } else {
            println!(
                "usertrap(): unexpected scause {:x} pid={}",
                scause::read().bits(),
                p.status.lock().pid,
            );
            println!(
                "            sepc={:x} stval={:x}",
                sepc::read(),
                stval::read(),
            );
            unsafe {
                crate::sys::setkilled(p);
            }
        }
    }

    unsafe {
        if killed(p) != 0 {
            exit(-1);
        }

        // give up the CPU if this is a timer interrupt.
        if which_dev == 2 {
            yield_();
        }

        usertrapret();
    }
}

/// return to user space
pub(super) fn usertrapret() -> ! {
    use crate::memlayout::TRAMPOLINE;

    let p = unsafe { myproc().as_mut().unwrap() };

    interrupt::disable();

    let trampoline_uservec =
        TRAMPOLINE + ((uservec as *const ()).addr() - (trampoline as *const ()).addr());
    unsafe {
        stvec::write(Stvec::new(trampoline_uservec, stvec::TrapMode::Direct));
    }

    let trapframe = unsafe { p.trapframe.as_mut().unwrap().assume_init_mut() };
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

    let satp =
        crate::riscv::make_satp(core::ptr::from_ref(p.pagetable.as_ref().unwrap().as_ref()).addr());

    let trampoline_userret =
        TRAMPOLINE + ((userret as *const ()).addr() - (trampoline as *const ()).addr());
    unsafe {
        core::mem::transmute::<_, unsafe extern "C" fn(usize) -> !>(trampoline_userret)(satp.bits())
    };
}

/// interrupts and exceptions from kernel code go here via kernelvec,
/// on whatever the current kernel stack is.
#[unsafe(no_mangle)]
unsafe extern "C" fn kerneltrap() {
    let sepc = sepc::read();
    let sstatus = sstatus::read();

    if sstatus.spp() != SPP::Supervisor {
        panic!("kerneltrap: not from supervisor mode");
    }
    if intr::get() {
        panic!("kerneltrap: interrupts enabled");
    }

    unsafe {
        let which_dev = devintr();
        if which_dev == 0 {
            println!("scause {:x}", scause::read().bits());
            println!("sepc={sepc:x} stval={:x}", stval::read());
            panic!("kerneltrap");
        } else if which_dev == 2
            && myproc()
                .as_ref()
                .is_some_and(|p| p.status.lock().state == crate::sys::procstate_RUNNING)
        {
            // give up the CPU if this is a timer interrupt.
            yield_();
        }

        // the yield() may have caused some traps to occur,
        // so restore trap registers for use by kernelvec.S's sepc instruction.
        sepc::write(sepc);
        sstatus::write(sstatus);
    }
}

fn clockintr() {
    let mut ticks = TICKS.lock();
    ticks.0 += 1;
    ticks.notify_all();
}

/// Check if it's an external interrupt or software interrupt,
/// and handle it.
/// returns 2 if timer interrupt,
/// 1 if other device,
/// 0 if not recognized.
fn devintr() -> c_int {
    use crate::{
        memlayout::{uart0, virtio0},
        sys::{plic_claim, plic_complete, virtio_disk_intr},
    };

    let Ok(Trap::Interrupt(scause)) = interrupt::try_cause::<Interrupt, Exception>() else {
        return 0;
    };

    match scause {
        Interrupt::SupervisorExternal => {
            // this is a supervisor external interrupt, via PLIC.

            unsafe {
                // irq indicates which device interrupted.
                let irq = plic_claim();

                match irq {
                    uart0::IRQ => crate::uart::intr(),
                    virtio0::IRQ => virtio_disk_intr(),
                    _ => {
                        if irq != 0 {
                            println!("unexpected interrupt irq={irq}")
                        }
                    }
                }

                // the PLIC allows each device to raise at most one
                // interrupt at a time; tell the PLIC the device is
                // now allowed to interrupt again.
                if irq != 0 {
                    plic_complete(irq);
                }
            }

            1
        }
        Interrupt::SupervisorSoft => {
            // software interrupt from a machine-mode timer interrupt,
            // forwarded by timervec in kernelvec.S.

            unsafe {
                if crate::proc::cpuid() == 0 {
                    clockintr();
                }

                // acknowledge the software interrupt by clearing
                // the SSIP bit in sip.
                riscv::register::sip::clear_ssoft();
            }

            2
        }
        _ => 0,
    }
}
