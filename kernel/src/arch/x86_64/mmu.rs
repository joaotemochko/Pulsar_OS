//! MMU x86_64: tabelas de páginas de 4 níveis (PML4, PDPT, PD, PT) com
//! páginas de 4 KiB e blocos de 2 MiB para o mapa identidade do kernel.

use core::arch::asm;
use crate::frame_allocator;
use super::platform;

#[repr(C, align(4096))]
struct Table { e: [u64; 512] }

/// Raiz (PML4) do kernel.
static mut L0: Table = Table { e: [0; 512] };

const P_PRESENT: u64 = 1 << 0;
const P_RW:      u64 = 1 << 1;
const P_USER:    u64 = 1 << 2;
const P_PWT:     u64 = 1 << 3;
const P_PCD:     u64 = 1 << 4;
const P_PS:      u64 = 1 << 7; // 2 MiB page in PD
const P_NX:      u64 = 1 << 63;

const TABLE_FLAGS: u64 = P_PRESENT | P_RW | P_USER;
const VALID_MASK:  u64 = P_PRESENT;
const ADDR_MASK:   u64 = 0x000F_FFFF_FFFF_F000;

/// Bit de software 55: página pertence a objeto compartilhado (shm) ou reservada
const SW_SHARED: u64 = 1 << 55;

#[derive(Clone, Copy)]
pub struct PageFlags {
    pub bits: u64,
}

impl PageFlags {
    /// Código/dados do kernel: RW, Ring 0, executável
    pub fn kernel_rwx() -> Self {
        Self { bits: P_PRESENT | P_RW }
    }

    /// Periférico: Device (PCD | PWT), Ring 0, não-executável
    pub fn device() -> Self {
        Self { bits: P_PRESENT | P_RW | P_PCD | P_PWT | P_NX }
    }

    /// Dados do kernel / frames: RW, Ring 0, não-executável
    pub fn kernel_data() -> Self {
        Self { bits: P_PRESENT | P_RW | P_NX }
    }

    /// Código de usuário: Presente, Ring 3, somente leitura, executável
    pub fn user_code() -> Self {
        Self { bits: P_PRESENT | P_USER }
    }

    /// Dados/pilha de usuário: Presente, RW, Ring 3, não-executável
    pub fn user_data() -> Self {
        Self { bits: P_PRESENT | P_RW | P_USER | P_NX }
    }

    /// Região gráfica compartilhada com o usuário
    pub fn user_shared() -> Self {
        Self { bits: P_PRESENT | P_RW | P_USER | P_NX }
    }

    pub fn shared(self) -> Self {
        Self { bits: self.bits | SW_SHARED }
    }

    /// Página com proteção `prot` (bit0 R, bit1 W, bit2 X)
    pub fn user_prot(prot: u64) -> Self {
        let mut bits = P_PRESENT;
        if prot != 0 {
            bits |= P_USER;
        }
        if prot & 2 != 0 {
            bits |= P_RW;
        }
        if prot & 4 == 0 {
            bits |= P_NX;
        }
        Self { bits }
    }
}

#[inline]
fn indices(va: u64) -> (usize, usize, usize, usize) {
    (
        ((va >> 39) & 0x1FF) as usize,
        ((va >> 30) & 0x1FF) as usize,
        ((va >> 21) & 0x1FF) as usize,
        ((va >> 12) & 0x1FF) as usize,
    )
}

unsafe fn next_table(table_pa: u64, idx: usize) -> Option<u64> {
    unsafe {
        let entry_ptr = (table_pa as *mut u64).add(idx);
        let entry = entry_ptr.read_volatile();
        if entry & VALID_MASK != 0 {
            if entry & P_PS != 0 {
                return None;
            }
            Some(entry & ADDR_MASK)
        } else {
            let new_pa = frame_allocator::alloc_frame()?;
            core::ptr::write_bytes(new_pa as *mut u8, 0, 4096);
            entry_ptr.write_volatile(new_pa | TABLE_FLAGS);
            Some(new_pa)
        }
    }
}

pub fn kernel_root() -> u64 {
    (&raw const L0) as u64
}

pub unsafe fn map_page_locked(root_pa: u64, va: u64, pa: u64, flags: PageFlags) -> bool {
    let (i0, i1, i2, i3) = indices(va);
    unsafe {
        let Some(l1) = next_table(root_pa, i0) else { return false };
        let Some(l2) = next_table(l1, i1) else { return false };
        let Some(l3) = next_table(l2, i2) else { return false };
        (l3 as *mut u64).add(i3).write_volatile((pa & ADDR_MASK) | flags.bits);
    }
    true
}

