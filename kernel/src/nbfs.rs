//! Nebular FileSystem (NBFS v1) — sucessor do PulsarFS com metadados ricos.
//!
//! Melhorias sobre o PFS1: timestamps (mtime), flags de atributo, tipos de
//! arquivo mais ricos (exec/texto/dados/diretorio), tabela de inodes maior
//! (128 arquivos vs 64), e um superbloco com rotulo de volume.
//!
//! Layout no disco (setor = 512 bytes):
//!   setor 0        : superbloco { magic "NBFS", version, file_count, table_sec, total_sec, label[16] }
//!   setores 1..=16 : tabela de inodes (128 entradas de 64 bytes)
//!   setor 17+      : dados, cada arquivo com extensao FIXA pre-alocada
//!
//! Inode (64 bytes):
//!   name[28] start_sec:u32 size:u32 kind:u32 cap_sec:u32 mtime:u32 flags:u32 reserved:u32
//!
//! kind: 1=exec(.pulse) 2=texto 3=dados 4=diretorio
//! flags: bit0=oculto  bit1=somente-leitura
//!
//! A API publica espelha a do pfs.rs para ser um drop-in: mount/count/entry/
//! find/read_file/write_file/create/name_str. Escrita in-place (capacidade fixa).

use crate::blk;
use crate::uart::Uart;
use core::fmt::Write;

pub const NBFS_MAGIC: u32 = 0x5346_424E; // "NBFS" little-endian
pub const MAX_FILES: usize = 128;
pub const NAME_LEN: usize = 28;
const TABLE_SECTORS: u32 = 16;
const DATA_START: u32 = 1 + TABLE_SECTORS; // setor 17

#[repr(C)]
#[derive(Clone, Copy)]
pub struct Inode {
    pub name: [u8; NAME_LEN],   // 28
    pub start_sector: u32,      // 32
    pub size_bytes: u32,        // 36
    pub kind: u32,              // 40
    pub capacity_sec: u32,      // 44
    pub mtime: u32,             // 48
    pub flags: u32,             // 52
    pub _reserved: [u32; 3],    // 64 (padding p/ fechar 64 bytes)
}

// Mesmo nome de tipo publico que o pfs usa, para compatibilidade de chamadas.
pub type FileEntry = Inode;

#[repr(C)]
struct SuperBlock {
    magic: u32,
    version: u32,
    file_count: u32,
    table_sec: u32,
    total_sec: u32,
    label: [u8; 16],
}

#[repr(C, align(512))]
struct SectorBuf([u8; 512]);

#[repr(C, align(512))]
struct TableBuf([Inode; MAX_FILES]);

static mut SECTOR: SectorBuf = SectorBuf([0; 512]);
static mut TABLE: TableBuf = TableBuf(
    [Inode { name: [0; NAME_LEN], start_sector: 0, size_bytes: 0, kind: 0,
             capacity_sec: 0, mtime: 0, flags: 0, _reserved: [0;3] }; MAX_FILES],
);
static mut FILE_COUNT: usize = 0;
static mut NEXT_FREE_SECTOR: u32 = 0;
static mut CLOCK: u32 = 2000; // relogio logico p/ mtime de novos arquivos

