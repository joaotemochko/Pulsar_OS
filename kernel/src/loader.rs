use crate::pulse::*;
use crate::mmu::{self, PageFlags};
use crate::frame_allocator;
use crate::uart::Uart;
use core::fmt::Write;

/// Resultado do carregamento: onde saltar, stack e o espaco criado.
pub struct Loaded {
    pub entry: u64,
    pub stack_top: u64,
    pub l0: u64,
}

/// Alocador monotonico de ASIDs (1..=255; 0 e o kernel).
static mut NEXT_ASID: u16 = 1;

fn alloc_asid() -> u16 {
    unsafe {
        let a = NEXT_ASID;
        NEXT_ASID = if NEXT_ASID >= 255 { 1 } else { NEXT_ASID + 1 };
        // Em caso de reuso (wrap), garante que nao ha traducoes velhas.
        crate::mmu::flush_asid(a);
        a
    }
}

/// Carrega um arquivo .pulse (v2) a partir de um buffer na memoria.
/// Mapeia os segmentos com W^X e a stack EL0 abaixo de `stack_top`.
pub unsafe fn load_pulse(file: *const u8) -> Option<Loaded> {
    let mut serial = Uart;

    let header = unsafe { &*(file as *const PulseHeader) };
    if header.magic != PULSE_MAGIC {
        let _ = write!(serial, "[loader] magic invalido: {:#x}\n", header.magic);
        return None;
    }
    if header.version < 2 || header.stack_top == 0 {
        let _ = write!(serial, "[loader] .pulse v{} sem stack_top — rejeitado\n", header.version);
        return None;
    }
    let _ = write!(serial, "[loader] .pulse v{}, {} segmento(s), entry={:#x} stack={:#x}\n",
                   header.version, header.seg_count, header.entry, header.stack_top);

    // Espaco de enderecos PROPRIO deste processo (kernel compartilhado,
    // regiao de usuario privada e vazia).
    let l0 = mmu::create_user_space()?;

    // Segmentos
    let seg_table = unsafe { file.add(header.seg_table_off as usize) as *const PulseSegment };
    for i in 0..header.seg_count as usize {
        let seg = unsafe { &*seg_table.add(i) };
        // Guarda contra o bug classico: um .pulse linkado em cima de MMIO
        // (UART 0x0900_0000, GIC 0x0800_0000, virtio 0x0a00_0000) veria
        // registradores de dispositivo como codigo. Rejeita.
        if (0x0800_0000..0x0a01_0000).contains(&seg.vaddr) {
            let _ = write!(serial, "[loader] REJEITADO: segmento em VA de MMIO {:#x}\n", seg.vaddr);
            return None;
        }
        let _ = write!(serial, "[loader] seg {}: vaddr={:#x} file={}B mem={}B flags={:#b}\n",
                       i, seg.vaddr, seg.file_size, seg.mem_size, seg.flags);

        // W^X: segmento executavel nunca e gravavel
        let flags = if seg.flags & SEG_X != 0 {
            PageFlags::user_code()
        } else {
            PageFlags::user_data()
        };

        let mut off = 0u32;
        while off < seg.mem_size {
            let frame = frame_allocator::alloc_frame()?;
            let va_page = seg.vaddr + off as u64;
            unsafe {
                mmu::map_page_in(l0, va_page, frame, flags);
                core::ptr::write_bytes(frame as *mut u8, 0, 4096);
                if off < seg.file_size {
                    let remaining = seg.file_size - off;
                    let to_copy = core::cmp::min(remaining, 4096) as usize;
                    let src = file.add(seg.file_off as usize + off as usize);
                    core::ptr::copy_nonoverlapping(src, frame as *mut u8, to_copy);
                }
            }
            off += 4096;
        }
    }

    // Stack EL0: STACK_PAGES frames contiguos mapeados abaixo do topo
    let stack_base_va = header.stack_top - STACK_PAGES * 4096;
    let stack_pa = frame_allocator::alloc_contig(STACK_PAGES as usize)?;
    for p in 0..STACK_PAGES {
        unsafe {
            mmu::map_page_in(l0, stack_base_va + p * 4096, stack_pa + p * 4096, PageFlags::user_data());
        }
    }

    // TLB pode ter entradas velhas dos VAs recem-mapeados; I-cache tem
    // codigo novo. Sincroniza tudo.
    mmu::flush_tlb();
    unsafe {
        core::arch::asm!("dsb ish", "ic iallu", "dsb ish", "isb",
            options(nostack, preserves_flags));
    }

    Some(Loaded { entry: header.entry, stack_top: header.stack_top, l0 })
}

/// Le um arquivo do PulsarFS e o carrega como processo.
/// Retorna o pid, ou None em erro.
pub fn spawn_from_fs(name: &str) -> Option<usize> {
    let mut serial = Uart;
    let idx = crate::fs::find(name)?;
    let size = crate::fs::entry(idx).size_bytes;
    let frames = (size.div_ceil(512).div_ceil(8)) as usize; // setores -> paginas
    let frames = frames.max(1);

    let buf = frame_allocator::alloc_contig(frames)?;
    if crate::fs::read_file(idx, buf).is_none() {
        frame_allocator::free_contig(buf, frames);
        return None;
    }

    let loaded = unsafe { load_pulse(buf as *const u8) };
    frame_allocator::free_contig(buf, frames); // staging nao e mais necessario

    let Some(loaded) = loaded else {
        let _ = write!(serial, "[loader] spawn de '{}' falhou\n", name);
        return None;
    };
    let asid = alloc_asid();
    let pid = crate::process::create(loaded.entry, loaded.stack_top, loaded.l0, asid);
    let _ = write!(serial, "[loader] '{}' -> pid {} (asid {})\n", name, pid, asid);
    Some(pid)
}
