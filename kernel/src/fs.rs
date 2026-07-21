//! fs — camada de detecção que escolhe o filesystem pelo magic do superbloco.
//!
//! Suporta dois formatos com a mesma API in-place de capacidade fixa:
//!   - Nebular FileSystem (NBFS): magic "NBFS", metadados ricos (mtime, flags)
//!   - PulsarFS (PFS1): formato legado
//!
//! O resto do kernel chama `fs::mount/count/entry/find/read_file/write_file/
//! create/name_str` sem se importar com qual driver responde. A struct
//! FileEntry retornada e sempre a do NBFS (superset), preenchida a partir do
//! PFS quando necessario.

use crate::blk;
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
enum Backend { None, Nbfs, Pfs }
static mut BACKEND: Backend = Backend::None;

const NBFS_MAGIC: u32 = 0x5346_424E;
const PFS_MAGIC: u32 = 0x3153_4650;

#[repr(C, align(512))]
struct Probe([u8; 512]);
static mut PROBE: Probe = Probe([0; 512]);

/// Detecta o formato lendo o magic do setor 0, depois monta o driver certo.
pub fn mount() -> Option<usize> {
    let mut serial = Uart;
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
    unsafe {
        match BACKEND {
            Backend::Nbfs => crate::nbfs::count(),
            Backend::Pfs => crate::pfs::count(),
            Backend::None => 0,
        }
    }
}

pub fn entry(idx: usize) -> FileEntry {
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
    for i in 0..count() {
        let e = entry(i);
        if name_str(&e.name) == name { return Some(i); }
    }
    None
}

pub fn read_file(idx: usize, dst_pa: u64) -> Option<u32> {
    unsafe {
        match BACKEND {
            Backend::Nbfs => crate::nbfs::read_file(idx, dst_pa),
            Backend::Pfs => crate::pfs::read_file(idx, dst_pa),
            Backend::None => None,
        }
    }
}

pub fn write_file(idx: usize, src_pa: u64, size: u32) -> bool {
    unsafe {
        match BACKEND {
            Backend::Nbfs => crate::nbfs::write_file(idx, src_pa, size),
            Backend::Pfs => crate::pfs::write_file(idx, src_pa, size),
            Backend::None => false,
        }
    }
}

pub fn create(name: &str, kind: u32, capacity_sec: u32) -> Option<usize> {
    unsafe {
        match BACKEND {
            Backend::Nbfs => crate::nbfs::create(name, kind, capacity_sec),
            Backend::Pfs => crate::pfs::create(name, kind, capacity_sec),
            Backend::None => None,
        }
    }
}
