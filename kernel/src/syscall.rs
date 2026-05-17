use core::{
    ffi::{CStr, c_char, c_int},
    mem::MaybeUninit,
};

use crate::sys::myproc;

mod file;
mod proc;

/// Fetch the uint64 at addr from the current process.
fn fetchaddr(addr: usize) -> Result<usize, ()> {
    let mut i = MaybeUninit::<usize>::uninit();
    if unsafe { crate::sys::fetchaddr(addr.try_into().unwrap(), i.as_mut_ptr().cast()) } < 0 {
        Err(())
    } else {
        Ok(unsafe { i.assume_init() })
    }
}

/// Fetch the nul-terminated string at addr from the current process.
/// Returns length of string, not including nul, or -1 for error.
fn fetchstr(addr: usize, buf: &mut [MaybeUninit<c_char>]) -> Result<&CStr, ()> {
    let p = unsafe { myproc().as_mut().unwrap() };
    if unsafe {
        crate::sys::copyinstr(
            p.pagetable.as_mut().unwrap().as_mut(),
            buf.as_mut_ptr().cast(),
            addr.try_into().unwrap(),
            buf.len().try_into().unwrap(),
        )
    } < 0
    {
        Err(())
    } else {
        Ok(unsafe { CStr::from_ptr(buf.as_ptr().cast()) })
    }
}

/// Fetch the nth 32-bit system call argument.
unsafe fn argint(n: c_int) -> c_int {
    unsafe {
        let mut i = MaybeUninit::<c_int>::uninit();
        crate::sys::argint(n, i.as_mut_ptr());
        i.assume_init()
    }
}

/// Fetch the nth word-sized system call argument as a null-terminated string.
/// Copies into buf, at most max.
/// Returns string length if OK (including nul), -1 if error.
unsafe fn argstr(n: c_int, buf: &mut [MaybeUninit<c_char>]) -> c_int {
    unsafe { crate::sys::argstr(n, buf.as_mut_ptr().cast(), buf.len().try_into().unwrap()) }
}

// Prototypes for the functions that handle system calls.
unsafe extern "C" {
    fn sys_fstat() -> u64;
    fn sys_chdir() -> u64;
    fn sys_dup() -> u64;
    fn sys_mknod() -> u64;
    fn sys_unlink() -> u64;
    fn sys_link() -> u64;
    fn sys_mkdir() -> u64;
}

/// An array mapping syscall numbers from syscall.h
/// to the function that handles the system call.
static SYSCALLS: &[Option<unsafe extern "C" fn() -> u64>] = &[
    None,
    Some(proc::fork),
    Some(proc::exit),
    Some(proc::wait),
    Some(file::pipe),
    Some(file::read),
    Some(proc::kill),
    Some(file::exec),
    Some(sys_fstat),
    Some(sys_chdir),
    Some(sys_dup),
    Some(proc::getpid),
    Some(proc::sbrk),
    Some(proc::sleep),
    Some(proc::uptime),
    Some(file::open),
    Some(file::write),
    Some(sys_mknod),
    Some(sys_unlink),
    Some(sys_link),
    Some(sys_mkdir),
    Some(file::close),
];

pub(super) unsafe fn syscall() {
    let p = unsafe { myproc().as_mut().unwrap() };

    let num =
        usize::try_from(unsafe { p.trapframe.as_ref().unwrap().assume_init_ref().a7 }).unwrap();
    if let Some(f) = SYSCALLS.get(num).and_then(|e| e.as_ref()) {
        // Use num to lookup the system call function for num, call it,
        // and store its return value in p->trapframe->a0
        unsafe { p.trapframe.as_mut().unwrap().assume_init_mut().a0 = f() };
    } else {
        crate::println!(
            "{} {}: unknown sys call {num}",
            p.pid,
            unsafe { CStr::from_ptr(p.name.as_ptr()) }.to_str().unwrap(),
        );
        unsafe {
            p.trapframe.as_mut().unwrap().assume_init_mut().a0 = (-1i64).cast_unsigned();
        }
    }
}
