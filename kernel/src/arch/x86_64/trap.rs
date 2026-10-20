//! Configuração de IDT, PIC e tratamento de interrupções/exceções/syscalls para x86_64.

use core::arch::asm;
use super::context::Context;
use super::uart::outb;
use crate::vm::Access;

#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct IdtEntry {
    offset_low: u16,
    selector: u16,
    ist: u8,
    type_attr: u8,
    offset_mid: u16,
    offset_high: u32,
    zero: u32,
}

impl IdtEntry {
    pub const fn zero() -> Self {
        IdtEntry {
            offset_low: 0,
            selector: 0,
            ist: 0,
            type_attr: 0,
            offset_mid: 0,
            offset_high: 0,
            zero: 0,
        }
    }

    pub fn set_handler(&mut self, handler: u64, dpl: u8) {
        self.offset_low = (handler & 0xFFFF) as u16;
        self.selector = 0x08; // Kernel CS
        self.ist = 0;
        self.type_attr = 0x80 | ((dpl & 3) << 5) | 0x0E; // 64-bit Interrupt Gate
        self.offset_mid = ((handler >> 16) & 0xFFFF) as u16;
        self.offset_high = (handler >> 32) as u32;
        self.zero = 0;
    }
}

#[repr(C, packed)]
struct IdtPtr {
    limit: u16,
    base: u64,
}

static mut IDT: [IdtEntry; 256] = [IdtEntry::zero(); 256];

unsafe extern "C" {
    fn trap_entry_0();
    fn trap_entry_1();
    fn trap_entry_2();
    fn trap_entry_3();
    fn trap_entry_4();
    fn trap_entry_5();
    fn trap_entry_6();
    fn trap_entry_7();
    fn trap_entry_8();
    fn trap_entry_9();
    fn trap_entry_10();
    fn trap_entry_11();
    fn trap_entry_12();
    fn trap_entry_13();
    fn trap_entry_14();
    fn trap_entry_15();
    fn trap_entry_16();
    fn trap_entry_17();
    fn trap_entry_18();
    fn trap_entry_19();
    fn trap_entry_20();
    fn trap_entry_21();
    fn trap_entry_22();
    fn trap_entry_23();
    fn trap_entry_24();
    fn trap_entry_25();
    fn trap_entry_26();
    fn trap_entry_27();
    fn trap_entry_28();
    fn trap_entry_29();
    fn trap_entry_30();
    fn trap_entry_31();
    fn trap_entry_32();
    fn trap_entry_33();
    fn trap_entry_48();
    fn trap_entry_255();
    fn x86_syscall_entry();
}

/// Remapeia o 8259 PIC para vetores 32..47 e habilita apenas IRQ 0 (timer)
pub fn init_pic() {
    unsafe {
        // ICW1: inicia inicialização em modo cascata
        outb(0x20, 0x11);
        outb(0xA0, 0x11);
        // ICW2: offset do vetor (Master = 32, Slave = 40)
        outb(0x21, 32);
        outb(0xA1, 40);
        // ICW3: conexão entre master e slave
        outb(0x21, 0x04);
        outb(0xA1, 0x02);
        // ICW4: modo 8086
        outb(0x21, 0x01);
        outb(0xA1, 0x01);
        // Máscaras: habilita IRQ 0 (bit 0 = 0) no Master, mascara os outros
        outb(0x21, 0xFE);
        outb(0xA1, 0xFF);
    }
}

/// Mascara todas as interrupções do PIC legado (usado ao ativar LAPIC)
pub fn mask_pic() {
    unsafe {
        outb(0x21, 0xFF);
        outb(0xA1, 0xFF);
    }
}