pub unsafe fn map_page_in(root_pa: u64, va: u64, pa: u64, flags: PageFlags) -> bool {
    let _g = crate::sync::MMU.guard();
    unsafe { map_page_locked(root_pa, va, pa, flags) }
}

const SZ_2M: u64 = 2 * 1024 * 1024;

unsafe fn map_block_2m(va: u64, flags: PageFlags) -> bool {
    let (i0, i1, i2, _) = indices(va);
    unsafe {
        let Some(l1) = next_table(kernel_root(), i0) else { return false };
        let Some(l2) = next_table(l1, i1) else { return false };
        (l2 as *mut u64).add(i2).write_volatile((va & ADDR_MASK) | P_PS | flags.bits);
    }
    true
}

unsafe fn map_range_identity(start: u64, size: u64, flags: PageFlags) {
    let _g = crate::sync::MMU.guard();
    let mut off = 0u64;
    while off < size {
        unsafe { map_page_locked(kernel_root(), start + off, start + off, flags); }
        off += 4096;
    }
}

static mut MMU_ACTIVE: bool = false;

#[unsafe(no_mangle)]
pub static mut x86_kernel_root: u64 = 0;

pub unsafe fn init() {
    unsafe {
        // MMIO: Slots Virtio em 0xfeb0_0000 (GB 3)
        map_range_identity(
            platform::VIRTIO_MMIO_BASE,
            platform::VIRTIO_MMIO_SLOTS * platform::VIRTIO_MMIO_STRIDE,
            PageFlags::device(),
        );

        // Local APIC (0xFEE0_0000 no GB 3)
        map_range_identity(0xFEE0_0000, 4096, PageFlags::device());

        // Região gráfica compartilhada (GB 1: 0x4600_0000 .. 0x5100_0000)
        map_range_identity(platform::FB_PA, platform::FB_REGION_BYTES, PageFlags::kernel_data());

        // Kernel: código, dados, BSS, IDT, GDT, TSS, stacks (0..16 MiB: índices 0..7 de 2 MiB)
        let mut a = 0u64;
        while a < 0x0100_0000 {
            map_block_2m(a, PageFlags::kernel_rwx());
            a += SZ_2M;
        }

        // Heap do kernel: HEAP_START .. RAM_END (1344 MiB .. 3072 MiB em blocos de 2 MiB)
        let mut a = platform::HEAP_START;
        while a < platform::RAM_END {
            map_block_2m(a, PageFlags::kernel_data());
            a += SZ_2M;
        }

        // Carrega CR3 com a raiz final do kernel
        let root = kernel_root();
        x86_kernel_root = root;
        asm!("mov cr3, {}", in(reg) root, options(nostack, preserves_flags));
        MMU_ACTIVE = true;
    }
}

pub fn is_enabled() -> bool {
    unsafe { MMU_ACTIVE }
}

pub fn flush_tlb() {
    unsafe {
        let cr3: u64;
        asm!("mov {}, cr3", out(reg) cr3, options(nostack, preserves_flags));
        asm!("mov cr3, {}", in(reg) cr3, options(nostack, preserves_flags));
    }
}

pub fn flush_range(_asid: u16, start: u64, end: u64) {
    let start = start & !0xFFF;
    let end = (end + 4095) & !0xFFF;
    if start >= end { return; }
    let pages = (end - start) / 4096;
    if pages > 64 {
        flush_tlb();
        return;
    }
    let mut va = start;
    while va < end {
        unsafe { asm!("invlpg [{}]", in(reg) va, options(nostack, preserves_flags)); }
        va += 4096;
    }
}


pub fn flush_asid(_asid: u16) {
    flush_tlb();
}

#[inline]
pub fn sync_new_pages() {}

unsafe fn table_entry(table_pa: u64, idx: usize) -> u64 {
    unsafe { (table_pa as *const u64).add(idx).read_volatile() }
}

