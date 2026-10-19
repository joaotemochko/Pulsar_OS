//! Execucao de programas por caminho: binarios ELF estaticos (ABI Linux) e
//! apps .pulse (ABI Pulsar), com argumentos e ambiente.
//!
//! Para ELF, a pilha inicial segue o formato do Linux, o que a libc (musl)
//! espera encontrar em `_start`:
//!
//!   sp -> argc | argv[0..argc] | 0 | envp[..] | 0 | auxv (tipo, valor).. | AT_NULL
//!         ... strings de argv/envp, AT_RANDOM, AT_EXECFN ... | STACK_TOP
//!
//! A pilha e uma VMA de 8 MiB logo abaixo do topo da regiao de mmap; so as
//! paginas com os argumentos sao criadas agora, o resto aparece sob demanda.
use crate::arch::mmu::{self, PageFlags};
use crate::ext2::{ENOSPC, Res};
use crate::frame_allocator;
use crate::process;
use crate::uart::Uart;
use core::fmt::Write;

const ENOEXEC: i64 = 8;
const E2BIG: i64 = 7;
const EAGAIN: i64 = 11;

pub const STACK_TOP: u64 = crate::vm::MMAP_END;
pub const STACK_SIZE: u64 = 8 << 20;
/// Base de carga de executaveis ELF independentes de posicao (static-pie).
const PIE_BASE: u64 = 0x1000_0000;
/// Faixa aceita para segmentos ELF (abaixo da regiao grafica).
const USER_LO: u64 = 0x1_0000;
const USER_HI: u64 = 0x4000_0000;
/// Maior executavel aceito.
const MAX_IMAGE: u64 = 1 << 30;
/// Espaco maximo para argv + envp + auxv.
const ARG_MAX: usize = 128 * 1024;

#[cfg(target_arch = "aarch64")]
const EM_MACHINE: u16 = 183;
#[cfg(target_arch = "riscv64")]
const EM_MACHINE: u16 = 243;

#[cfg(target_arch = "aarch64")]
const HWCAP: u64 = 0b11; // FP | ASIMD
#[cfg(target_arch = "riscv64")]
const HWCAP: u64 = (1 << (b'i' - b'a')) | (1 << (b'm' - b'a')) | (1 << (b'a' - b'a'))
    | (1 << (b'f' - b'a')) | (1 << (b'd' - b'a')) | (1 << (b'c' - b'a'));

const PT_LOAD: u32 = 1;
const PT_INTERP: u32 = 3;
const PT_PHDR: u32 = 6;

/// Argumentos ja copiados para o kernel: strings separadas por NUL.
pub struct Args<'a> {
    pub argv: &'a [u8],
    pub argc: usize,
    pub envp: &'a [u8],
    pub envc: usize,
}

fn rd16(b: &[u8], o: usize) -> u16 { u16::from_le_bytes([b[o], b[o + 1]]) }
fn rd32(b: &[u8], o: usize) -> u32 { u32::from_le_bytes(b[o..o + 4].try_into().unwrap()) }
fn rd64(b: &[u8], o: usize) -> u64 { u64::from_le_bytes(b[o..o + 8].try_into().unwrap()) }

struct Image {
    entry: u64,      // onde a execucao comeca (o interpretador, se houver)
    phdr: u64,       // program headers do programa (AT_PHDR)
    phnum: u64,
    root: u64,
    base: u64,       // base do interpretador (AT_BASE; 0 = estatico)
    prog_entry: u64, // entrada do programa (AT_ENTRY)
    segs: Segs,      // segmentos carregados, viram VMAs (mprotect do RELRO etc.)
}

/// Faixas (inicio, fim, prot) alinhadas a pagina dos segmentos carregados.
#[derive(Clone, Copy)]
pub struct Segs { pub r: [(u64, u64, u64); 24], pub n: usize }
impl Segs {
    const EMPTY: Segs = Segs { r: [(0, 0, 0); 24], n: 0 };
    fn push(&mut self, va: u64, len: u64, prot: u64) {
        if self.n < self.r.len() {
            self.r[self.n] = (va & !4095, (va + len + 4095) & !4095, prot);
            self.n += 1;
        }
    }
    /// Registra as faixas como VMAs do slot (com SCHED presa, no setup).
    pub fn register(&self, slot: usize) {
        for &(a, b, prot) in &self.r[..self.n] { crate::vm::add_region_over(slot, a, b, prot); }
    }
}