pub fn init_idt() {
    let handlers: [unsafe extern "C" fn(); 34] = [
        trap_entry_0, trap_entry_1, trap_entry_2, trap_entry_3,
        trap_entry_4, trap_entry_5, trap_entry_6, trap_entry_7,
        trap_entry_8, trap_entry_9, trap_entry_10, trap_entry_11,
        trap_entry_12, trap_entry_13, trap_entry_14, trap_entry_15,
        trap_entry_16, trap_entry_17, trap_entry_18, trap_entry_19,
        trap_entry_20, trap_entry_21, trap_entry_22, trap_entry_23,
        trap_entry_24, trap_entry_25, trap_entry_26, trap_entry_27,
        trap_entry_28, trap_entry_29, trap_entry_30, trap_entry_31,
        trap_entry_32, trap_entry_33,
    ];

    unsafe {
        for (i, &h) in handlers.iter().enumerate() {
            IDT[i].set_handler(h as *const () as u64, 0);
        }
        IDT[48].set_handler(trap_entry_48 as *const () as u64, 0);
        IDT[255].set_handler(trap_entry_255 as *const () as u64, 0);

        let ptr = IdtPtr {
            limit: (core::mem::size_of::<[IdtEntry; 256]>() - 1) as u16,
            base: (&raw const IDT) as u64,
        };
        asm!("lidt [{}]", in(reg) &ptr, options(nostack));
    }
}

pub fn init_idt_secondary() {
    unsafe {
        let ptr = IdtPtr {
            limit: (core::mem::size_of::<[IdtEntry; 256]>() - 1) as u16,
            base: (&raw const IDT) as u64,
        };
        asm!("lidt [{}]", in(reg) &ptr, options(nostack));
    }
}

#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct Tss64 {
    reserved0: u32,
    pub rsp0: u64,
    pub rsp1: u64,
    pub rsp2: u64,
    reserved1: u64,
    pub ist: [u64; 7],
    reserved2: u64,
    reserved3: u16,
    pub iomap_base: u16,
}

static mut TSS: [Tss64; super::platform::MAX_CPUS] = [Tss64 {
    reserved0: 0,
    rsp0: 0,
    rsp1: 0,
    rsp2: 0,
    reserved1: 0,
    ist: [0; 7],
    reserved2: 0,
    reserved3: 0,
    iomap_base: core::mem::size_of::<Tss64>() as u16,
}; super::platform::MAX_CPUS];

#[repr(C, align(64))]
#[derive(Clone, Copy)]
pub struct PerCpu {
    pub user_rsp: u64,
    pub kstack_top: u64,
}

pub static mut PER_CPU: [PerCpu; super::platform::MAX_CPUS] = [PerCpu {
    user_rsp: 0,
    kstack_top: 0,
}; super::platform::MAX_CPUS];

unsafe extern "C" {
    static mut boot_gdt64: [u64; 21];
    static boot_gdt64_ptr: u8;
}

pub fn init_gdt_tss(cpu: usize) {
    unsafe {
        asm!("lgdt [{}]", in(reg) &raw const boot_gdt64_ptr, options(nostack));

        let tss_ptr = &raw mut TSS[cpu];
        (*tss_ptr).rsp0 = crate::arch::kstack_top(cpu);
        (*tss_ptr).iomap_base = core::mem::size_of::<Tss64>() as u16;

        let base = tss_ptr as u64;
        let limit = (core::mem::size_of::<Tss64>() - 1) as u64;

        let desc_low = (limit & 0xFFFF)
            | ((base & 0xFFFF) << 16)
            | (((base >> 16) & 0xFF) << 32)
            | (0x89u64 << 40)
            | (((limit >> 16) & 0x0F) << 48)
            | (((base >> 24) & 0xFF) << 56);
        let desc_high = (base >> 32) & 0xFFFF_FFFF;

        let entry_idx = (0x28 / 8) + cpu * 2;
        boot_gdt64[entry_idx] = desc_low;
        boot_gdt64[entry_idx + 1] = desc_high;

        let sel = (0x28 + cpu * 16) as u16;
        asm!("ltr {0:x}", in(reg) sel, options(nomem, nostack));
    }
}

