//! Memoria virtual de usuario: mmap / munmap / mprotect.
//!
//! Cada espaco de enderecamento (um por processo; as threads usam o do
//! lider) tem uma tabela ordenada de VMAs dentro da regiao [MMAP_BASE,
//! MMAP_END). As paginas sao preguicosas: mmap so registra a faixa e cada
//! pagina e alocada (zerada) no primeiro acesso, pelo page fault ou por
//! `user_ok` quando uma syscall recebe um buffer ainda nao tocado. Assim um
//! programa pode reservar gigabytes com PROT_NONE (como fazem os motores de
//! JS) sem gastar memoria.
//!
//! A tabela vive em frames contiguos que dobram de tamanho quando enchem.
//! Trava VM: SCHED -> VM -> MMU -> FRAME (ver sync.rs).
use crate::arch::mmu::{self, PageFlags};
use crate::frame_allocator;
use crate::process::{self, MAX_PROCS};
use crate::sync::SpinLock;

pub static VM: SpinLock = SpinLock::new();

/// Regiao do mmap: 128 GiB .. ~252 GiB (o Sv39 vai ate 256 GiB).
pub const MMAP_BASE: u64 = 0x20_0000_0000;
pub const MMAP_END: u64 = 0x3F_0000_0000;

pub const PROT_READ: u64 = 1;
pub const PROT_WRITE: u64 = 2;
pub const PROT_EXEC: u64 = 4;
pub const MAP_FIXED: u64 = 1 << 8;           // exatamente em `addr` (substitui o que houver)
pub const MAP_NOREPLACE: u64 = 1 << 9;       // com FIXED: falha se ja houver algo

// erros devolvidos como -errno (mesmos numeros do Linux)
pub const ENOMEM: i64 = 12;
pub const EEXIST: i64 = 17;
pub const EINVAL: i64 = 22;

#[derive(Clone, Copy, PartialEq)]
pub enum Access { Read, Write, Exec }

#[derive(Clone, Copy)]
#[repr(C)]
struct Vma {
    start: u64,
    end: u64,
    prot: u64,
    obj: u32,     // objeto de memoria compartilhada (shm.rs); 0 = anonima privada
    pgoff: u64,   // pagina do objeto correspondente a `start`
}

impl Vma {
    const fn anon(start: u64, end: u64, prot: u64) -> Vma { Vma { start, end, prot, obj: 0, pgoff: 0 } }
}

/// Cada VMA que aponta para um objeto e uma referencia a ele.
fn obj_ref(obj: u32, delta: i32) {
    if obj != 0 { crate::shm::map_ref(obj, delta); }
}

const VMA_SIZE: usize = core::mem::size_of::<Vma>();
const FIRST_PAGES: usize = 4;      // ~680 VMAs
const MAX_PAGES: usize = 256;      // ~43 mil VMAs

#[derive(Clone, Copy)]
struct Space {
    table: u64,    // PA (= VA do kernel) do vetor de VMAs; 0 se nunca usou mmap
    pages: usize,  // frames do vetor
    len: usize,
    hint: u64,     // onde a proxima busca por espaco livre comeca
}

impl Space {
    const fn empty() -> Self { Space { table: 0, pages: 0, len: 0, hint: MMAP_BASE } }

    fn cap(&self) -> usize { self.pages * 4096 / VMA_SIZE }

    fn vmas(&mut self) -> &mut [Vma] {
        if self.table == 0 { return &mut []; }
        unsafe { core::slice::from_raw_parts_mut(self.table as *mut Vma, self.len) }
    }

    /// Garante espaco para mais `extra` VMAs.
    fn reserve(&mut self, extra: usize) -> bool {
        if self.len + extra <= self.cap() { return true; }
        let pages = if self.pages == 0 { FIRST_PAGES } else { self.pages * 2 };
        if pages > MAX_PAGES { return false; }
        let Some(t) = frame_allocator::alloc_contig(pages) else { return false };
        if self.table != 0 {
            unsafe { core::ptr::copy_nonoverlapping(self.table as *const Vma, t as *mut Vma, self.len) };
            frame_allocator::free_contig(self.table, self.pages);
        }
        self.table = t;
        self.pages = pages;
        true
    }

