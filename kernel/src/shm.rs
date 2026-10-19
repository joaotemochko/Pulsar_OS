//! Objetos de memoria compartilhada: memfd_create, shm_open (/dev/shm) e
//! mmap(MAP_SHARED | MAP_ANONYMOUS).
//!
//! Um objeto e uma lista de paginas fisicas (criadas no primeiro uso) com
//! um tamanho. Ele e referenciado por descritores (`fd_refs`) e por VMAs
//! (`map_refs`, mantidas pelo vm.rs); as paginas so voltam ao alocador
//! quando nao sobra referencia nenhuma e o objeto nao tem nome. Nas tabelas
//! de pagina elas levam o bit de software "compartilhada", entao desmapear
//! ou destruir um espaco nao as libera e o fork as compartilha.
//!
//! Trava SHM: VM -> SHM -> FRAME e FS -> SHM.
use crate::frame_allocator;
use crate::sync::SpinLock;

pub static SHM: SpinLock = SpinLock::new();

const MAX_OBJS: usize = 512;  // memfd/shm (o Firefox usa muitos)
const NAME_MAX: usize = 64;
/// Tamanho maximo de um objeto (1 GiB).
pub const MAX_SIZE: u64 = 1 << 30;

const ENOMEM: i64 = 12;
const EFBIG: i64 = 27;

#[derive(Clone, Copy)]
struct Obj {
    used: bool,
    size: u64,
    index: u64,        // vetor de PAs (u64 por pagina; 0 = ainda nao criada)
    index_frames: usize,
    cap: usize,        // entradas do vetor
    fd_refs: u32,
    map_refs: u32,
    named: bool,
    name: [u8; NAME_MAX],
    name_len: usize,
}

const EMPTY: Obj = Obj { used: false, size: 0, index: 0, index_frames: 0, cap: 0, fd_refs: 0, map_refs: 0,
                         named: false, name: [0; NAME_MAX], name_len: 0 };
static mut OBJS: [Obj; MAX_OBJS] = [EMPTY; MAX_OBJS];

fn objs() -> &'static mut [Obj; MAX_OBJS] {
    unsafe { &mut *(&raw mut OBJS) }
}

/// Ids publicos comecam em 1 (0 = "sem objeto" nas VMAs).
fn obj(id: u32) -> Option<&'static mut Obj> {
    objs().get_mut((id as usize).checked_sub(1)?).filter(|o| o.used)
}

fn page_slot(o: &Obj, pg: usize) -> *mut u64 {
    (o.index + pg as u64 * 8) as *mut u64
}

/// Libera tudo se ninguem mais usa o objeto. (SHM presa)
fn maybe_free(id: u32) {
    let Some(o) = obj(id) else { return };
    if o.fd_refs > 0 || o.map_refs > 0 || o.named { return; }
    for pg in 0..o.cap {
        let pa = unsafe { *page_slot(o, pg) };
        if pa != 0 { frame_allocator::free_frame(pa); }
    }
    if o.index != 0 { frame_allocator::free_contig(o.index, o.index_frames); }
    *o = EMPTY;
}

/// Garante espaco no vetor para `pages` paginas.
fn grow_index(o: &mut Obj, pages: usize) -> bool {
    if pages <= o.cap { return true; }
    let frames = (pages * 8).div_ceil(4096).max(1);
    let Some(ix) = frame_allocator::alloc_contig(frames) else { return false };
    unsafe {
        core::ptr::write_bytes(ix as *mut u8, 0, frames * 4096);
        if o.index != 0 {
            core::ptr::copy_nonoverlapping(o.index as *const u64, ix as *mut u64, o.cap);
            frame_allocator::free_contig(o.index, o.index_frames);
        }
    }
    o.index = ix;
    o.index_frames = frames;
    o.cap = frames * 4096 / 8;
    true
}

/// Objeto novo (memfd ou shm_open com O_CREAT), com uma referencia de
/// descritor. Retorna o id.
pub fn create(name: Option<&[u8]>) -> Option<u32> {
    let _g = SHM.guard();
    let i = objs().iter().position(|o| !o.used)?;
    let mut o = Obj { used: true, fd_refs: 1, ..EMPTY };
    if let Some(n) = name {
        let n = &n[..n.len().min(NAME_MAX)];
        o.name[..n.len()].copy_from_slice(n);
        o.name_len = n.len();
        o.named = true;
    }
    objs()[i] = o;
    Some(i as u32 + 1)
}

/// Objeto anonimo para mmap(MAP_SHARED | MAP_ANONYMOUS): ja com o tamanho
/// e sem referencias — a VMA criada por vm::mmap_obj soma a primeira. Se o
/// mmap falhar, chame `release_unused`.
pub fn create_anon(size: u64) -> Option<u32> {
    let id = create(None)?;
    let _g = SHM.guard();
    let o = obj(id)?;
    o.fd_refs = 0;
    if size > MAX_SIZE || !grow_index(o, size.div_ceil(4096) as usize) {
        maybe_free(id);
        return None;
    }
    o.size = size;
    Some(id)
}

