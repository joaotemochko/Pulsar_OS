//! MMU RISC-V Sv39: tabelas de 3 niveis, paginas de 4 KiB, ASIDs.
//!
//! Raiz do kernel (compartilhada por copia em todo espaco de usuario):
//!   [2]   gigapagina 0x8000_0000 (RAM: kernel, heap, regiao grafica), global
//!   [256] gigapagina PA 0 em MMIO_VA (UART, RTC, virtio), global, sem X
//! O resto da raiz (GB0, GB1, ...) e privado de cada processo.
use core::arch::asm;
use crate::frame_allocator;
use super::sbi;

#[repr(C, align(4096))]
struct Table { e: [u64; 512] }

static mut ROOT: Table = Table { e: [0; 512] };

const V: u64 = 1 << 0;
const R: u64 = 1 << 1;
const W: u64 = 1 << 2;
const X: u64 = 1 << 3;
const U: u64 = 1 << 4;
const G: u64 = 1 << 5;
const A: u64 = 1 << 6;
const D: u64 = 1 << 7;
const LEAF: u64 = R | W | X;

const SATP_SV39: u64 = 8 << 60;
/// Gigapaginas da RAM: indice da raiz de cada GiB de RAM_BASE ate RAM_END.
const IDX_RAM: usize = (super::platform::RAM_BASE >> 30) as usize;
const NRAM: usize = (super::platform::RAM_SIZE.div_ceil(1 << 30)) as usize;
#[inline]
fn is_ram_idx(i: usize) -> bool { i >= IDX_RAM && i < IDX_RAM + NRAM }
const IDX_MMIO: usize = 256;

#[derive(Clone, Copy)]
pub struct PageFlags {
    pub bits: u64,
}

impl PageFlags {
    pub fn kernel_rwx() -> Self { Self { bits: R | W | X | A | D | G } }
    pub fn device() -> Self { Self { bits: R | W | A | D | G } }
    /// Codigo de usuario: R+X (W^X).
    pub fn user_code() -> Self { Self { bits: R | X | U | A } }
    /// Dados/pilha de usuario: R+W, nunca executavel.
    pub fn user_data() -> Self { Self { bits: R | W | U | A | D } }
    /// Regiao grafica no espaco do processo (mesmos bits; privada aqui).
    pub fn user_shared() -> Self { Self::user_data() }
}

#[inline]
fn pte_pa(e: u64) -> u64 { (e >> 10) << 12 }
#[inline]
fn pa_pte(pa: u64) -> u64 { (pa >> 12) << 10 }
#[inline]
fn vpn(va: u64, level: u32) -> usize { ((va >> (12 + 9 * level)) & 0x1FF) as usize }

unsafe fn entry(table: u64, idx: usize) -> u64 {
    unsafe { (table as *const u64).add(idx).read_volatile() }
}
unsafe fn set_entry(table: u64, idx: usize, v: u64) {
    unsafe { (table as *mut u64).add(idx).write_volatile(v) }
}

pub fn kernel_root() -> u64 {
    (&raw const ROOT) as u64
}

pub unsafe fn init() {
    unsafe {
        let root = kernel_root();
        // gigapaginas exigem PA alinhado a 1 GiB
        for k in 0..NRAM {
            let pa = (((IDX_RAM + k) as u64) << 30) as u64;
            set_entry(root, IDX_RAM + k, pa_pte(pa) | PageFlags::kernel_rwx().bits | V);
        }
        set_entry(root, IDX_MMIO, pa_pte(0) | PageFlags::device().bits | V);
        enable_on_this_hart();
    }
}

pub unsafe fn enable_on_this_hart() {
    let satp = SATP_SV39 | (kernel_root() >> 12);
    unsafe { asm!("csrw satp, {}", "sfence.vma", in(reg) satp, options(nostack)) };
}

pub fn is_enabled() -> bool {
    let satp: u64;
    unsafe { asm!("csrr {}, satp", out(reg) satp, options(nomem, nostack)) };
    satp >> 60 == 8
}