    /// Indice da primeira VMA com end > addr.
    fn lower(&mut self, addr: u64) -> usize {
        let v = self.vmas();
        let (mut lo, mut hi) = (0, v.len());
        while lo < hi {
            let mid = (lo + hi) / 2;
            if v[mid].end <= addr { lo = mid + 1 } else { hi = mid }
        }
        lo
    }

    fn find(&mut self, addr: u64) -> Option<Vma> {
        let i = self.lower(addr);
        let v = self.vmas();
        if i < v.len() && v[i].start <= addr { Some(v[i]) } else { None }
    }

    fn insert_at(&mut self, i: usize, vma: Vma) {
        unsafe {
            let p = self.table as *mut Vma;
            core::ptr::copy(p.add(i), p.add(i + 1), self.len - i);
            p.add(i).write(vma);
        }
        self.len += 1;
    }

    fn remove_at(&mut self, i: usize, n: usize) {
        unsafe {
            let p = self.table as *mut Vma;
            core::ptr::copy(p.add(i + n), p.add(i), self.len - i - n);
        }
        self.len -= n;
    }

    /// Ha alguma VMA tocando [start, end)?
    fn overlaps(&mut self, start: u64, end: u64) -> bool {
        let i = self.lower(start);
        let v = self.vmas();
        i < v.len() && v[i].start < end
    }

    /// [start, end) inteiro coberto por VMAs, sem buracos?
    fn covered(&mut self, start: u64, end: u64) -> bool {
        let mut i = self.lower(start);
        let mut at = start;
        let v = self.vmas();
        while at < end {
            if i >= v.len() || v[i].start > at { return false; }
            at = v[i].end;
            i += 1;
        }
        true
    }

    /// Tira [start, end) da tabela (cortando ou dividindo VMAs). Precisa
    /// de espaco para uma VMA a mais (divisao): chame `reserve(1)` antes.
    fn carve(&mut self, start: u64, end: u64) {
        let mut i = self.lower(start);
        while i < self.len {
            let v = self.vmas()[i];
            if v.start >= end { break; }
            if v.start < start && v.end > end {
                // a faixa esta no meio: divide em duas
                self.vmas()[i].end = start;
                let pgoff = v.pgoff + (end - v.start) / 4096;
                self.insert_at(i + 1, Vma { start: end, end: v.end, prot: v.prot, obj: v.obj, pgoff });
                obj_ref(v.obj, 1);
                return;
            }
            if v.start < start {
                self.vmas()[i].end = start;
                i += 1;
            } else if v.end > end {
                self.vmas()[i].start = end;
                self.vmas()[i].pgoff = v.pgoff + (end - v.start) / 4096;
                return;
            } else {
                self.remove_at(i, 1);
                obj_ref(v.obj, -1);
            }
        }
    }

    /// Insere [start, end) anonima (livre na tabela) e funde com vizinhas.
    fn add(&mut self, start: u64, end: u64, prot: u64) {
        self.add_vma(Vma::anon(start, end, prot));
    }

    /// Insere uma VMA (a referencia ao objeto, se houver, ja e dela) e
    /// funde com vizinhas compativeis.
    fn add_vma(&mut self, v: Vma) {
        let i = self.lower(v.start);
        self.insert_at(i, v);
        let mut i = i;
        if i > 0 && self.mergeable(i - 1, i) {
            let cur = self.vmas()[i];
            self.vmas()[i - 1].end = cur.end;
            self.remove_at(i, 1);
            obj_ref(cur.obj, -1);
            i -= 1;
        }
        if i + 1 < self.len && self.mergeable(i, i + 1) {
            let next = self.vmas()[i + 1];
            self.vmas()[i].end = next.end;
            self.remove_at(i + 1, 1);
            obj_ref(next.obj, -1);
        }
    }

