//! Driver de Local APIC (LAPIC) para x86_64: temporizador periódico, IPIs e inicialização SMP (INIT-SIPI).

pub const LAPIC_BASE: u64 = 0xFEE0_0000;

pub const REG_ID: u32 = 0x0020;
pub const REG_VERSION: u32 = 0x0030;
pub const REG_TPR: u32 = 0x0080;
pub const REG_EOI: u32 = 0x00B0;
pub const REG_LDR: u32 = 0x00D0;
pub const REG_DFR: u32 = 0x00E0;
pub const REG_SVR: u32 = 0x00F0;
pub const REG_ESR: u32 = 0x0280;
pub const REG_ICR_LOW: u32 = 0x0300;
pub const REG_ICR_HIGH: u32 = 0x0310;
pub const REG_LVT_TIMER: u32 = 0x0320;
pub const REG_LVT_THERMAL: u32 = 0x0330;
pub const REG_LVT_PERF: u32 = 0x0340;
pub const REG_LVT_LINT0: u32 = 0x0350;
pub const REG_LVT_LINT1: u32 = 0x0360;
pub const REG_LVT_ERROR: u32 = 0x0370;
pub const REG_TIMER_INIT: u32 = 0x0380;
pub const REG_TIMER_CURR: u32 = 0x0390;
pub const REG_TIMER_DIV: u32 = 0x03E0;

#[inline]
pub unsafe fn lapic_read(reg: u32) -> u32 {
    let ptr = (LAPIC_BASE + reg as u64) as *const u32;
    unsafe { core::ptr::read_volatile(ptr) }
}

#[inline]
pub unsafe fn lapic_write(reg: u32, val: u32) {
    let ptr = (LAPIC_BASE + reg as u64) as *mut u32;
    unsafe { core::ptr::write_volatile(ptr, val); }
}

#[inline]
pub fn eoi() {
    unsafe {
        lapic_write(REG_EOI, 0);
    }
}

pub fn init_lapic() {
    unsafe {
        // Habilita APIC global no MSR IA32_APIC_BASE (0x1B)
        let base = super::trap::rdmsr(0x1B);
        super::trap::wrmsr(0x1B, (base & 0xFFFF_F000) | (1 << 11));

        // Spurious Interrupt Vector Register (0xF0): bit 8 habilita APIC, vetor 255 (0xFF)
        lapic_write(REG_SVR, 0x1FF);

        // Prioridade de tarefas (TPR = 0) para aceitar todas as interrupções
        lapic_write(REG_TPR, 0);

        // Mascara entradas LVT não utilizadas
        lapic_write(REG_LVT_THERMAL, 1 << 16);
        lapic_write(REG_LVT_PERF, 1 << 16);
        lapic_write(REG_LVT_LINT0, 1 << 16);
        lapic_write(REG_LVT_LINT1, 1 << 16);
        lapic_write(REG_LVT_ERROR, 1 << 16);

        // Limpa registro de erro (requer duas escritas consecutivas)
        lapic_write(REG_ESR, 0);
        lapic_write(REG_ESR, 0);

        // Envia EOI para limpar qualquer pendência
        lapic_write(REG_EOI, 0);
    }
}

static mut LAPIC_TICKS_PER_10MS: u32 = 625_000; // Fallback padrão QEMU (1 GHz / 16 / 100 Hz)

pub fn calibrate_timer() {
    unsafe {
        lapic_write(REG_TIMER_DIV, 0x03); // Divisor 16
        lapic_write(REG_LVT_TIMER, 1 << 16); // Mascarado, modo one-shot
        lapic_write(REG_TIMER_INIT, 0xFFFF_FFFF);

        // Aguarda 10 ms (20_000_000 ciclos de RDTSC a 2 GHz)
        let start = super::timer::now();
        let target = 20_000_000;
        while super::timer::now().wrapping_sub(start) < target {
            core::hint::spin_loop();
        }

        let remaining = lapic_read(REG_TIMER_CURR);
        let elapsed = 0xFFFF_FFFFu32.saturating_sub(remaining);
        if elapsed > 10_000 {
            LAPIC_TICKS_PER_10MS = elapsed;
        }
    }
}

