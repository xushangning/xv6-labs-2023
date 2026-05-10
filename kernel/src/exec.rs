use core::{
    ffi::{CStr, c_char},
    mem::{self, DropGuard, MaybeUninit},
    ops::Range,
};

use crate::{
    fs,
    log::OpGuard,
    proc::ProcVm,
    riscv::PGSIZE,
    sys::{inode, pagetable_t},
    vm::{PteFlags, Vm},
};

fn flags2perm(flags: u32) -> PteFlags {
    let mut perm = PteFlags::empty();
    if flags & 0x1 != 0 {
        perm |= PteFlags::X;
    }
    if flags & 0x2 != 0 {
        perm |= PteFlags::W;
    }
    perm
}

pub(super) fn exec(path: *const c_char, argv: &[*const c_char]) -> Result<usize, ()> {
    use goblin::elf64::{
        header::{ELFMAG, Header},
        program_header::{PT_LOAD, ProgramHeader},
    };

    use crate::{
        param::MAXARG,
        riscv::pgroundup,
        vm::{copyout, uvmalloc},
    };

    let p = unsafe { crate::sys::myproc().as_mut().unwrap() };

    let mut proc_vm = ProcVm::new(p)?;

    let elf_entry = {
        let _op_guard = OpGuard::new();

        let ip = unsafe { crate::sys::namei(path.cast_mut()) };
        if ip.is_null() {
            return Err(());
        }
        unsafe {
            crate::sys::ilock(ip);
        }
        let ip = DropGuard::new(ip, |ip| unsafe { crate::sys::iunlockput(ip) });

        // Check ELF header
        let mut elf = MaybeUninit::<Header>::uninit();
        if unsafe {
            fs::readi(
                *ip,
                false,
                elf.as_mut_ptr().addr(),
                0,
                mem::size_of::<Header>(),
            )
        }? != mem::size_of::<Header>()
        {
            return Err(());
        }
        let elf = unsafe { elf.assume_init_ref() };

        if &elf.e_ident[0..4] != ELFMAG {
            return Err(());
        }

        // Load program into memory.
        let mut off = usize::try_from(elf.e_phoff).unwrap();
        for _ in 0..elf.e_phnum {
            let mut ph = MaybeUninit::<ProgramHeader>::uninit();
            if unsafe {
                fs::readi(
                    *ip,
                    false,
                    ph.as_mut_ptr().addr(),
                    off.try_into().unwrap(),
                    mem::size_of::<ProgramHeader>(),
                )
            }? != mem::size_of::<ProgramHeader>()
            {
                return Err(());
            }
            let ph = unsafe { ph.assume_init_ref() };
            off += mem::size_of::<ProgramHeader>();
            if ph.p_type != PT_LOAD {
                continue;
            }
            if ph.p_memsz < ph.p_filesz {
                return Err(());
            }
            if ph.p_vaddr + ph.p_memsz < ph.p_vaddr {
                return Err(());
            }
            if ph.p_vaddr % PGSIZE as u64 != 0 {
                return Err(());
            }
            proc_vm.0.sz = unsafe {
                uvmalloc(
                    &mut proc_vm.0,
                    (ph.p_vaddr + ph.p_memsz).try_into().unwrap(),
                    flags2perm(ph.p_flags),
                )
            }?
            .get();
            let p_vaddr = usize::try_from(ph.p_vaddr).unwrap();
            let p_filesz = usize::try_from(ph.p_filesz).unwrap();
            unsafe {
                loadseg(
                    proc_vm.0.pagetable.as_mut(),
                    p_vaddr..p_vaddr + p_filesz,
                    *ip,
                    ph.p_offset.try_into().unwrap(),
                )
            }?;
        }

        elf.e_entry
    };

    let oldsz = p.sz;

    // Allocate two pages at the next page boundary.
    // Make the first inaccessible as a stack guard.
    // Use the second as the user stack.
    proc_vm.0.sz = pgroundup(proc_vm.0.sz);
    let newsz = proc_vm.0.sz + 2 * PGSIZE;
    proc_vm.0.sz = unsafe { uvmalloc(&mut proc_vm.0, newsz, PteFlags::W) }?.get();
    unsafe {
        crate::sys::uvmclear(
            proc_vm.0.pagetable.as_mut(),
            (proc_vm.0.sz - 2 * PGSIZE).try_into().unwrap(),
        );
    }
    let mut sp = proc_vm.0.sz;
    let stackbase = sp - PGSIZE;

    // Push argument strings, prepare rest of stack in ustack.
    let mut ustack = heapless::Vec::<usize, { MAXARG + 1 }>::new();
    if argv.len() >= MAXARG {
        return Err(());
    }
    for &arg in argv {
        let arg_bytes = unsafe { CStr::from_ptr(arg) }.to_bytes_with_nul();
        sp -= arg_bytes.len();
        sp -= sp % 16; // riscv sp must be 16-byte aligned
        if sp < stackbase {
            return Err(());
        }
        unsafe {
            copyout(
                proc_vm.0.pagetable.as_mut(),
                sp.try_into().unwrap(),
                arg_bytes,
            )?;
            ustack.push_unchecked(sp);
        }
    }
    unsafe {
        ustack.push_unchecked(0);
    }

    // push the array of argv[] pointers.
    sp -= ustack.len() * mem::size_of::<u64>();
    sp -= sp % 16;
    if sp < stackbase {
        return Err(());
    }
    unsafe {
        copyout(
            proc_vm.0.pagetable.as_mut(),
            sp,
            bytemuck::cast_slice(ustack.as_slice()),
        )?;
    }

    // arguments to user main(argc, argv)
    // argc is returned via the system call return
    // value, which goes in a0.
    unsafe {
        p.trapframe.as_mut().unwrap().assume_init_mut().a1 = sp.try_into().unwrap();
    }

    // Save program name for debugging.
    let mut s = path;
    let mut last = path;
    unsafe {
        while *s != 0 {
            if *s == b'/' {
                last = s.add(1);
            }
            s = s.add(1);
        }
    }
    unsafe {
        crate::sys::safestrcpy(p.name.as_mut_ptr(), last, p.name.len().try_into().unwrap());
    }

    // Commit to the user image.
    let oldpagetable = p.pagetable.take().unwrap();
    p.sz = proc_vm.0.sz.try_into().unwrap();
    p.pagetable = Some(proc_vm.leak());
    unsafe {
        p.trapframe.as_mut().unwrap().assume_init_mut().epc = elf_entry;
        p.trapframe.as_mut().unwrap().assume_init_mut().sp = sp.try_into().unwrap();
    }
    mem::drop(ProcVm(Vm {
        pagetable: oldpagetable,
        sz: oldsz.try_into().unwrap(),
    }));

    Ok(ustack.len() - 1)
}

/// Load a program segment into pagetable at virtual address va.
/// va must be page-aligned
/// and the pages from va to va+sz must already be mapped.
/// Returns 0 on success, -1 on failure.
unsafe fn loadseg(
    pagetable: pagetable_t,
    varange: Range<usize>,
    ip: *mut inode,
    mut offset: usize,
) -> Result<(), ()> {
    use crate::sys::walkaddr;

    let va_end = varange.end;
    for va in varange.step_by(PGSIZE) {
        let pa = unsafe { walkaddr(pagetable, va.try_into().unwrap()) };
        if pa == 0 {
            panic!("loadseg: address should exist");
        }
        let n = core::cmp::min(va_end - va, PGSIZE);
        if unsafe { fs::readi(ip, false, pa.try_into().unwrap(), offset, n) }? != n {
            return Err(());
        }
        offset += PGSIZE;
    }

    Ok(())
}