    /// `a` e `b` (vizinhas) podem virar uma so VMA?
    fn mergeable(&mut self, a: usize, b: usize) -> bool {
        let (x, y) = (self.vmas()[a], self.vmas()[b]);
        x.end == y.start && x.prot == y.prot && x.obj == y.obj
            && (x.obj == 0 || x.pgoff + (x.end - x.start) / 4096 == y.pgoff)
    }

    /// Divide a VMA que contem `addr` (estritamente dentro) em duas.
    /// Precisa de espaco para uma VMA a mais.
    fn split_at(&mut self, addr: u64) {
        let i = self.lower(addr);
        if i >= self.len { return; }
        let v = self.vmas()[i];
        if v.start >= addr || v.end <= addr { return; }
        self.vmas()[i].end = addr;
        let pgoff = v.pgoff + (addr - v.start) / 4096;
        self.insert_at(i + 1, Vma { start: addr, end: v.end, prot: v.prot, obj: v.obj, pgoff });
        obj_ref(v.obj, 1);
    }

    /// Primeiro buraco de `len` bytes a partir de `from` (next-fit).
    fn gap_from(&mut self, from: u64, len: u64) -> Option<u64> {
        let mut cand = from.max(MMAP_BASE);
        let mut i = self.lower(cand);
        let v = self.vmas();
        loop {
            let end = cand.checked_add(len)?;
            if end > MMAP_END { return None; }
            if i >= v.len() || v[i].start >= end { return Some(cand); }
            cand = cand.max(v[i].end);
            i += 1;
        }
    }

    fn gap(&mut self, len: u64) -> Option<u64> {
        let h = self.hint;
        self.gap_from(h, len).or_else(|| self.gap_from(MMAP_BASE, len))
    }
}

static mut SPACES: [Space; MAX_PROCS] = [Space::empty(); MAX_PROCS];

fn space(leader: usize) -> &'static mut Space {
    unsafe { &mut (*(&raw mut SPACES))[leader] }
}

#[inline]
fn page_up(x: u64) -> Option<u64> { x.checked_add(4095).map(|v| v & !4095) }

/// SYS_MMAP: memoria anonima zerada. Retorna o endereco ou -errno.
pub fn mmap(addr: u64, len: u64, prot: u64, flags: u64) -> i64 {
    mmap_obj(addr, len, prot, flags, 0, 0)
}

/// mmap de um objeto compartilhado (`obj` != 0, a partir da pagina
/// `pgoff`). A VMA nova leva uma referencia ao objeto (somada aqui).
pub fn mmap_obj(addr: u64, len: u64, prot: u64, flags: u64, obj: u32, pgoff: u64) -> i64 {
    let Some(len) = page_up(len) else { return -ENOMEM };
    if len == 0 || prot & !7 != 0 { return -EINVAL; }
    let Some((l, root, asid)) = process::current_space() else { return -EINVAL };
    let _g = VM.guard();
    let sp = space(l);
    if !sp.reserve(2) { return -ENOMEM; }
    let start = if flags & MAP_FIXED != 0 {
        if addr & 4095 != 0 { return -EINVAL; }
        let Some(end) = addr.checked_add(len) else { return -ENOMEM };
        if addr < MMAP_BASE || end > MMAP_END { return -ENOMEM; }
        if sp.overlaps(addr, end) {
            if flags & MAP_NOREPLACE != 0 { return -EEXIST; }
            sp.carve(addr, end);
            mmu::unmap_range(root, addr, end);
            mmu::flush_asid(asid);
        }
        addr
    } else {
        // a dica so vale se a faixa estiver livre; senao, qualquer lugar
        let hinted = addr & !4095;
        let free_hint = hinted >= MMAP_BASE && hinted.checked_add(len).is_some_and(|e| e <= MMAP_END)
            && !sp.overlaps(hinted, hinted + len);
        if free_hint { hinted } else {
            let Some(a) = sp.gap(len) else { return -ENOMEM };
            sp.hint = a + len;
            a
        }
    };
    obj_ref(obj, 1);
    sp.add_vma(Vma { start, end: start + len, prot, obj, pgoff });
    start as i64
}

