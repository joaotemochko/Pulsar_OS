use crate::context::Context;
use crate::gic;
use crate::timer;
use crate::process;

#[unsafe(no_mangle)]
pub extern "C" fn rust_irq_handler(frame: *mut Context) {
    let irq = gic::ack();

    if irq == timer::TIMER_IRQ {
        timer::rearm(100);                      // proximo tick (100 Hz)
        timer::tick();                          // conta uptime
        unsafe { process::preempt(frame); }     // PREEMPCAO: troca de processo no tick
    }

    gic::eoi(irq);
}