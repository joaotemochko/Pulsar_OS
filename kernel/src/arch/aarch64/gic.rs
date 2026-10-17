//! GICv2: distribuidor compartilhado + interface por CPU.
//!
//! Registradores de SGI/PPI (ISENABLER0, IPRIORITYR0..7) e a interface de
//! CPU (GICC) sao "banked": cada nucleo enxerga a sua copia no mesmo
//! endereco, por isso `init_cpu` roda em todas as CPUs.
use core::ptr::{read_volatile, write_volatile};
use super::platform::{GICC_BASE, GICD_BASE};

const GICD_CTLR: u64 = 0x000;
const GICD_ISENABLER: u64 = 0x100;
const GICD_IPRIORITYR: u64 = 0x400;
const GICD_SGIR: u64 = 0xF00;

const GICC_CTLR: u64 = 0x000;
const GICC_PMR: u64 = 0x004;
const GICC_IAR: u64 = 0x00C;
const GICC_EOIR: u64 = 0x010;

/// SGI usada como IPI de reescalonamento.
pub const IPI_SGI: u32 = 1;

#[inline]
fn w32(addr: u64, v: u32) { unsafe { write_volatile(addr as *mut u32, v) } }
#[inline]
fn r32(addr: u64) -> u32 { unsafe { read_volatile(addr as *const u32) } }

pub fn init_distributor() {
    w32(GICD_BASE + GICD_CTLR, 1);
}

/// Interface de CPU + habilita timer (PPI 30) e a SGI de IPI neste nucleo.
pub fn init_cpu() {
    w32(GICC_BASE + GICC_CTLR, 1);
    w32(GICC_BASE + GICC_PMR, 0xFF);
    for irq in [super::timer::TIMER_IRQ, IPI_SGI] {
        w32(GICD_BASE + GICD_ISENABLER + (irq as u64 / 32) * 4, 1 << (irq % 32));
        unsafe { write_volatile((GICD_BASE + GICD_IPRIORITYR + irq as u64) as *mut u8, 0x00) };
    }
}

/// Le o IAR (acknowledge). Retorna o valor bruto (inclui a CPU de origem
/// das SGIs, necessaria no EOI).
pub fn ack() -> u32 {
    r32(GICC_BASE + GICC_IAR)
}

pub fn eoi(iar: u32) {
    w32(GICC_BASE + GICC_EOIR, iar);
}

/// Envia a SGI `id` para a CPU `cpu` (alvo por lista de CPUs).
pub fn send_sgi(cpu: usize, id: u32) {
    unsafe { core::arch::asm!("dsb ishst", options(nostack, preserves_flags)) };
    w32(GICD_BASE + GICD_SGIR, ((1u32 << cpu) << 16) | id);
}