/// SYS_MUNMAP: libera [addr, addr+len). Faixas nao mapeadas sao ignoradas.
pub fn munmap(addr: u64, len: u64) -> i64 {
    if addr & 4095 != 0 { return -EINVAL; }
    let Some(len) = page_up(len) else { return -EINVAL };
    let Some(end) = addr.checked_add(len) else { return -EINVAL };
    if len == 0 || addr < MMAP_BASE || end > MMAP_END { return -EINVAL; }
    let Some((l, root, asid)) = process::current_space() else { return -EINVAL };
    let _g = VM.guard();
    let sp = space(l);
    if !sp.reserve(1) { return -ENOMEM; }
    sp.carve(addr, end);
    mmu::unmap_range(root, addr, end);
    mmu::flush_asid(asid);
    0
}

/// SYS_MPROTECT: troca a protecao de [addr, addr+len), que precisa estar
/// todo mapeado.
pub fn mprotect(addr: u64, len: u64, prot: u64) -> i64 {
    if addr & 4095 != 0 || prot & !7 != 0 { return -EINVAL; }
    let Some(len) = page_up(len) else { return -ENOMEM };
    let Some(end) = addr.checked_add(len) else { return -ENOMEM };
    if len == 0 { return 0; }
    let Some((l, root, asid)) = process::current_space() else { return -EINVAL };
    let _g = VM.guard();
    let sp = space(l);
    if !sp.covered(addr, end) { return -ENOMEM; }
    if !sp.reserve(2) { return -ENOMEM; }
    // divide nas bordas e troca a protecao das VMAs de dentro (cada uma
    // guarda o seu objeto e deslocamento)
    sp.split_at(addr);
    sp.split_at(end);
    let mut i = sp.lower(addr);
    while i < sp.len && sp.vmas()[i].start < end {
        sp.vmas()[i].prot = prot;
        i += 1;
    }
    mmu::protect_range(root, addr, end, PageFlags::user_prot(prot));
    mmu::flush_asid(asid);
    0
}

/// Page fault (ou buffer de syscall) em `addr`: se cai numa VMA que permite
/// o acesso, aloca a pagina e retorna true. False = acesso invalido.
pub fn fault(addr: u64, access: Access) -> bool {
    let Some((l, root, _)) = process::current_space() else { return false };
    let _g = VM.guard();
    let Some(v) = space(l).find(addr) else { return false };
    let ok = match access {
        Access::Read => v.prot & (PROT_READ | PROT_WRITE | PROT_EXEC) != 0,
        Access::Write => v.prot & PROT_WRITE != 0,
        Access::Exec => v.prot & PROT_EXEC != 0,
    };
    if !ok { return false; }
    let page = addr & !4095;
    let flags = PageFlags::user_prot(v.prot);
    if mmu::is_mapped(root, page) {
        // outra thread chegou antes: so garante as permissoes
        mmu::protect_range(root, page, page + 4096, flags);
    } else if v.obj != 0 {
        // pagina de um objeto compartilhado: a mesma em todo processo
        let pg = v.pgoff + (page - v.start) / 4096;
        let Some(frame) = crate::shm::page(v.obj, pg as usize, true) else { return false };
        if !unsafe { mmu::map_page_in(root, page, frame, flags.shared()) } { return false; }
    } else {
        if page < MMAP_BASE || page >= MMAP_END { return false; }
        let Some(frame) = frame_allocator::alloc_frame() else { return false };
        unsafe {
            core::ptr::write_bytes(frame as *mut u8, 0, 4096);
            if !mmu::map_page_in(root, page, frame, flags) {
                frame_allocator::free_frame(frame);
                return false;
            }
        }
    }
    mmu::flush_tlb();
    true
}

