#![no_std]
#![no_main]

core::arch::global_asm!(include_str!("../entry.S"));

#[panic_handler]
fn panic(_panic: &core::panic::PanicInfo<'_>) -> ! {
    loop {}
}
