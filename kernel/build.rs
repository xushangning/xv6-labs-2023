fn main() {
    cc::Build::new()
        .files(&[
            // OBJS
            "kalloc.c",
            "string.c",
            "vm.c",
            "proc.c",
            "swtch.S",
            "trampoline.S",
            "trap.c",
            "syscall.c",
            "sysproc.c",
            "bio.c",
            "fs.c",
            "log.c",
            "sleeplock.c",
            "file.c",
            "pipe.c",
            "exec.c",
            "sysfile.c",
            "kernelvec.S",
            "plic.c",
            "virtio_disk.c",
            // OBJS_KCSAN
            "console.c",
            "printf.c",
            "uart.c",
            "spinlock.c",
        ])
        .flags(&[
            "-Wno-builtin-declaration-mismatch",
            // Force cc to compile for hard float support because this is the default for the Rust compiler
            // and cc by default only enables soft float:
            // https://github.com/rust-lang/cc-rs/blob/fbd480758b5f9a2c2d3261d76725b41e90e2ae2f/src/lib.rs#L2464-L2470
            // If this flag is not specified, rust-lld will report the error "cannot link object files with different floating-point ABI."
            "-mabi=lp64d",
            "-mcmodel=medany",
        ])
        .warnings(false)
        .compile(concat!(env!("CARGO_PKG_NAME"), "-cc"));

    println!("cargo::rustc-link-arg-bins=-Tkernel.ld")
}
