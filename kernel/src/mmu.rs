use core::arch::asm;
use aarch64_cpu::registers::*;
use tock_registers::interfaces::{Readable, Writeable};

#[repr(C, align(4096))]
struct Table { e: [u64; 512] }

static mut L0:     Table = Table { e: [0; 512] };
static mut L1:     Table = Table { e: [0; 512] };
static mut L2_DEV: Table = Table { e: [0; 512] };
static mut L3_DEV: Table = Table { e: [0; 512] };
static mut L2_RAM: Table = Table { e: [0; 512] };
static mut L3_RAM: Table = Table { e: [0; 512] };

const TABLE: u64 = 0b11;
const PAGE:  u64 = 0b11;
const AF:    u64 = 1 << 10;

const AP_KERNEL: u64 = 0b00 << 6;   // RW so EL1 -> executavel em EL1
const AP_USER:   u64 = 0b01 << 6;   // RW EL0+EL1 -> executavel em EL0 (PXN em EL1)
const SH_IS:     u64 = 0b11 << 8;
const IDX_DEV:   u64 = 0 << 2;
const IDX_NRM:   u64 = 1 << 2;
const PXN:       u64 = 1 << 53;
const UXN:       u64 = 1 << 54;

// Enderecos da regiao de usuario (dentro dos 2MB cobertos pela L3_RAM)
pub const USER_CODE_VA:  u64 = 0x4010_0000;   // pagina de codigo de EL0
pub const USER_STACK_TOP: u64 = 0x4010_2000;  // topo da stack de EL0 (cresce p/ baixo)

pub unsafe fn init() {
    unsafe {
        let l0  = (&raw mut L0).cast::<u64>();
        let l1  = (&raw mut L1).cast::<u64>();
        let l2d = (&raw mut L2_DEV).cast::<u64>();
        let l3d = (&raw mut L3_DEV).cast::<u64>();
        let l2r = (&raw mut L2_RAM).cast::<u64>();
        let l3r = (&raw mut L3_RAM).cast::<u64>();

        l0.add(0).write_volatile((&raw const L1 as u64) | TABLE);
        l1.add(0).write_volatile((&raw const L2_DEV as u64) | TABLE);
        l1.add(1).write_volatile((&raw const L2_RAM as u64) | TABLE);

        l2d.add(72).write_volatile((&raw const L3_DEV as u64) | TABLE);
        l3d.add(0).write_volatile(0x0900_0000 | PAGE | IDX_DEV | AP_KERNEL | AF | PXN | UXN);

        // RAM do kernel: tudo AP_KERNEL (so EL1, executavel em EL1)
        l2r.add(0).write_volatile((&raw const L3_RAM as u64) | TABLE);
        for i in 0..512u64 {
            let pa = 0x4000_0000 + i * 0x1000;
            l3r.add(i as usize).write_volatile(pa | PAGE | IDX_NRM | AP_KERNEL | AF | SH_IS);
        }

        // --- Sobrescreve as paginas de USUARIO com permissao de EL0 ---
        // indice L3 = (VA - 0x40000000) / 4KB
        let code_idx  = ((USER_CODE_VA  - 0x4000_0000) / 0x1000) as usize;  // 0x100
        let stack_idx = code_idx + 1;  // pagina seguinte para a stack

        // Codigo de usuario: AP_USER + executavel em EL0 (UXN=0), nao-exec em EL1 (PXN=1)
        l3r.add(code_idx).write_volatile(
            USER_CODE_VA | PAGE | IDX_NRM | AP_USER | AF | SH_IS | PXN
        );
        // Stack de usuario: AP_USER, nao-executavel (UXN+PXN)
        l3r.add(stack_idx).write_volatile(
            (USER_CODE_VA + 0x1000) | PAGE | IDX_NRM | AP_USER | AF | SH_IS | PXN | UXN
        );
    }

    MAIR_EL1.write(
        MAIR_EL1::Attr0_Device::nonGathering_nonReordering_noEarlyWriteAck
        + MAIR_EL1::Attr1_Normal_Inner::WriteBack_NonTransient_ReadWriteAlloc
        + MAIR_EL1::Attr1_Normal_Outer::WriteBack_NonTransient_ReadWriteAlloc,
    );

    TCR_EL1.write(
        TCR_EL1::T0SZ.val(16)
        + TCR_EL1::IRGN0::WriteBack_ReadAlloc_WriteAlloc_Cacheable
        + TCR_EL1::ORGN0::WriteBack_ReadAlloc_WriteAlloc_Cacheable
        + TCR_EL1::SH0::Inner
        + TCR_EL1::TG0::KiB_4
        + TCR_EL1::EPD1::DisableTTBR1Walks
        + TCR_EL1::IPS::Bits_40,
    );

    TTBR0_EL1.set(&raw const L0 as u64);

    unsafe { asm!("dsb ish", "tlbi vmalle1", "dsb ish", "isb", options(nostack, preserves_flags)); }

    let mut sctlr = SCTLR_EL1.get();
    sctlr &= !(1u64 << 19);
    sctlr |= (1 << 0) | (1 << 2) | (1 << 12);
    SCTLR_EL1.set(sctlr);

    unsafe { asm!("isb", "ic iallu", "tlbi vmalle1", "dsb nsh", "isb", options(nostack, preserves_flags)); }
}

pub fn is_enabled() -> bool {
    SCTLR_EL1.is_set(SCTLR_EL1::M)
}