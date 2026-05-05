use core::ffi::{c_int, c_uint};

use riscv::register::{
    satp, scause, sepc,
    sstatus::{self, SPP},
    stval, stvec,
};

use crate::{
    println,
    riscv::intr,
    spinlock::Mutex,
    sys::{exit, killed, myproc, yield_},
};

pub(super) static TICKS: Mutex<c_uint> = Mutex::new(c"time", 0);

unsafe extern "C" {
    fn trampoline();
    fn uservec();
    fn userret();

    /// in kernelvec.S, calls kerneltrap().
    fn kernelvec();
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

            crate::syscall::syscall();
        } else {
            which_dev = devintr();
            if which_dev != 0 {
                // ok
            } else {
                println!(
                    "usertrap(): unexpected scause {:x} pid={}",
                    scause::read().bits(),
                    p.pid,
                );
                println!(
                    "            sepc={:x} stval={:x}",
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
            yield_();
        }

        usertrapret();
    }
}

/// return to user space
pub(super) fn usertrapret() -> ! {
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
                .is_some_and(|p| p.state == crate::sys::procstate_RUNNING)
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
    *ticks += 1;
    unsafe {
        crate::sys::wakeup((&raw const TICKS).cast_mut().cast());
    }
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

    let scause = scause::read();

    if scause.is_interrupt() && scause.bits() & 0xff == 9 {
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
    } else if scause.is_interrupt() && scause.code() == 1 {
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
    } else {
        0
    }
}