/// Base de carga do ligador dinamico (PT_INTERP, ex.: ld-musl).
const INTERP_BASE: u64 = 0x3000_0000;

/// Copia `data` para o espaco `root` a partir de `va`, criando paginas
/// (zeradas) onde faltar e somando `prot` as que ja existem.
fn copy_into(root: u64, va: u64, data: &[u8], prot: u64, mem_len: u64) -> bool {
    let end = va + mem_len;
    let mut page = va & !4095;
    while page < end {
        let pa = match mmu::translate_user(root, page, false) {
            Some(pa) => {
                // pagina dividida entre dois segmentos: une as permissoes
                mmu::protect_range(root, page, page + 4096, PageFlags::user_prot(prot | 1 | 2));
                pa
            }
            None => {
                let Some(f) = frame_allocator::alloc_frame() else { return false };
                unsafe { core::ptr::write_bytes(f as *mut u8, 0, 4096) };
                if !unsafe { mmu::map_page_in(root, page, f, PageFlags::user_prot(prot)) } {
                    frame_allocator::free_frame(f);
                    return false;
                }
                f
            }
        };
        // parte de `data` que cai nesta pagina
        let lo = page.max(va);
        let hi = (page + 4096).min(va + data.len() as u64);
        if lo < hi {
            let src = &data[(lo - va) as usize..(hi - va) as usize];
            unsafe {
                core::ptr::copy_nonoverlapping(src.as_ptr(), (pa + (lo - page)) as *mut u8, src.len());
            }
        }
        page += 4096;
    }
    true
}

/// Mapeia os PT_LOAD de `file` em `root` (ET_DYN em `dyn_base`).
/// Retorna (entrada, AT_PHDR, phnum, faixa do caminho PT_INTERP no arquivo).
fn map_elf(root: u64, file: &[u8], dyn_base: u64, segs: &mut Segs) -> Res<(u64, u64, u64, Option<(usize, usize)>)> {
    if file.len() < 64 || &file[..4] != b"\x7fELF" || file[4] != 2 || file[5] != 1 {
        return Err(ENOEXEC);
    }
    let etype = rd16(file, 16);
    if rd16(file, 18) != EM_MACHINE || (etype != 2 && etype != 3) { return Err(ENOEXEC); }
    let phoff = rd64(file, 32) as usize;
    let phent = rd16(file, 54) as usize;
    let phnum = rd16(file, 56) as usize;
    if phent != 56 || phoff + phnum * phent > file.len() { return Err(ENOEXEC); }
    let base = if etype == 3 { dyn_base } else { 0 };
    let ph = |i: usize| &file[phoff + i * phent..phoff + (i + 1) * phent];
    let mut phdr_va = 0;
    let mut first_load = None;
    let mut interp = None;
    for i in 0..phnum {
        let p = ph(i);
        let ptype = rd32(p, 0);
        if ptype == PT_INTERP {
            let (off, sz) = (rd64(p, 8) as usize, rd64(p, 32) as usize);
            if off.saturating_add(sz) > file.len() || sz < 2 || sz > 256 { return Err(ENOEXEC); }
            interp = Some((off, off + sz - 1));   // sem o NUL final
        }
        if ptype == PT_PHDR { phdr_va = rd64(p, 16) + base; }
        if ptype != PT_LOAD { continue; }
        let flags = rd32(p, 4);
        let off = rd64(p, 8) as usize;
        let va = rd64(p, 16) + base;
        let filesz = rd64(p, 32) as usize;
        let memsz = rd64(p, 40);
        if memsz == 0 { continue; }
        if va < USER_LO || va.checked_add(memsz).is_none_or(|e| e > USER_HI)
            || off.checked_add(filesz).is_none_or(|e| e > file.len()) || filesz as u64 > memsz
        {
            return Err(ENOEXEC);
        }
        if first_load.is_none() { first_load = Some((va, off)); }
        // p_flags: X=1 W=2 R=4  ->  prot: R=1 W=2 X=4
        let prot = (if flags & 4 != 0 { 1 } else { 0 }) | (if flags & 2 != 0 { 2 } else { 0 })
            | (if flags & 1 != 0 { 4 } else { 0 });
        if !copy_into(root, va, &file[off..off + filesz], prot, memsz) { return Err(ENOSPC); }
        segs.push(va, memsz, prot);
    }
    // sem PT_PHDR: os cabecalhos estao no primeiro segmento carregado
    if phdr_va == 0 {
        if let Some((va, off)) = first_load {
            if phoff >= off { phdr_va = va + (phoff - off) as u64; }
        }
    }
    Ok((rd64(file, 24) + base, phdr_va, phnum as u64, interp))
}