/// Proxima tabela no indice `idx`, criando-a se ausente. None se a
/// entrada for uma folha (gigapagina do kernel) ou faltar memoria.
unsafe fn next_table(table: u64, idx: usize) -> Option<u64> {
    unsafe {
        let e = entry(table, idx);
        if e & V != 0 {
            if e & LEAF != 0 { return None; }
            return Some(pte_pa(e));
        }
        let new = frame_allocator::alloc_frame()?;
        core::ptr::write_bytes(new as *mut u8, 0, 4096);
        set_entry(table, idx, pa_pte(new) | V);
        Some(new)
    }
}

pub unsafe fn map_page_in(root: u64, va: u64, pa: u64, flags: PageFlags) -> bool {
    let _g = crate::sync::MMU.guard();
    unsafe {
        let Some(l1) = next_table(root, vpn(va, 2)) else { return false };
        let Some(l0) = next_table(l1, vpn(va, 1)) else { return false };
        set_entry(l0, vpn(va, 0), pa_pte(pa) | flags.bits | V);
    }
    true
}

pub fn create_user_space() -> Option<u64> {
    let _g = crate::sync::MMU.guard();
    let root = frame_allocator::alloc_frame()?;
    unsafe {
        core::ptr::write_bytes(root as *mut u8, 0, 4096);
        for k in IDX_RAM..IDX_RAM + NRAM {
            set_entry(root, k, entry(kernel_root(), k));
        }
        set_entry(root, IDX_MMIO, entry(kernel_root(), IDX_MMIO));
    }
    Some(root)
}

/// Libera tabelas e paginas privadas (folhas fora do heap, como a regiao
/// grafica, sao ignoradas pelo frame allocator).
pub fn free_user_space(root: u64) {
    let _g = crate::sync::MMU.guard();
    unsafe {
        for i2 in 0..512 {
            if is_ram_idx(i2) || i2 == IDX_MMIO { continue; }
            let e2 = entry(root, i2);
            if e2 & V == 0 || e2 & LEAF != 0 { continue; }
            let l1 = pte_pa(e2);
            for i1 in 0..512 {
                let e1 = entry(l1, i1);
                if e1 & V == 0 { continue; }
                if e1 & LEAF != 0 { continue; }
                let l0 = pte_pa(e1);
                for i0 in 0..512 {
                    let e0 = entry(l0, i0);
                    if e0 & V != 0 && e0 & LEAF != 0 {
                        frame_allocator::free_frame(pte_pa(e0));
                    }
                }
                frame_allocator::free_frame(l0);
            }
            frame_allocator::free_frame(l1);
        }
        frame_allocator::free_frame(root);
    }
}

pub fn switch_space(root: u64, asid: u16) {
    let satp = SATP_SV39 | ((asid as u64) << 44) | (root >> 12);
    unsafe { asm!("csrw satp, {}", in(reg) satp, options(nostack)) };
}

/// Torna visiveis novas entradas de tabela nesta hart (entradas novas
/// nunca estao em cache como invalidas nas demais).
pub fn flush_tlb() {
    unsafe { asm!("sfence.vma", options(nostack)) };
}

pub fn flush_asid(asid: u16) {
    unsafe { asm!("sfence.vma zero, {}", in(reg) asid as u64, options(nostack)) };
    sbi::remote_sfence_asid(asid as u64);
}

pub fn translate_user(root: u64, va: u64, write: bool) -> Option<u64> {
    if va >= (1 << 38) { return None; }
    unsafe {
        let mut table = root;
        let mut level = 2u32;
        loop {
            let e = entry(table, vpn(va, level));
            if e & V == 0 { return None; }
            if e & LEAF != 0 {
                if e & U == 0 { return None; }
                if write && e & W == 0 { return None; }
                let page_mask = (1u64 << (12 + 9 * level)) - 1;
                return Some((pte_pa(e) & !page_mask) | (va & page_mask));
            }
            if level == 0 { return None; }
            table = pte_pa(e);
            level -= 1;
        }
    }
}