pub fn init_lapic_timer() {
    unsafe {
        lapic_write(REG_TIMER_DIV, 0x03); // Divisor 16
        // Vetor 32, Modo Periódico (1 << 17 = 0x20000)
        lapic_write(REG_LVT_TIMER, 0x0002_0020);
        lapic_write(REG_TIMER_INIT, LAPIC_TICKS_PER_10MS);
    }
}

pub fn send_ipi(target_cpu: u32, vector: u8) {
    unsafe {
        lapic_write(REG_ICR_HIGH, target_cpu << 24);
        lapic_write(REG_ICR_LOW, (1 << 14) | (vector as u32));
        while (lapic_read(REG_ICR_LOW) & (1 << 12)) != 0 {
            core::hint::spin_loop();
        }
    }
}

unsafe extern "C" {
    static ap_trampoline_start: u8;
    static ap_trampoline_end: u8;
}

pub fn copy_trampoline() {
    let src = &raw const ap_trampoline_start as *const u8;
    let end = &raw const ap_trampoline_end as *const u8;
    let size = (end as usize).saturating_sub(src as usize);
    let dst = 0x8000 as *mut u8;
    unsafe {
        core::ptr::copy_nonoverlapping(src, dst, size);
    }
}

fn busy_wait_us(us: u64) {
    let cycles = us * 2000;
    let start = super::timer::now();
    while super::timer::now().wrapping_sub(start) < cycles {
        core::hint::spin_loop();
    }
}

pub fn start_ap(cpu: u32) -> bool {
    let mailbox_cpu = 0x7FF0 as *mut u32;
    let mailbox_ack = 0x7FF4 as *mut u32;

    unsafe {
        mailbox_cpu.write_volatile(cpu);
        mailbox_ack.write_volatile(0);
        super::mb();

        // 1. INIT IPI (Assert)
        lapic_write(REG_ICR_HIGH, cpu << 24);
        lapic_write(REG_ICR_LOW, 0x0000_4500); // Delivery INIT (0x500), Level Assert (1 << 14)
        while (lapic_read(REG_ICR_LOW) & (1 << 12)) != 0 {
            core::hint::spin_loop();
        }

        busy_wait_us(10_000); // 10 ms

        // INIT IPI (De-assert)
        lapic_write(REG_ICR_HIGH, cpu << 24);
        lapic_write(REG_ICR_LOW, 0x0000_0500); // Delivery INIT, Level De-assert (0)
        while (lapic_read(REG_ICR_LOW) & (1 << 12)) != 0 {
            core::hint::spin_loop();
        }

        busy_wait_us(1_000); // 1 ms

        // 2. Startup IPI (SIPI): vetor 0x08 (0x08 * 4096 = 0x8000)
        lapic_write(REG_ICR_HIGH, cpu << 24);
        lapic_write(REG_ICR_LOW, 0x0000_4608); // Delivery Startup (0x600), Level Assert (1 << 14), Vetor 0x08
        while (lapic_read(REG_ICR_LOW) & (1 << 12)) != 0 {
            core::hint::spin_loop();
        }

        // Aguarda resposta da CPU secundária (até 10 ms)
        for _ in 0..100 {
            if mailbox_ack.read_volatile() == 1 {
                return true;
            }
            busy_wait_us(100);
        }

        // Segundo SIPI conforme especificação MultiProcessor Intel
        lapic_write(REG_ICR_HIGH, cpu << 24);
        lapic_write(REG_ICR_LOW, 0x0000_4608);
        while (lapic_read(REG_ICR_LOW) & (1 << 12)) != 0 {
            core::hint::spin_loop();
        }

        for _ in 0..100 {
            if mailbox_ack.read_volatile() == 1 {
                return true;
            }
            busy_wait_us(100);
        }
    }

    false
}