/// Carrega um ELF. Se ele pede um interpretador (PT_INTERP, binario
/// dinamico), carrega tambem o interpretador, que comeca a execucao e
/// recebe o programa pelo auxv (AT_BASE, AT_ENTRY, AT_PHDR).
fn load_elf(file: &[u8]) -> Res<Image> {
    let root = mmu::create_user_space().ok_or(ENOSPC)?;
    let fail = |e| { mmu::free_user_space(root); Err(e) };
    let mut segs = Segs::EMPTY;
    let (entry, phdr, phnum, interp) = match map_elf(root, file, PIE_BASE, &mut segs) {
        Ok(v) => v,
        Err(e) => return fail(e),
    };
    let mut img = Image { entry, phdr, phnum, root, base: 0, prog_entry: entry, segs: Segs::EMPTY };
    if let Some((a, b)) = interp {
        let (buf, frames, size) = match crate::file::read_whole(crate::ext2::ROOT_INO, &file[a..b], MAX_IMAGE) {
            Ok(v) => v,
            Err(e) => return fail(e),
        };
        let ifile = unsafe { core::slice::from_raw_parts(buf as *const u8, size as usize) };
        let r = map_elf(root, ifile, INTERP_BASE, &mut segs);
        frame_allocator::free_contig(buf, frames);
        match r {
            Ok((ientry, _, _, None)) => { img.entry = ientry; img.base = INTERP_BASE; }
            Ok(_) => return fail(ENOEXEC),          // interpretador que pede outro
            Err(e) => return fail(e),
        }
    }
    img.segs = segs;
    mmu::flush_tlb();
    crate::loader::sync_icache();
    Ok(img)
}

/// Monta a pilha inicial (argc, argv, envp, auxv e strings) no espaco
/// `root`, terminando em STACK_TOP. Retorna o sp inicial.
fn build_stack(root: u64, img: &Image, path: &[u8], args: &Args) -> Res<u64> {
    let total = path.len() + 1 + args.argv.len() + args.envp.len() + 16
        + (args.argc + args.envc + 3 + 2 * 20) * 8 + 64;
    if total > ARG_MAX { return Err(E2BIG); }
    let pages = ARG_MAX / 4096;
    let buf = frame_allocator::alloc_contig(pages).ok_or(ENOSPC)?;
    let img_buf = unsafe { core::slice::from_raw_parts_mut(buf as *mut u8, ARG_MAX) };
    // o buffer representa [STACK_TOP - ARG_MAX, STACK_TOP)
    let bottom = STACK_TOP - ARG_MAX as u64;
    let mut top = ARG_MAX;
    let mut push = |data: &[u8]| -> u64 {
        top -= data.len();
        img_buf[top..top + data.len()].copy_from_slice(data);
        bottom + top as u64
    };
    push(&[0u8; 8]);                           // marcador de fim
    let execfn = { push(&[0]); push(path) };
    let mut random = [0u8; 16];
    crate::rng::fill(&mut random);
    let at_random = push(&random);
    // strings de envp e argv (na ordem inversa, para os ponteiros saberem onde)
    let mut env_ptrs = [0u64; 512];
    let mut arg_ptrs = [0u64; 512];
    if args.argc > 512 || args.envc > 512 { frame_allocator::free_contig(buf, pages); return Err(E2BIG); }
    fn split(block: &[u8], n: usize, out: &mut [u64], push: &mut dyn FnMut(&[u8]) -> u64) {
        // empilha cada string com seu NUL; guarda o endereco
        let mut starts = [0usize; 513];
        let mut k = 0;
        let mut i = 0;
        while k < n && i <= block.len() {
            starts[k] = i;
            while i < block.len() && block[i] != 0 { i += 1; }
            i += 1;
            k += 1;
        }
        for j in (0..k).rev() {
            let s = starts[j];
            let e = block[s..].iter().position(|&c| c == 0).map_or(block.len(), |p| s + p);
            push(&[0]);
            out[j] = push(&block[s..e]);
        }
    }
    split(args.envp, args.envc, &mut env_ptrs, &mut push);
    split(args.argv, args.argc, &mut arg_ptrs, &mut push);
    // vetores, alinhados a 16
    let auxv: [(u64, u64); 16] = [
        (3, img.phdr), (4, 56), (5, img.phnum), (6, 4096), (7, img.base), (8, 0), (9, img.prog_entry),
        (11, 0), (12, 0), (13, 0), (14, 0), (16, HWCAP), (17, 100), (23, 0),
        (25, at_random), (31, execfn),
    ];
    let words = 1 + args.argc + 1 + args.envc + 1 + 2 * (auxv.len() + 1);
    top &= !15;
    top -= words * 8;
    top &= !15;
    let sp = bottom + top as u64;
    let mut w = top;
    let mut put = |v: u64| { img_buf[w..w + 8].copy_from_slice(&v.to_le_bytes()); w += 8; };
    put(args.argc as u64);
    for i in 0..args.argc { put(arg_ptrs[i]); }
    put(0);
    for i in 0..args.envc { put(env_ptrs[i]); }
    put(0);
    for &(t, v) in auxv.iter() { put(t); put(v); }
    put(0);
    put(0);
    // copia [sp & !4095, STACK_TOP) para o processo
    let first = sp & !4095;
    let ok = copy_into(root, first, &img_buf[(first - bottom) as usize..], 3, STACK_TOP - first);
    frame_allocator::free_contig(buf, pages);
    if ok { Ok(sp) } else { Err(ENOSPC) }
}

