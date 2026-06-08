use core::arch::asm;
use aarch64_cpu::registers::*;
use tock_registers::interfaces::{Readable, Writeable};
use crate::frame_allocator;

#[repr(C, align(4096))]
struct Table { e: [u64; 512] }

// So a L0 raiz continua estatica — e o ponto de entrada do walk.
// Todas as outras tabelas (L1/L2/L3) sao alocadas dinamicamente.
static mut L0: Table = Table { e: [0; 512] };

const TABLE: u64 = 0b11;   // descritor de tabela (aponta p/ proximo nivel)
const PAGE:  u64 = 0b11;   // descritor de pagina (L3)
const VALID_MASK: u64 = 0b11;

// Flags de pagina, agrupados numa struct para clareza no loader.
#[derive(Clone, Copy)]
pub struct PageFlags {
    pub bits: u64,
}

impl PageFlags {
    const AF:    u64 = 1 << 10;
    const SH_IS: u64 = 0b11 << 8;
    const IDX_DEV: u64 = 0 << 2;
    const IDX_NRM: u64 = 1 << 2;
    const AP_KERNEL: u64 = 0b00 << 6;  // RW so EL1 -> executavel em EL1
    const AP_USER:   u64 = 0b01 << 6;  // RW EL0+EL1 -> executavel em EL0
    const AP_USER_RO: u64 = 0b11 << 6;  // RO EL0+EL1 (nao-gravavel)
    const PXN: u64 = 1 << 53;
    const UXN: u64 = 1 << 54;

    /// Codigo/dados do kernel: Normal, RW so EL1, executavel em EL1.
    pub fn kernel_rwx() -> Self {
        Self { bits: Self::IDX_NRM | Self::AP_KERNEL | Self::AF | Self::SH_IS }
    }

    /// Periferico (UART/GIC): Device, RW so EL1, nunca executavel.
    pub fn device() -> Self {
        Self { bits: Self::IDX_DEV | Self::AP_KERNEL | Self::AF | Self::PXN | Self::UXN }
    }

    /// Codigo de usuario: Normal, RW EL0+EL1, executavel em EL0, nao-exec em EL1.
    pub fn user_code() -> Self {
        Self { bits: Self::IDX_NRM | Self::AP_USER_RO | Self::AF | Self::SH_IS | Self::PXN }
    }

    /// Dados/stack de usuario: Normal, RW EL0+EL1, nao-executavel.
    pub fn user_data() -> Self {
        Self { bits: Self::IDX_NRM | Self::AP_USER | Self::AF | Self::SH_IS | Self::PXN | Self::UXN }
    }
}

pub const USER_CODE_VA:   u64 = 0x4010_0000;
pub const USER_STACK_TOP: u64 = 0x4010_2000;

/// Indices de cada nivel para um dado VA (4KB granule, VA 48 bits).
#[inline]
fn indices(va: u64) -> (usize, usize, usize, usize) {
    (
        ((va >> 39) & 0x1FF) as usize,
        ((va >> 30) & 0x1FF) as usize,
        ((va >> 21) & 0x1FF) as usize,
        ((va >> 12) & 0x1FF) as usize,
    )
}

/// Garante que a entrada `idx` da tabela em `table_pa` aponta para uma
/// tabela do proximo nivel; aloca uma nova (zerada) se necessario.
/// Retorna o endereco fisico da tabela do proximo nivel.
unsafe fn next_table(table_pa: u64, idx: usize) -> u64 {
    unsafe {
        let entry_ptr = (table_pa as *mut u64).add(idx);
        let entry = entry_ptr.read_volatile();
        if entry & VALID_MASK == TABLE {
            entry & 0x0000_FFFF_FFFF_F000
        } else {
            let new_pa = frame_allocator::alloc_frame().expect("sem frames para page table");
            core::ptr::write_bytes(new_pa as *mut u8, 0, 4096);
            entry_ptr.write_volatile(new_pa | TABLE);
            new_pa
        }
    }
}

/// Mapeia uma pagina de 4KB: VA -> PA com os flags dados.
/// Constroi os niveis intermediarios sob demanda via frame allocator.
pub unsafe fn map_page(va: u64, pa: u64, flags: PageFlags) {
    let (i0, i1, i2, i3) = indices(va);
    let l0_pa = (&raw const L0) as u64;
    unsafe {
        let l1_pa = next_table(l0_pa, i0);
        let l2_pa = next_table(l1_pa, i1);
        let l3_pa = next_table(l2_pa, i2);
        // descritor de pagina final no L3
        let l3_entry = (l3_pa as *mut u64).add(i3);
        l3_entry.write_volatile((pa & 0x0000_FFFF_FFFF_F000) | PAGE | flags.bits);
    }
}

/// Mapeia um intervalo [va, va+size) identity (VA==PA) com os flags dados.
pub unsafe fn map_range_identity(start: u64, size: u64, flags: PageFlags) {
    let mut off = 0u64;
    while off < size {
        unsafe { map_page(start + off, start + off, flags); }
        off += 4096;
    }
}

pub unsafe fn init() {
    unsafe {
        // UART (PL011) em 0x09000000
        map_page(0x0900_0000, 0x0900_0000, PageFlags::device());

        // GICv2: distribuidor (GICD) e interface de CPU (GICC)
        map_page(0x0800_0000, 0x0800_0000, PageFlags::device()); // GICD
        map_page(0x0801_0000, 0x0801_0000, PageFlags::device()); // GICC

        // Kernel + stacks: identity em 0x40000000, 2MB, kernel_rwx.
        map_range_identity(0x4000_0000, 0x20_0000, PageFlags::kernel_rwx());

        // frames que o loader vai alocar) — identity, para o kernel poder
        // ler/editar tabelas com a MMU ja ligada. Mapeia os primeiros 16MB.
        map_range_identity(0x4020_0000, 0x100_0000, PageFlags::kernel_rwx());

        // Regiao de usuario A: codigo + stack
        map_page(0x4010_0000, 0x4010_0000, PageFlags::user_code());
        map_page(0x4010_1000, 0x4010_1000, PageFlags::user_data());

        // Regiao de usuario B: codigo + stack
        map_page(0x4011_0000, 0x4011_0000, PageFlags::user_code());
        map_page(0x4011_1000, 0x4011_1000, PageFlags::user_data());
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

    TTBR0_EL1.set((&raw const L0) as u64);

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