/// madvise(DONTNEED): devolve as paginas de [addr, addr+len); a faixa
/// continua mapeada e volta zerada no proximo acesso.
pub fn discard(addr: u64, len: u64) -> i64 {
    if addr & 4095 != 0 { return -EINVAL; }
    let Some(len) = page_up(len) else { return -EINVAL };
    let start = addr.max(MMAP_BASE);
    let end = addr.saturating_add(len).min(MMAP_END);
    if start >= end { return 0; }
    let Some((_, root, asid)) = process::current_space() else { return -EINVAL };
    let _g = VM.guard();
    mmu::unmap_range(root, start, end);
    mmu::flush_asid(asid);
    0
}

/// Texto de /proc/self/maps: as VMAs do espaco corrente.
pub fn maps_text(out: &mut [u8]) -> usize {
    use core::fmt::Write;
    let Some((l, _, _)) = process::current_space() else { return 0 };
    let mut t = crate::file::Text { buf: out, len: 0 };
    let _g = VM.guard();
    for v in space(l).vmas().iter() {
        let p = |b: u64, c: char| if v.prot & b != 0 { c } else { '-' };
        let name = if v.end == MMAP_END { "[stack]" } else if v.obj != 0 { "/memfd (deleted)" } else { "" };
        let _ = write!(t, "{:x}-{:x} {}{}{}{} {:08x} 00:00 0          {}\n",
            v.start, v.end, p(PROT_READ, 'r'), p(PROT_WRITE, 'w'), p(PROT_EXEC, 'x'),
            if v.obj != 0 { 's' } else { 'p' }, v.pgoff * 4096, name);
    }
    t.len
}

/// Registra a faixa [start, end) com `prot` no espaco do lider `l` (pilha
/// de um processo recem-criado, antes de ele rodar). Faixa precisa estar livre.
pub fn add_region(l: usize, start: u64, end: u64, prot: u64) -> bool {
    let _g = VM.guard();
    let sp = space(l);
    if !sp.reserve(1) || sp.overlaps(start, end) { return false; }
    sp.add(start, end, prot);
    true
}

/// fork: o espaco `dst` recebe uma copia das VMAs de `src`. Chamado com
/// SCHED presa (SCHED -> VM).
pub fn clone_space(src: usize, dst: usize) {
    let _g = VM.guard();
    let s = *space(src);
    let d = space(dst);
    *d = Space::empty();
    d.hint = s.hint;
    if s.table == 0 || s.len == 0 { return; }
    let Some(t) = frame_allocator::alloc_contig(s.pages) else { return };
    unsafe { core::ptr::copy_nonoverlapping(s.table as *const Vma, t as *mut Vma, s.len) };
    d.table = t;
    d.pages = s.pages;
    d.len = s.len;
    // o filho tambem referencia os objetos compartilhados
    for v in d.vmas().iter() { obj_ref(v.obj, 1); }
}

/// Como add_region, mas por cima do que ja houver na faixa (segmentos de
/// um ELF que dividem uma pagina). Fora da regiao do mmap tambem vale:
/// assim mprotect/munmap funcionam nos segmentos carregados pelo kernel.
pub fn add_region_over(l: usize, start: u64, end: u64, prot: u64) {
    let _g = VM.guard();
    let sp = space(l);
    if start >= end || !sp.reserve(2) { return; }
    sp.carve(start, end);
    sp.add(start, end, prot);
}

/// O espaco do lider `l` acabou: esquece as VMAs (as paginas saem com as
/// tabelas, em free_user_space). Chamado com SCHED presa.
pub fn release(l: usize) {
    let _g = VM.guard();
    let sp = space(l);
    for v in sp.vmas().iter() { obj_ref(v.obj, -1); }
    if sp.table != 0 { frame_allocator::free_contig(sp.table, sp.pages); }
    *sp = Space::empty();
}