fn basename(path: &[u8]) -> &str {
    let s = path.iter().rposition(|&c| c == b'/').map_or(path, |i| &path[i + 1..]);
    let s = s.strip_suffix(b".pulse").unwrap_or(s);
    core::str::from_utf8(s).unwrap_or("?")
}

/// .pulse v3 da personalidade Linux (tools/elf2pulse.py): os segmentos de
/// um ELF estatico + onde estao os program headers. Carregado como o ELF.
fn load_pulse_linux(file: &[u8]) -> Res<Image> {
    if file.len() < 48 || rd32(file, 0) != crate::pulse::PULSE_MAGIC || rd16(file, 4) != 3 {
        return Err(ENOEXEC);
    }
    let segc = rd16(file, 6) as usize;
    let entry = rd64(file, 8);
    let table = rd32(file, 16) as usize;
    if !crate::pulse::arch_matches(rd32(file, 20)) || rd64(file, 24) != 0 || rd16(file, 42) != 1 {
        return Err(ENOEXEC);
    }
    let (phdr, phnum) = (rd64(file, 32), rd16(file, 40) as u64);
    if table + segc * 24 > file.len() { return Err(ENOEXEC); }
    let root = mmu::create_user_space().ok_or(ENOSPC)?;
    let mut segs = Segs::EMPTY;
    for i in 0..segc {
        let o = table + i * 24;
        let (off, fsz, va, msz, fl) = (rd32(file, o) as usize, rd32(file, o + 4) as usize,
                                       rd64(file, o + 8), rd32(file, o + 16) as u64, rd32(file, o + 20) as u64);
        if va < USER_LO || va.saturating_add(msz) > USER_HI || off.saturating_add(fsz) > file.len()
            || fsz as u64 > msz || !copy_into(root, va, &file[off..off + fsz], fl & 7, msz)
        {
            mmu::free_user_space(root);
            return Err(ENOEXEC);
        }
        segs.push(va, msz, fl & 7);
    }
    mmu::flush_tlb();
    crate::loader::sync_icache();
    Ok(Image { entry, phdr, phnum, root, base: 0, prog_entry: entry, segs })
}

/// Carrega qualquer executavel: ELF (ABI Linux), .pulse v3 (ABI Linux,
/// convertido de um ELF) ou .pulse nativo. Retorna (raiz, entrada, sp, ABI).
fn load_image(file: &[u8], path: &[u8], args: &Args) -> Res<(u64, u64, u64, u8, Segs)> {
    let linux = if file.len() >= 4 && &file[..4] == b"\x7fELF" {
        Some(load_elf(file)?)
    } else if file.len() >= 6 && rd16(file, 4) == 3 {
        Some(load_pulse_linux(file)?)
    } else {
        None
    };
    if let Some(img) = &linux { crate::signal::map_sigpage(img.root); }
    match linux {
        Some(img) => match build_stack(img.root, &img, path, args) {
            Ok(sp) => Ok((img.root, img.entry, sp, process::ABI_LINUX, img.segs)),
            Err(e) => { mmu::free_user_space(img.root); Err(e) }
        },
        None => match unsafe { crate::loader::load_pulse(file.as_ptr(), file.len()) } {
            Some(l) => Ok((l.root, l.entry, l.stack_top, process::ABI_PULSAR, Segs::EMPTY)),
            None => Err(ENOEXEC),
        },
    }
}

