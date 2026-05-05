#![no_std]
#![no_main]

use core::panic::PanicInfo;

use kernel::Allocator;

core::arch::global_asm!(include_str!("../entry.S"));

#[panic_handler]
fn panic(panic_info: &PanicInfo) -> ! {
    kernel::panic(panic_info);
}

#[global_allocator]
static ALLOCATOR: Allocator = Allocator;
