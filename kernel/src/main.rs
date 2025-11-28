#![no_std]
#![no_main]

// Force linking the library.
use kernel as _;

core::arch::global_asm!(include_str!("../entry.S"));

#[panic_handler]
fn panic(_panic: &core::panic::PanicInfo<'_>) -> ! {
    loop {}
}
