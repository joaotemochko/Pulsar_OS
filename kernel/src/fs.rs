//! fs — camada de detecção que escolhe o filesystem pelo magic do superbloco.
//!
//! Formatos suportados:
//!   - ext2: diretorios, links simbolicos, arquivos grandes (padrao; ver
//!     ext2.rs e file.rs para a API POSIX com descritores)
//!   - Nebular FileSystem (NBFS): plano, magic "NBFS", metadados ricos
//!   - PulsarFS (PFS1): formato legado
//!
//! A API indexada abaixo (count/entry/read_file/...) e a dos apps antigos:
//! no ext2 ela enxerga os arquivos regulares do diretorio raiz.
//!
//! O resto do kernel chama `fs::mount/count/entry/find/read_file/write_file/
//! create/name_str` sem se importar com qual driver responde. A struct
//! FileEntry retornada e sempre a do NBFS (superset), preenchida a partir do
//! PFS quando necessario.

use crate::blk;
use crate::sync::SpinLock;

/// Trava do sistema de arquivos e do disco (virtio-blk) por baixo dele.
pub static FS: SpinLock = SpinLock::new();
use crate::uart::Uart;
use core::fmt::Write;

pub const NAME_LEN: usize = 28;

/// Entrada unificada (superset). O PFS preenche mtime/flags com 0.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct FileEntry {
    pub name: [u8; NAME_LEN],
    pub start_sector: u32,
    pub size_bytes: u32,
    pub kind: u32,
    pub capacity_sec: u32,
    pub mtime: u32,
    pub flags: u32,
}

#[derive(Clone, Copy, PartialEq)]
enum Backend { None, Nbfs, Pfs, Ext2 }
static mut BACKEND: Backend = Backend::None;

const NBFS_MAGIC: u32 = 0x5346_424E;
const PFS_MAGIC: u32 = 0x3153_4650;

#[repr(C, align(512))]
struct Probe([u8; 512]);
static mut PROBE: Probe = Probe([0; 512]);

/// Detecta o formato lendo o magic do setor 0, depois monta o driver certo.
pub fn mount() -> Option<usize> {
    let mut serial = Uart;
    if crate::ext2::mount() {
        unsafe { BACKEND = Backend::Ext2; }
        let _g = FS.guard();
        return Some(count_nl());
    }
    let pa = unsafe { &raw mut PROBE.0 as *mut u8 as u64 };
    if !blk::read_sectors(0, 1, pa) {
        let _ = write!(serial, "[fs] falha lendo setor 0\n");
        return None;
    }
    let magic = unsafe { core::ptr::read_volatile(pa as *const u32) };
    if magic == NBFS_MAGIC {
        let _ = write!(serial, "[fs] Nebular FileSystem detectado\n");
        unsafe { BACKEND = Backend::Nbfs; }
        crate::nbfs::mount()
    } else if magic == PFS_MAGIC {
        let _ = write!(serial, "[fs] PulsarFS (legado) detectado\n");
        unsafe { BACKEND = Backend::Pfs; }
        crate::pfs::mount()
    } else {
        let _ = write!(serial, "[fs] magic desconhecido: {:#x}\n", magic);
        None
    }
}

pub fn count() -> usize {
    let _g = FS.guard();
    count_nl()
}

fn count_nl() -> usize {
    unsafe {
        match BACKEND {
            Backend::Nbfs => crate::nbfs::count(),
            Backend::Pfs => crate::pfs::count(),
            Backend::Ext2 => { legacy_refresh(); (*(&raw const LEGACY)).len }
            Backend::None => 0,
        }
    }
}

pub fn entry(idx: usize) -> FileEntry {
    let _g = FS.guard();
    entry_nl(idx)
}

fn entry_nl(idx: usize) -> FileEntry {
    unsafe {
        match BACKEND {
            Backend::Nbfs => {
                let e = crate::nbfs::entry(idx);
                FileEntry { name: e.name, start_sector: e.start_sector, size_bytes: e.size_bytes,
                            kind: e.kind, capacity_sec: e.capacity_sec, mtime: e.mtime, flags: e.flags }
            }
            Backend::Pfs => {
                let e = crate::pfs::entry(idx);
                // PFS tem name[32]; copiamos os primeiros 28 (nomes sao curtos).
                let mut name = [0u8; NAME_LEN];
                let n = NAME_LEN.min(32);
                name[..n].copy_from_slice(&e.name[..n]);
                FileEntry { name, start_sector: e.start_sector, size_bytes: e.size_bytes,
                            kind: e.kind, capacity_sec: e.capacity_sec, mtime: 0, flags: 0 }
            }
            Backend::Ext2 => {
                let l = &*(&raw const LEGACY);
                let mut e = FileEntry { name: [0; NAME_LEN], start_sector: 0, size_bytes: 0,
                                        kind: 0, capacity_sec: 0, mtime: 0, flags: 0 };
                if idx < l.len {
                    let (ino, name) = l.items[idx];
                    e.name = name;
                    e.start_sector = ino;
                    e.kind = legacy_kind(&name);
                    e.capacity_sec = u32::MAX / 512;
                    if let Ok(n) = crate::ext2::read_inode(ino) {
                        e.size_bytes = n.size().min(u32::MAX as u64) as u32;
                        e.mtime = n.mtime;
                    }
                }
                e
            }
            Backend::None => FileEntry { name: [0; NAME_LEN], start_sector: 0, size_bytes: 0,
                                         kind: 0, capacity_sec: 0, mtime: 0, flags: 0 },
        }
    }
}