pub fn create_user_space() -> Option<u64> {
    let _g = crate::sync::MMU.guard();
    let l0 = frame_allocator::alloc_frame()?;
    let l1 = frame_allocator::alloc_frame()?;
    let l2_gb0 = frame_allocator::alloc_frame()?;
    unsafe {
        core::ptr::write_bytes(l0 as *mut u8, 0, 4096);
        core::ptr::write_bytes(l1 as *mut u8, 0, 4096);
        core::ptr::write_bytes(l2_gb0 as *mut u8, 0, 4096);

        let k_l1 = table_entry(kernel_root(), 0) & ADDR_MASK;
        let k_l2_gb0 = table_entry(k_l1, 0) & ADDR_MASK;

        // GB1..: Heap do kernel, região gráfica e MMIO (compartilhados)
        for i1 in 1..512 {
            let e = table_entry(k_l1, i1);
            if e & VALID_MASK != 0 {
                (l1 as *mut u64).add(i1).write_volatile(e);
            }
        }
        (l1 as *mut u64).add(0).write_volatile(l2_gb0 | TABLE_FLAGS);

        // No GB0: copia APENAS os primeiros 16 MiB do kernel (índices 0..7)!
        // Índices 8..511 (16 MiB .. 1 GiB) começam livres para o espaço de usuário!
        for idx in 0..8 {
            let e = table_entry(k_l2_gb0, idx);
            if e & VALID_MASK != 0 {
                (l2_gb0 as *mut u64).add(idx).write_volatile(e);
            }
        }
        (l0 as *mut u64).add(0).write_volatile(l1 | TABLE_FLAGS);
    }
    Some(l0)
}

pub fn free_user_space(l0: u64) {
    let _g = crate::sync::MMU.guard();
    unsafe {
        let k_l1 = table_entry(kernel_root(), 0) & ADDR_MASK;
        let k_l2_gb0 = table_entry(k_l1, 0) & ADDR_MASK;
        for i0 in 0..512 {
            let e0 = table_entry(l0, i0);
            if e0 & VALID_MASK == 0 { continue; }
            let l1 = e0 & ADDR_MASK;
            for i1 in 0..512 {
                let e1 = table_entry(l1, i1);
                if e1 & VALID_MASK == 0 { continue; }
                if i0 == 0 && e1 == table_entry(k_l1, i1) { continue; } // Kernel compartilhado (GB1..)
                let l2 = e1 & ADDR_MASK;
                for i2 in 0..512 {
                    let e2 = table_entry(l2, i2);
                    if e2 & VALID_MASK == 0 { continue; }
                    if i0 == 0 && i1 == 0 && (i2 < 8 || e2 == table_entry(k_l2_gb0, i2)) { continue; } // Kernel GB0 (0..16 MiB)
                    if e2 & P_PS != 0 { continue; }
                    let l3 = e2 & ADDR_MASK;
                    for i3 in 0..512 {
                        let e3 = table_entry(l3, i3);
                        if e3 & VALID_MASK != 0 && e3 & SW_SHARED == 0 {
                            frame_allocator::free_frame(e3 & ADDR_MASK);
                        }
                    }
                    frame_allocator::free_frame(l3);
                }
                frame_allocator::free_frame(l2);
            }
            frame_allocator::free_frame(l1);
        }
        frame_allocator::free_frame(l0);
    }
}

pub fn switch_space(l0_pa: u64, _asid: u16) {
    unsafe {
        asm!("mov cr3, {}", in(reg) l0_pa, options(nostack, preserves_flags));
    }
}


pub fn translate_user(root_pa: u64, va: u64, write: bool) -> Option<u64> {
    let (i0, i1, i2, i3) = indices(va);
    unsafe {
        let e0 = table_entry(root_pa, i0);
        if e0 & VALID_MASK == 0 { return None; }
        let e1 = table_entry(e0 & ADDR_MASK, i1);
        if e1 & VALID_MASK == 0 { return None; }
        let e2 = table_entry(e1 & ADDR_MASK, i2);
        if e2 & VALID_MASK == 0 || e2 & P_PS != 0 { return None; }
        let e3 = table_entry(e2 & ADDR_MASK, i3);
        if e3 & VALID_MASK == 0 { return None; }
        if e3 & P_USER == 0 { return None; } // Apenas acessível em Ring 3
        if write && (e3 & P_RW == 0) { return None; }
        Some((e3 & ADDR_MASK) | (va & 0xFFF))
    }
}

unsafe fn walk_leaves(root_pa: u64, start: u64, end: u64, mut f: impl FnMut(u64, *mut u64)) {
    let next = |va: u64, shift: u32| ((va >> shift) + 1) << shift;
    let mut va = start & !0xFFF;
    while va < end {
        let (i0, i1, i2, i3) = indices(va);
        unsafe {
            let e0 = table_entry(root_pa, i0);
            if e0 & VALID_MASK == 0 { va = next(va, 39); continue; }
            let e1 = table_entry(e0 & ADDR_MASK, i1);
            if e1 & VALID_MASK == 0 { va = next(va, 30); continue; }
            let e2 = table_entry(e1 & ADDR_MASK, i2);
            if e2 & VALID_MASK == 0 || e2 & P_PS != 0 { va = next(va, 21); continue; }
            f(va, ((e2 & ADDR_MASK) as *mut u64).add(i3));
        }
        va += 4096;
    }
}