pub fn init_syscall(cpu: usize) {
    unsafe {
        PER_CPU[cpu].kstack_top = crate::arch::kstack_top(cpu);

        // MSR_KERNEL_GS_BASE (0xC0000102): aponta para PER_CPU[cpu]
        wrmsr(0xC0000102, (&raw const PER_CPU[cpu]) as u64);

        // MSR_STAR (0xC0000081):
        // [47:32] = Kernel CS (0x08), Kernel SS é CS + 8 (0x10)
        // [63:48] = User CS base: SYSRET carrega CS de [63:48]+16 e SS de [63:48]+8
        // Com base 0x10: User SS = 0x18 | 3 = 0x1B; User CS = 0x20 | 3 = 0x23
        let star: u64 = (0x0013u64 << 48) | (0x0008u64 << 32);
        wrmsr(0xC0000081, star);

        // MSR_LSTAR (0xC0000082): endereço de entrada do syscall
        wrmsr(0xC0000082, x86_syscall_entry as *const () as u64);

        // MSR_SFMASK (0xC0000084): mascara RFLAGS no syscall (limpa IF: 0x200)
        wrmsr(0xC0000084, 0x200);

        // MSR_EFER (0xC0000080): liga SCE (bit 0)
        let efer = rdmsr(0xC0000080);
        wrmsr(0xC0000080, efer | 1);
    }
}

pub unsafe fn rdmsr(msr: u32) -> u64 {
    let lo: u32;
    let hi: u32;
    unsafe {
        asm!("rdmsr", in("ecx") msr, out("eax") lo, out("edx") hi, options(nomem, nostack));
    }
    ((hi as u64) << 32) | (lo as u64)
}

pub unsafe fn wrmsr(msr: u32, val: u64) {
    let lo = val as u32;
    let hi = (val >> 32) as u32;
    unsafe {
        asm!("wrmsr", in("ecx") msr, in("eax") lo, in("edx") hi, options(nomem, nostack));
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn x86_trap_dispatch(frame: *mut Context) {
    let vec = unsafe { (*frame).vector };

    if vec == 14 {
        // #PF Page Fault
        let cr2: u64;
        unsafe { asm!("mov {}, cr2", out(reg) cr2, options(nomem, nostack)); }

        let err = unsafe { (*frame).err_code };
        let is_write = err & 2 != 0;
        let is_exec = err & 16 != 0;
        let access = if is_exec {
            Access::Exec
        } else if is_write {
            Access::Write
        } else {
            Access::Read
        };
        crate::trap::on_page_fault(frame, "falha de pagina", cr2, access);
    } else if vec == 32 {
        // IRQ 0: Timer PIC / LAPIC Timer
        unsafe { outb(0x20, 0x20); } // EOI para Master PIC
        super::apic::eoi();          // EOI para LAPIC
        crate::trap::on_timer(frame);
    } else if vec == 48 {
        // IPI inter-processador
        super::apic::eoi();
        crate::trap::on_ipi(frame);
    } else if vec == 255 {
        // Spurious interrupt do LAPIC
    } else if vec < 32 {
        // Exceção de hardware não tratada
        let mut u = crate::uart::Uart;
        use core::fmt::Write;
        let _ = write!(u, "[x86_trap] vec={} err=0x{:x} rip=0x{:x} cs=0x{:x} rsp=0x{:x} ss=0x{:x}\n",
                       vec, unsafe { (*frame).err_code }, unsafe { (*frame).rip }, unsafe { (*frame).cs },
                       unsafe { (*frame).rsp }, unsafe { (*frame).ss });
        let rip = unsafe { (*frame).rip };
        crate::trap::on_user_fault_sig(frame, "excecao de hardware", rip, 11, vec as i32);
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn x86_syscall_dispatch(frame: *mut Context) {
    crate::trap::on_syscall(frame);
}