pub fn name_str(name: &[u8; NAME_LEN]) -> &str {
    let len = name.iter().position(|&b| b == 0).unwrap_or(NAME_LEN);
    core::str::from_utf8(&name[..len]).unwrap_or("?")
}

pub fn find(name: &str) -> Option<usize> {
    let _g = FS.guard();
    for i in 0..count_nl() {
        let e = entry_nl(i);
        if name_str(&e.name) == name { return Some(i); }
    }
    None
}

pub fn read_file(idx: usize, dst_pa: u64) -> Option<u32> {
    let _g = FS.guard();
    unsafe {
        match BACKEND {
            Backend::Nbfs => crate::nbfs::read_file(idx, dst_pa),
            Backend::Pfs => crate::pfs::read_file(idx, dst_pa),
            Backend::Ext2 => {
                let ino = legacy_ino(idx)?;
                crate::ext2::read_all_phys(ino, dst_pa).ok().map(|s| s as u32)
            }
            Backend::None => None,
        }
    }
}

pub fn write_file(idx: usize, src_pa: u64, size: u32) -> bool {
    let _g = FS.guard();
    unsafe {
        match BACKEND {
            Backend::Nbfs => crate::nbfs::write_file(idx, src_pa, size),
            Backend::Pfs => crate::pfs::write_file(idx, src_pa, size),
            Backend::Ext2 => {
                let Some(ino) = legacy_ino(idx) else { return false };
                crate::ext2::write(ino, 0, src_pa as *const u8, size as usize) == Ok(size as usize)
                    && crate::ext2::truncate(ino, size as u64).is_ok()
            }
            Backend::None => false,
        }
    }
}

pub fn create(name: &str, kind: u32, capacity_sec: u32) -> Option<usize> {
    let _g = FS.guard();
    unsafe {
        match BACKEND {
            Backend::Nbfs => crate::nbfs::create(name, kind, capacity_sec),
            Backend::Pfs => crate::pfs::create(name, kind, capacity_sec),
            Backend::Ext2 => {
                if name.len() >= NAME_LEN || name.contains('/') { return None; }
                let mut path = [0u8; NAME_LEN + 1];
                path[0] = b'/';
                path[1..1 + name.len()].copy_from_slice(name.as_bytes());
                let (_, created) = crate::ext2::create(crate::ext2::ROOT_INO, &path[..1 + name.len()], 0o644, true).ok()?;
                if !created { return None; }
                legacy_dirty();
                legacy_refresh();
                let l = &*(&raw const LEGACY);
                (0..l.len).find(|&i| name_str(&l.items[i].1) == name)
            }
            Backend::None => None,
        }
    }
}

// ---------------------------------------------------------------- ext2: API antiga

/// Arquivos regulares da raiz com nome curto (cabe em NAME_LEN), na ordem
/// do diretorio. Refeita quando algo muda no espaco de nomes.
const LEGACY_MAX: usize = 256;
struct Legacy {
    items: [(u32, [u8; NAME_LEN]); LEGACY_MAX],
    len: usize,
    dirty: bool,
}
static mut LEGACY: Legacy = Legacy { items: [(0, [0; NAME_LEN]); LEGACY_MAX], len: 0, dirty: true };

pub fn is_ext2() -> bool {
    unsafe { BACKEND == Backend::Ext2 }
}

/// Algo foi criado, removido ou renomeado: refazer a lista antiga.
pub fn legacy_dirty() {
    unsafe { (*(&raw mut LEGACY)).dirty = true; }
}

fn legacy_refresh() {
    let l = unsafe { &mut *(&raw mut LEGACY) };
    if !l.dirty { return; }
    l.len = 0;
    let mut pos = 0;
    while let Ok(Some(d)) = crate::ext2::readdir(crate::ext2::ROOT_INO, pos) {
        pos = d.next;
        if d.ftype != crate::ext2::FT_REG || d.name_len >= NAME_LEN || l.len >= LEGACY_MAX { continue; }
        let mut name = [0u8; NAME_LEN];
        name[..d.name_len].copy_from_slice(d.name());
        l.items[l.len] = (d.ino, name);
        l.len += 1;
    }
    l.dirty = false;
}

fn legacy_ino(idx: usize) -> Option<u32> {
    legacy_refresh();
    let l = unsafe { &*(&raw const LEGACY) };
    if idx < l.len { Some(l.items[idx].0) } else { None }
}

/// kind da API antiga a partir da extensao: 1 exec, 2 texto, 3 dados.
fn legacy_kind(name: &[u8; NAME_LEN]) -> u32 {
    let n = name_str(name);
    let ext = n.rsplit_once('.').map(|(_, e)| e).unwrap_or("");
    match ext {
        "pulse" => 1,
        "txt" | "md" | "cfg" | "conf" | "ini" | "c" | "h" | "rs" | "py" | "sh" | "html" | "htm"
        | "css" | "js" | "json" | "xml" | "csv" | "log" | "toml" => 2,
        _ => 3,
    }
}
