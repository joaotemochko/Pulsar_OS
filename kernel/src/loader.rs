use crate::pulse::*;
use crate::arch::mmu::{self, PageFlags};
use crate::frame_allocator;
use crate::uart::Uart;
use core::fmt::Write;
use core::sync::atomic::{AtomicU32, Ordering};

/// Resultado do carregamento: onde saltar, stack e o espaco criado.
pub struct Loaded {
    pub entry: u64,
    pub stack_top: u64,
    pub root: u64,
}

/// Alocador circular de ASIDs (1..=255; 0 e o kernel/ocioso).
static NEXT_ASID: AtomicU32 = AtomicU32::new(0);

fn alloc_asid() -> u16 {
    let a = (NEXT_ASID.fetch_add(1, Ordering::Relaxed) % 255) as u16 + 1;
    // Em caso de reuso (wrap), garante que nao ha traducoes velhas.
    mmu::flush_asid(a);
    a
}

/// Carrega um arquivo .pulse (v2) a partir de um buffer na memoria.
/// Mapeia os segmentos com W^X e a stack de usuario abaixo de `stack_top`.
pub unsafe fn load_pulse(file: *const u8, file_len: usize) -> Option<Loaded> {
    let mut serial = Uart;

    if file_len < core::mem::size_of::<PulseHeader>() {
        return None;
    }
    let header = unsafe { &*(file as *const PulseHeader) };
    if header.magic != PULSE_MAGIC {
        let _ = write!(serial, "[loader] magic invalido: {:#x}\n", header.magic);
        return None;
    }
    if header.version < 2 || header.stack_top == 0 {
        let _ = write!(serial, "[loader] .pulse v{} sem stack_top — rejeitado\n", header.version);
        return None;
    }
    if !arch_matches(header.arch) {
        let _ = write!(serial, "[loader] executavel de outra arquitetura (arch={}) — rejeitado\n",
                       header.arch);
        return None;
    }
    let table_end = header.seg_table_off as usize
        + header.seg_count as usize * core::mem::size_of::<PulseSegment>();
    if table_end > file_len {
        return None;
    }

    // Espaco de enderecos PROPRIO deste processo.
    let root = mmu::create_user_space()?;

    let seg_table = unsafe { file.add(header.seg_table_off as usize) as *const PulseSegment };
    for i in 0..header.seg_count as usize {
        let seg = unsafe { &*seg_table.add(i) };
        // Segmentos so na metade baixa de usuario, fora da regiao grafica.
        let end = seg.vaddr + seg.mem_size as u64;
        if seg.vaddr < 0x0010_0000 || end > 0x4000_0000
            || seg.file_off as usize + seg.file_size as usize > file_len
            || seg.file_size > seg.mem_size
        {
            let _ = write!(serial, "[loader] REJEITADO: segmento invalido em {:#x}\n", seg.vaddr);
            mmu::free_user_space(root);
            return None;
        }

        // W^X: segmento executavel nunca e gravavel
        let flags = if seg.flags & SEG_X != 0 { PageFlags::user_code() } else { PageFlags::user_data() };

        let mut off = 0u32;
        while off < seg.mem_size {
            let Some(frame) = frame_allocator::alloc_frame() else {
                mmu::free_user_space(root);
                return None;
            };
            let va_page = seg.vaddr + off as u64;
            unsafe {
                core::ptr::write_bytes(frame as *mut u8, 0, 4096);
                if off < seg.file_size {
                    let to_copy = core::cmp::min(seg.file_size - off, 4096) as usize;
                    let src = file.add(seg.file_off as usize + off as usize);
                    core::ptr::copy_nonoverlapping(src, frame as *mut u8, to_copy);
                }
                if !mmu::map_page_in(root, va_page, frame, flags) {
                    frame_allocator::free_frame(frame);
                    mmu::free_user_space(root);
                    return None;
                }
            }
            off += 4096;
        }
    }

    // Stack de usuario: STACK_PAGES paginas abaixo do topo
    let stack_base_va = header.stack_top - STACK_PAGES * 4096;
    for p in 0..STACK_PAGES {
        let Some(frame) = frame_allocator::alloc_frame() else {
            mmu::free_user_space(root);
            return None;
        };
        unsafe {
            core::ptr::write_bytes(frame as *mut u8, 0, 4096);
            mmu::map_page_in(root, stack_base_va + p * 4096, frame, PageFlags::user_data());
        }
    }

    // TLB pode ter entradas velhas dos VAs recem-mapeados; I-cache tem
    // codigo novo. Sincroniza tudo.
    mmu::flush_tlb();
    sync_icache();

    Some(Loaded { entry: header.entry, stack_top: header.stack_top, root })
}

fn sync_icache() {
    #[cfg(target_arch = "aarch64")]
    unsafe {
        core::arch::asm!("dsb ish", "ic ialluis", "dsb ish", "isb", options(nostack, preserves_flags));
    }
    #[cfg(target_arch = "riscv64")]
    unsafe {
        core::arch::asm!("fence.i", options(nostack, preserves_flags));
        crate::arch::sbi::remote_fence_i();
    }
}

/// Le um arquivo do sistema de arquivos e o carrega como processo.
/// Retorna o pid, ou None em erro.
pub fn spawn_from_fs(name: &str) -> Option<usize> {
    let mut serial = Uart;
    let idx = crate::fs::find(name)?;
    let size = crate::fs::entry(idx).size_bytes as usize;
    let frames = size.div_ceil(4096).max(1) + 1; // leitura e em setores inteiros

    let buf = frame_allocator::alloc_contig(frames)?;
    if crate::fs::read_file(idx, buf).is_none() {
        frame_allocator::free_contig(buf, frames);
        return None;
    }

    let loaded = unsafe { load_pulse(buf as *const u8, size) };
    frame_allocator::free_contig(buf, frames);

    let Some(loaded) = loaded else {
        let _ = write!(serial, "[loader] spawn de '{}' falhou\n", name);
        return None;
    };
    let asid = alloc_asid();
    let short = name.strip_suffix(".pulse").unwrap_or(name);
    let Some(pid) = crate::process::create(loaded.entry, loaded.stack_top, loaded.root, asid, short) else {
        mmu::free_user_space(loaded.root);
        let _ = write!(serial, "[loader] sem slots de processo para '{}'\n", name);
        return None;
    };
    let _ = write!(serial, "[loader] '{}' -> pid {} (asid {})\n", name, pid, asid);
    Some(pid)
}