/// Monta o Nebular FS: le superbloco + tabela de inodes.
pub fn mount() -> Option<usize> {
    let mut serial = Uart;

    let sector_pa = unsafe { &raw mut SECTOR.0 as *mut u8 as u64 };
    if !blk::read_sectors(0, 1, sector_pa) {
        let _ = write!(serial, "[nbfs] falha lendo superbloco\n");
        return None;
    }
    let sb = unsafe { &*(sector_pa as *const SuperBlock) };
    if sb.magic != NBFS_MAGIC {
        let _ = write!(serial, "[nbfs] magic invalido: {:#x} (esperado NBFS)\n", sb.magic);
        return None;
    }
    let count = (sb.file_count as usize).min(MAX_FILES);
    let label_len = sb.label.iter().position(|&b| b == 0).unwrap_or(16);
    let label = core::str::from_utf8(&sb.label[..label_len]).unwrap_or("?");

    let table_pa = unsafe { &raw mut TABLE.0 as *mut Inode as u64 };
    if !blk::read_sectors(1, TABLE_SECTORS, table_pa) {
        let _ = write!(serial, "[nbfs] falha lendo tabela de inodes\n");
        return None;
    }

    let mut high = DATA_START;
    for i in 0..count {
        let e = entry(i);
        let end = e.start_sector + e.capacity_sec;
        if end > high { high = end; }
    }

    unsafe {
        FILE_COUNT = count;
        NEXT_FREE_SECTOR = high;
    }
    let _ = write!(serial, "[nbfs] volume '{}' montado: {} arquivo(s), proximo setor {}\n",
                   label, count, high);
    for i in 0..count {
        let e = entry(i);
        let k = match e.kind { 1 => "exec", 2 => "texto", 3 => "dados", 4 => "dir", _ => "?" };
        let _ = write!(serial, "[nbfs]   {} — {} B @ setor {} (cap {}, {}, mtime {})\n",
                       name_str(&e.name), e.size_bytes, e.start_sector, e.capacity_sec, k, e.mtime);
    }
    Some(count)
}

pub fn count() -> usize { unsafe { FILE_COUNT } }

pub fn entry(idx: usize) -> Inode {
    unsafe { core::ptr::read_volatile((&raw const TABLE.0).cast::<Inode>().add(idx)) }
}

fn set_entry(idx: usize, e: Inode) {
    unsafe { core::ptr::write_volatile((&raw mut TABLE.0).cast::<Inode>().add(idx), e) }
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
    if idx >= count() { return None; }
    let e = entry(idx);
    let sectors = e.size_bytes.div_ceil(512).max(1);
    if !blk::read_sectors(e.start_sector as u64, sectors, dst_pa) { return None; }
    Some(e.size_bytes)
}

fn flush_meta() -> bool {
    unsafe {
        let mut label = [0u8; 16];
        let l = b"Nebular";
        label[..l.len()].copy_from_slice(l);
        let sb = SuperBlock {
            magic: NBFS_MAGIC,
            version: 1,
            file_count: FILE_COUNT as u32,
            table_sec: TABLE_SECTORS,
            total_sec: 16384,
            label,
        };
        core::ptr::write_bytes(&raw mut SECTOR.0 as *mut u8, 0, 512);
        core::ptr::copy_nonoverlapping(
            &sb as *const SuperBlock as *const u8,
            &raw mut SECTOR.0 as *mut u8,
            size_of::<SuperBlock>(),
        );
        if !blk::write_sectors(0, 1, &raw const SECTOR.0 as *const u8 as u64) {
            return false;
        }
        blk::write_sectors(1, TABLE_SECTORS, &raw const TABLE.0 as *const Inode as u64)
    }
}

pub fn write_file(idx: usize, src_pa: u64, size: u32) -> bool {
    if idx >= count() { return false; }
    let mut e = entry(idx);
    if e.flags & 0x2 != 0 { return false; } // somente-leitura
    let need = size.div_ceil(512).max(1);
    if need > e.capacity_sec { return false; }
    if !blk::write_sectors(e.start_sector as u64, need, src_pa) { return false; }
    e.size_bytes = size;
    unsafe { CLOCK += 1; e.mtime = CLOCK; }
    set_entry(idx, e);
    flush_meta()
}

pub fn create(name: &str, kind: u32, capacity_sec: u32) -> Option<usize> {
    if find(name).is_some() || name.len() >= NAME_LEN { return None; }
    let idx = count();
    if idx >= MAX_FILES { return None; }
    let start = unsafe { NEXT_FREE_SECTOR };
    if start + capacity_sec > 16384 { return None; }

    let mut e = Inode {
        name: [0; NAME_LEN],
        start_sector: start,
        size_bytes: 0,
        kind,
        capacity_sec,
        mtime: unsafe { CLOCK += 1; CLOCK },
        flags: 0,
        _reserved: [0; 3],
    };
    e.name[..name.len()].copy_from_slice(name.as_bytes());
    set_entry(idx, e);
    unsafe {
        FILE_COUNT += 1;
        NEXT_FREE_SECTOR += capacity_sec;
    }
    if flush_meta() { Some(idx) } else { None }
}
