use riscv::register::{mhartid, mstatus};

use crate::param::NCPU;

#[repr(C, align(16))]
struct Stack([u8; 4096 * NCPU]);

#[unsafe(export_name = "stack0")]
static mut STACK0: Stack = Stack([0; _]);

static mut TIMER_SCRATCH: [[usize; 5]; NCPU] = [[0; _]; _];

/// entry.S jumps here in machine mode on stack0.
#[unsafe(no_mangle)]
unsafe extern "C" fn start() -> ! {
    use riscv::register::{
        medeleg::{self, Medeleg},
        mepc,
        mideleg::{self, Mideleg},
        mstatus::MPP,
        pmpaddr0, pmpcfg0,
        satp::{self, Satp},
        sie,
    };

    unsafe {
        // set M Previous Privilege mode to Supervisor, for mret.
        mstatus::set_mpp(MPP::Supervisor);

        // set M Exception Program Counter to main, for mret.
        // requires gcc -mcmodel=medany
        unsafe extern "C" {
            fn main();
        }
        mepc::write(main as usize);

        // disable paging for now.
        satp::write(Satp::from_bits(0));

        // delegate all interrupts and exceptions to supervisor mode.
        medeleg::write(Medeleg::from_bits(0xffff));
        mideleg::write(Mideleg::from_bits(0xffff));
        let mut sie_val = sie::read();
        sie_val.set_sext(true);
        sie_val.set_stimer(true);
        sie_val.set_ssoft(true);
        sie::write(sie_val);

        // configure Physical Memory Protection to give supervisor mode
        // access to all of physical memory.
        pmpaddr0::write(0x3f_ffff_ffff_ffff);
        pmpcfg0::write(0xf);

        // ask for clock interrupts.
        timerinit();

        // keep each CPU's hartid in its tp register, for cpuid().
        crate::riscv::tp::write(mhartid::read());

        // switch to supervisor mode and jump to main().
        core::arch::asm!("mret", options(noreturn));
    }
}

/// arrange to receive timer interrupts.
/// they will arrive in machine mode at
/// at timervec in kernelvec.S,
/// which turns them into software interrupts for
/// devintr() in trap.c.
fn timerinit() {
    use riscv::register::{
        mie, mscratch,
        mtvec::{self, Mtvec},
    };

    use crate::memlayout::clint;

    // each CPU has a separate source of timer interrupts.
    let id = mhartid::read();

    unsafe {
        // ask the CLINT for a timer interrupt.
        const INTERVAL: usize = 1_000_000; // cycles; about 1/10th second in qemu.
        let mtimecmp = clint::MTIMECMP.add(id);
        mtimecmp.write_volatile(clint::MTIME.read_volatile() + INTERVAL);

        // prepare information in scratch[] for timervec.
        // scratch[0..2] : space for timervec to save registers.
        // scratch[3] : address of CLINT MTIMECMP register.
        // scratch[4] : desired interval (in cycles) between timer interrupts.
        let scratch = &mut TIMER_SCRATCH[id];
        scratch[3] = mtimecmp.addr();
        scratch[4] = INTERVAL;
        mscratch::write(scratch.as_mut_ptr().addr());

        // set the machine-mode trap handler.
        unsafe extern "C" {
            fn timervec();
        }
        mtvec::write(Mtvec::from_bits(timervec as usize));

        // enable machine-mode interrupts.
        mstatus::set_mie();

        // enable machine-mode timer interrupts.
        mie::set_mtimer();
    }
}
