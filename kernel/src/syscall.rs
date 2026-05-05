use core::ffi::CStr;

use crate::sys::myproc;

mod proc;

// Prototypes for the functions that handle system calls.
unsafe extern "C" {
    fn sys_pipe() -> u64;
    fn sys_read() -> u64;
    fn sys_exec() -> u64;
    fn sys_fstat() -> u64;
    fn sys_chdir() -> u64;
    fn sys_dup() -> u64;
    fn sys_open() -> u64;
    fn sys_write() -> u64;
    fn sys_mknod() -> u64;
    fn sys_unlink() -> u64;
    fn sys_link() -> u64;
    fn sys_mkdir() -> u64;
    fn sys_close() -> u64;
}

/// An array mapping syscall numbers from syscall.h
/// to the function that handles the system call.
static SYSCALLS: &[Option<unsafe extern "C" fn() -> u64>] = &[
    None,
    Some(proc::fork),
    Some(proc::exit),
    Some(proc::wait),
    Some(sys_pipe),
    Some(sys_read),
    Some(proc::kill),
    Some(sys_exec),
    Some(sys_fstat),
    Some(sys_chdir),
    Some(sys_dup),
    Some(proc::getpid),
    Some(proc::sbrk),
    Some(proc::sleep),
    Some(proc::uptime),
    Some(sys_open),
    Some(sys_write),
    Some(sys_mknod),
    Some(sys_unlink),
    Some(sys_link),
    Some(sys_mkdir),
    Some(sys_close),
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
