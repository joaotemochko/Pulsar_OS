use crate::pulse::*;
use crate::mmu::{self, PageFlags};
use crate::frame_allocator;
use crate::uart::Uart;
use core::fmt::Write;

/// Carrega um arquivo .pulse a partir de um ponteiro na memoria.
/// Retorna o VA do entry point para saltar, ou None em erro.
pub unsafe fn load_pulse(file: *const u8) -> Option<u64> {
    let mut serial = Uart;

    // Le o header
    let header = unsafe { &*(file as *const PulseHeader) };
    if header.magic != PULSE_MAGIC {
        let _ = write!(serial, "[loader] magic invalido: {:#x}\n", header.magic);
        return None;
    }
    let _ = write!(serial, "[loader] .pulse valido, {} segmento(s), entry={:#x}\n",
                   header.seg_count, header.entry);

    // Itera os segmentos
    let seg_table = unsafe { file.add(header.seg_table_off as usize) as *const PulseSegment };
    for i in 0..header.seg_count as usize {
        let seg = unsafe { &*seg_table.add(i) };
        let _ = write!(serial, "[loader] seg {}: vaddr={:#x} file_size={} mem_size={} flags={:#b}\n",
                       i, seg.vaddr, seg.file_size, seg.mem_size, seg.flags);

        // Escolhe as permissoes da pagina conforme as flags do segmento (W^X)
        let flags = if seg.flags & SEG_X != 0 {
            PageFlags::user_code()   // executavel em EL0
        } else {
            PageFlags::user_data()   // dados: nao-executavel
        };

        // Mapeia e copia pagina por pagina
        let mut off = 0u32;
        while off < seg.mem_size {
            // aloca um frame fisico para esta pagina
            let frame = frame_allocator::alloc_frame()?;
            let va_page = seg.vaddr + off as u64;

            unsafe {
                // mapeia o frame no VA do segmento com as permissoes certas
                mmu::map_page(va_page, frame, flags);

                // zera o frame (cuida do .bss: mem_size > file_size)
                core::ptr::write_bytes(frame as *mut u8, 0, 4096);

                // copia os bytes do arquivo que pertencem a esta pagina
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

    // sincroniza I-cache (escrevemos codigo novo)
    unsafe { core::arch::asm!("dsb ish", "ic iallu", "dsb ish", "isb", options(nostack, preserves_flags)); }

    Some(header.entry)
}