pub fn unmap_range(root_pa: u64, start: u64, end: u64) -> usize {
    let _g = crate::sync::MMU.guard();
    let mut count = 0;
    unsafe {
        walk_leaves(root_pa, start, end, |_, p| {
            let e = p.read_volatile();
            if e & VALID_MASK != 0 {
                p.write_volatile(0);
                if e & SW_SHARED == 0 {
                    frame_allocator::free_frame(e & ADDR_MASK);
                }
                count += 1;
            }
        });
    }
    count
}

pub fn protect_range(root_pa: u64, start: u64, end: u64, flags: PageFlags) -> usize {
    let _g = crate::sync::MMU.guard();
    let mut count = 0;
    unsafe {
        walk_leaves(root_pa, start, end, |_, p| {
            let e = p.read_volatile();
            if e & VALID_MASK != 0 {
                let new_e = (e & ADDR_MASK) | flags.bits | (e & SW_SHARED);
                if new_e != e {
                    p.write_volatile(new_e);
                    count += 1;
                }
            }
        });
    }
    count
}

pub fn move_range(root_pa: u64, old_start: u64, old_end: u64, new_start: u64) -> usize {
    let _g = crate::sync::MMU.guard();
    let mut count = 0;
    unsafe {
        walk_leaves(root_pa, old_start, old_end, |va, p| {
            let e = p.read_volatile();
            if e & VALID_MASK != 0 {
                let dst_va = new_start + (va - old_start);
                let flags = PageFlags { bits: e & !ADDR_MASK };
                if map_page_locked(root_pa, dst_va, e & ADDR_MASK, flags) {
                    p.write_volatile(0);
                    count += 1;
                }
            }
        });
    }
    count
}

pub fn is_mapped(root_pa: u64, va: u64) -> bool {
    let _g = crate::sync::MMU.guard();
    let mut found = false;
    unsafe {
        walk_leaves(root_pa, va, va + 1, |_, p| {
            found = (p.read_volatile() & VALID_MASK) != 0;
        });
    }
    found
}

/// fork(): copia com Copy-on-Write (COW).
pub fn clone_user_space(src: u64) -> Option<u64> {
    let dst = create_user_space()?;
    let _g = crate::sync::MMU.guard();
    unsafe {
        let k_l1 = table_entry(kernel_root(), 0) & ADDR_MASK;
        let k_l2_gb0 = table_entry(k_l1, 0) & ADDR_MASK;
        for i0 in 0..512 {
            let e0 = table_entry(src, i0);
            if e0 & VALID_MASK == 0 { continue; }
            let l1 = e0 & ADDR_MASK;
            for i1 in 0..512 {
                let e1 = table_entry(l1, i1);
                if e1 & VALID_MASK == 0 { continue; }
                if i0 == 0 && e1 == table_entry(k_l1, i1) { continue; }
                let l2 = e1 & ADDR_MASK;
                for i2 in 0..512 {
                    let e2 = table_entry(l2, i2);
                    if e2 & VALID_MASK == 0 { continue; }
                    if i0 == 0 && i1 == 0 && (i2 < 8 || e2 == table_entry(k_l2_gb0, i2)) { continue; }
                    if e2 & P_PS != 0 { continue; }
                    let l3 = e2 & ADDR_MASK;
                    for i3 in 0..512 {
                        let e3 = table_entry(l3, i3);
                        if e3 & VALID_MASK == 0 { continue; }
                        let va = ((i0 as u64) << 39) | ((i1 as u64) << 30) | ((i2 as u64) << 21) | ((i3 as u64) << 12);
                        let pa = e3 & ADDR_MASK;
                        let flags = PageFlags { bits: e3 & !ADDR_MASK };
                        let read_only = (e3 & P_RW) == 0;
                        let (target, child_flags) = if frame_allocator::reserved(pa) || (e3 & SW_SHARED != 0) {
                            (pa, flags)
                        } else if read_only {
                            frame_allocator::inc_ref(pa);
                            (pa, flags)
                        } else {
                            // Copy-on-write: marca pai e filho como somente-leitura (limpa P_RW)
                            (l3 as *mut u64).add(i3).write_volatile(e3 & !P_RW);
                            frame_allocator::inc_ref(pa);
                            (pa, PageFlags { bits: flags.bits & !P_RW })
                        };
                        if !map_page_locked(dst, va, target, child_flags) {
                            if !frame_allocator::reserved(pa) && (e3 & SW_SHARED == 0) {
                                frame_allocator::free_frame(target);
                            }
                            drop(_g);
                            free_user_space(dst);
                            return None;
                        }
                    }
                }
            }
        }
        flush_tlb();
    }
    Some(dst)
}
