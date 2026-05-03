unsafe extern "C" {
    static mut uart_tx_lock: crate::sys::spinlock;

    fn uartstart();
}

/// handle a uart interrupt, raised because input has
/// arrived, or the uart is ready for more output, or
/// both. called from devintr().
pub(crate) fn intr() {
    // read and process incoming characters.
    loop {
        let c = unsafe { crate::sys::uartgetc() };
        if c == -1 {
            break;
        }
        crate::console::intr(c.try_into().unwrap());
    }

    // send buffered characters.
    unsafe {
        crate::sys::acquire(&raw mut uart_tx_lock);
        uartstart();
        crate::sys::release(&raw mut uart_tx_lock);
    }
}