/// Libera o objeto se ninguem chegou a referencia-lo.
pub fn release_unused(id: u32) {
    let _g = SHM.guard();
    maybe_free(id);
}

/// shm_open sem O_CREAT (ou com, se ja existe): procura pelo nome e soma
/// uma referencia de descritor.
pub fn open_named(name: &[u8]) -> Option<u32> {
    let _g = SHM.guard();
    let i = objs().iter().position(|o| o.used && o.named && &o.name[..o.name_len] == name)?;
    objs()[i].fd_refs += 1;
    Some(i as u32 + 1)
}

/// shm_unlink: tira o nome (o objeto vive enquanto houver referencias).
pub fn unlink(name: &[u8]) -> bool {
    let _g = SHM.guard();
    let Some(i) = objs().iter().position(|o| o.used && o.named && &o.name[..o.name_len] == name) else { return false };
    objs()[i].named = false;
    maybe_free(i as u32 + 1);
    true
}

/// Referencias de descritores (dup, fork, close).
pub fn fd_ref(id: u32, delta: i32) {
    let _g = SHM.guard();
    if let Some(o) = obj(id) { o.fd_refs = (o.fd_refs as i32 + delta).max(0) as u32; }
    if delta < 0 { maybe_free(id); }
}

/// Referencias de VMAs (chamado pelo vm.rs com a trava VM presa).
pub fn map_ref(id: u32, delta: i32) {
    let _g = SHM.guard();
    if let Some(o) = obj(id) { o.map_refs = (o.map_refs as i32 + delta).max(0) as u32; }
    if delta < 0 { maybe_free(id); }
}

pub fn size(id: u32) -> u64 {
    let _g = SHM.guard();
    obj(id).map_or(0, |o| o.size)
}

/// ftruncate: muda o tamanho (paginas alem do fim sao devolvidas).
pub fn truncate(id: u32, size: u64) -> Result<(), i64> {
    if size > MAX_SIZE { return Err(EFBIG); }
    let _g = SHM.guard();
    let o = obj(id).ok_or(ENOMEM)?;
    let pages = size.div_ceil(4096) as usize;
    if !grow_index(o, pages) { return Err(ENOMEM); }
    for pg in pages..o.cap {
        let slot = page_slot(o, pg);
        let pa = unsafe { *slot };
        if pa != 0 { frame_allocator::free_frame(pa); unsafe { *slot = 0 }; }
    }
    // zera o fim da ultima pagina mantida
    if size % 4096 != 0 && pages > 0 {
        let pa = unsafe { *page_slot(o, pages - 1) };
        if pa != 0 {
            let off = (size % 4096) as usize;
            unsafe { core::ptr::write_bytes((pa as *mut u8).add(off), 0, 4096 - off) };
        }
    }
    o.size = size;
    Ok(())
}

/// Pagina `pg` do objeto (criada zerada se `alloc`). None alem do fim.
pub fn page(id: u32, pg: usize, alloc: bool) -> Option<u64> {
    let _g = SHM.guard();
    let o = obj(id)?;
    if pg as u64 >= o.size.div_ceil(4096) { return None; }
    let slot = page_slot(o, pg);
    let pa = unsafe { *slot };
    if pa != 0 || !alloc { return if pa != 0 { Some(pa) } else { None }; }
    let f = frame_allocator::alloc_frame()?;
    unsafe {
        core::ptr::write_bytes(f as *mut u8, 0, 4096);
        *slot = f;
    }
    Some(f)
}

/// read/write por descritor (memoria do usuario ja validada).
pub fn rw(id: u32, off: u64, buf: u64, len: u64, write: bool) -> Result<u64, i64> {
    let size = size(id);
    let end = if write {
        let e = off.checked_add(len).ok_or(EFBIG)?;
        if e > size { truncate(id, e)?; }
        e
    } else {
        if off >= size { return Ok(0); }
        (off + len).min(size)
    };
    let mut at = off;
    while at < end {
        let pg = (at / 4096) as usize;
        let inpg = at % 4096;
        let n = (4096 - inpg).min(end - at);
        let pa = page(id, pg, write);
        unsafe {
            let user = (buf + (at - off)) as *mut u8;
            match (pa, write) {
                (Some(pa), true) => core::ptr::copy_nonoverlapping(user as *const u8, (pa + inpg) as *mut u8, n as usize),
                (Some(pa), false) => core::ptr::copy_nonoverlapping((pa + inpg) as *const u8, user, n as usize),
                (None, false) => core::ptr::write_bytes(user, 0, n as usize),   // buraco
                (None, true) => return Err(ENOMEM),
            }
        }
        at += n;
    }
    Ok(end - off)
}
