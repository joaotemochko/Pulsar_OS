use crate::context::Context;
use crate::gic;
use crate::timer;
use crate::process;

#[unsafe(no_mangle)]
pub extern "C" fn rust_irq_handler(frame: *mut Context) {
    let irq = gic::ack();

    if irq == timer::TIMER_IRQ {
        timer::rearm(10);                       // proximo tick (10x por segundo)
        unsafe { process::schedule(frame); }    // PREEMPCAO: troca de processo no tick
    }

    gic::eoi(irq);
}