/// Executa `path` como um novo processo filho do corrente. Retorna o pid.
pub fn spawn(at: u32, path: &[u8], args: &Args) -> Res<usize> {
    let (buf, frames, size) = crate::file::read_whole(at, path, MAX_IMAGE)?;
    let file = unsafe { core::slice::from_raw_parts(buf as *const u8, size as usize) };
    let loaded = load_image(file, path, args);
    frame_allocator::free_contig(buf, frames);
    let (root, entry, sp, abi, segs) = loaded?;
    // o filho herda o diretorio corrente e os descritores (menos os
    // close-on-exec), como num fork + exec
    let parent = process::current_leader();
    let cwd = parent.map_or(crate::ext2::ROOT_INO, crate::file::cwd_of);
    let asid = crate::loader::alloc_asid();
    let mut cbuf = [0u8; crate::ext2::PATH_MAX];
    let exe = canon_exe(path, &mut cbuf);
    let pid = process::create_ex(entry, sp, root, asid, basename(path), abi, &mut |slot| {
        if abi == process::ABI_LINUX {
            // a tabela de VMAs do slot novo esta vazia: registrar a pilha so
            // falha sem memoria (e entao so as paginas ja criadas existem)
            crate::vm::add_region(slot, STACK_TOP - STACK_SIZE, STACK_TOP, 3);
            segs.register(slot);
        }
        crate::file::set_cwd_of(slot, cwd);
        if let Some(p) = parent {
            crate::file::clone_table(p, slot);
            crate::file::exec_cleanup(slot);
        }
        process::set_exe_path(slot, exe);
        process::set_cmdline(slot, args.argv);
    });
    crate::file::flush_wakes();     // pontas de pipe fechadas pelo close-on-exec
    let Some(pid) = pid else { mmu::free_user_space(root); return Err(EAGAIN) };
    let _ = write!(Uart, "[exec] '{}' -> pid {}\n", core::str::from_utf8(path).unwrap_or("?"), pid);
    Ok(pid)
}

/// /proc/self/exe: o caminho canonico do executavel (links resolvidos),
/// como no Linux. O Firefox acha os proprios arquivos por ele quando aberto
/// pelo atalho /usr/bin/firefox -> /opt/firefox/firefox.
/// (Calculado antes de criar o processo: le o disco, o que nao pode ser
/// feito com a trava do escalonador.)
fn canon_exe<'a>(path: &'a [u8], buf: &'a mut [u8; crate::ext2::PATH_MAX]) -> &'a [u8] {
    match crate::file::realpath(path, buf) {
        Ok(n) => &buf[..n],
        Err(_) => path,
    }
}

/// execve(): carrega `path` e troca a imagem do processo corrente. So
/// retorna em caso de erro (com o processo intacto).
pub fn execve(frame: *mut crate::arch::Context, at: u32, path: &[u8], args: &Args) -> Res<()> {
    let (buf, frames, size) = crate::file::read_whole(at, path, MAX_IMAGE)?;
    let file = unsafe { core::slice::from_raw_parts(buf as *const u8, size as usize) };
    let name = basename(path);
    let loaded = load_image(file, path, args);
    frame_allocator::free_contig(buf, frames);
    let (root, entry, sp, abi, segs) = loaded?;
    let mut cbuf = [0u8; crate::ext2::PATH_MAX];
    let exe = canon_exe(path, &mut cbuf);
    let asid = crate::loader::alloc_asid();
    let ok = unsafe {
        process::exec_replace(frame, root, asid, entry, sp, abi, name, &mut |slot| {
            if abi == process::ABI_LINUX {
                crate::vm::add_region(slot, STACK_TOP - STACK_SIZE, STACK_TOP, 3);
                segs.register(slot);
            }
            process::set_exe_path(slot, exe);
            process::set_cmdline(slot, args.argv);
        })
    };
    if !ok {
        mmu::free_user_space(root);
        return Err(EAGAIN);
    }
    let _ = write!(Uart, "[exec] execve '{}' (pid {})\n", core::str::from_utf8(path).unwrap_or("?"), process::current_pid());
    Ok(())
}
