use core::ptr::{read_volatile, write_volatile};

const GICD_BASE: usize = 0x0800_0000;
const GICC_BASE: usize = 0x0801_0000;

// Registradores do distribuidor (GICD)
const GICD_CTLR:      *mut u32 = (GICD_BASE + 0x000) as *mut u32;
const GICD_ISENABLER: *mut u32 = (GICD_BASE + 0x100) as *mut u32; // enable set
const GICD_IPRIORITYR:*mut u32 = (GICD_BASE + 0x400) as *mut u32;
const GICD_ITARGETSR: *mut u32 = (GICD_BASE + 0x800) as *mut u32;

// Registradores da interface de CPU (GICC)
const GICC_CTLR: *mut u32 = (GICC_BASE + 0x000) as *mut u32;
const GICC_PMR:  *mut u32 = (GICC_BASE + 0x004) as *mut u32; // priority mask
const GICC_IAR:  *mut u32 = (GICC_BASE + 0x00C) as *mut u32; // ack
const GICC_EOIR: *mut u32 = (GICC_BASE + 0x010) as *mut u32; // end of interrupt

pub fn init() {
    unsafe {
        // Habilita o distribuidor
        write_volatile(GICD_CTLR, 1);
        // Habilita a interface de CPU
        write_volatile(GICC_CTLR, 1);
        // Mascara de prioridade: aceita todas (0xFF = menor prioridade permitida)
        write_volatile(GICC_PMR, 0xFF);
    }
}

/// Habilita um IRQ especifico no distribuidor.
pub fn enable_irq(irq: u32) {
    unsafe {
        let reg = GICD_ISENABLER.add((irq / 32) as usize);
        write_volatile(reg, 1 << (irq % 32));

        // prioridade do IRQ (byte por IRQ); 0 = maior prioridade
        let prio = GICD_IPRIORITYR.cast::<u8>().add(irq as usize);
        write_volatile(prio, 0x00);

        // alvo: core 0 (byte por IRQ)
        let target = GICD_ITARGETSR.cast::<u8>().add(irq as usize);
        write_volatile(target, 0x01);
    }
}

/// Le o numero do IRQ pendente (acknowledge).
pub fn ack() -> u32 {
    unsafe { read_volatile(GICC_IAR) & 0x3FF }
}

/// Sinaliza fim de tratamento do IRQ.
pub fn eoi(irq: u32) {
    unsafe { write_volatile(GICC_EOIR, irq); }
}