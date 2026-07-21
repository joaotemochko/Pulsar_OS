//! PulsarFS v2 — sistema de arquivos com leitura E escrita.
//!
//! Layout no disco (setor = 512 bytes):
//!   setor 0        : superbloco { magic "PFS1", version, file_count, table_sectors }
//!   setores 1..=8  : tabela de arquivos (64 entradas de 64 bytes)
//!   setor 9+       : dados, cada arquivo com extensao FIXA pre-alocada
//!
//! Entrada da tabela (64 bytes):
//!   name[32]      : nome ASCII, zero-terminado
//!   start_sector  : u32  (inicio da extensao)
//!   size_bytes    : u32  (tamanho logico atual)
//!   kind          : u32  (1 = .pulse executavel, 2 = texto/dado)
//!   capacity_sec  : u32  (setores reservados p/ este arquivo)
//!   reserved[16]
//!
//! Escrita in-place: como cada arquivo tem capacidade fixa, escrever nao
//! realoca — so atualiza os setores de dados e o size_bytes na tabela.
//! Simples e suficiente para um editor de texto. (Sem fragmentacao,
//! sem crescer alem da capacidade; o mkpfs reserva folga por arquivo.)

use crate::blk;
use crate::uart::Uart;
use core::fmt::Write;

pub const PFS_MAGIC: u32 = 0x3153_4650; // "PFS1" little-endian
pub const MAX_FILES: usize = 64;
pub const NAME_LEN: usize = 32;
const TABLE_SECTORS: u32 = 8;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct FileEntry {
    pub name: [u8; NAME_LEN],
    pub start_sector: u32,
    pub size_bytes: u32,
    pub kind: u32,
    pub capacity_sec: u32,
    pub _reserved: [u8; 16],
}

#[repr(C)]
struct SuperBlock {
    magic: u32,
    version: u32,
    file_count: u32,
    table_sectors: u32,
}

#[repr(C, align(512))]
struct SectorBuf([u8; 512]);

#[repr(C, align(512))]
struct TableBuf([FileEntry; MAX_FILES]);

static mut SECTOR: SectorBuf = SectorBuf([0; 512]);
static mut TABLE: TableBuf = TableBuf(
    [FileEntry { name: [0; NAME_LEN], start_sector: 0, size_bytes: 0, kind: 0,
                 capacity_sec: 0, _reserved: [0; 16] }; MAX_FILES],
);
static mut FILE_COUNT: usize = 0;
static mut NEXT_FREE_SECTOR: u32 = 0;

/// Monta o PulsarFS: le superbloco + tabela. Retorna o numero de arquivos.
pub fn mount() -> Option<usize> {
    let mut serial = Uart;

    let sector_pa = unsafe { &raw mut SECTOR.0 as *mut u8 as u64 };
    if !blk::read_sectors(0, 1, sector_pa) {
        let _ = write!(serial, "[pfs] falha lendo superbloco\n");
        return None;
    }
    let sb = unsafe { &*(sector_pa as *const SuperBlock) };
    if sb.magic != PFS_MAGIC {
        let _ = write!(serial, "[pfs] magic invalido: {:#x}\n", sb.magic);
        return None;
    }
    let count = (sb.file_count as usize).min(MAX_FILES);

    let table_pa = unsafe { &raw mut TABLE.0 as *mut FileEntry as u64 };
    if !blk::read_sectors(1, TABLE_SECTORS, table_pa) {
        let _ = write!(serial, "[pfs] falha lendo tabela\n");
        return None;
    }

    // Calcula o primeiro setor livre (fim da ultima extensao)
    let mut high = 1 + TABLE_SECTORS;
    for i in 0..count {
        let e = entry(i);
        let end = e.start_sector + e.capacity_sec;
        if end > high {
            high = end;
        }
    }

    unsafe {
        FILE_COUNT = count;
        NEXT_FREE_SECTOR = high;
    }
    let _ = write!(serial, "[pfs] montado: {} arquivo(s), proximo setor livre {}\n", count, high);
    for i in 0..count {
        let e = entry(i);
        let _ = write!(serial, "[pfs]   {} — {} bytes @ setor {} (cap {} sec)\n",
                       name_str(&e.name), e.size_bytes, e.start_sector, e.capacity_sec);
    }
    Some(count)
}

pub fn count() -> usize {
    unsafe { FILE_COUNT }
}

pub fn entry(idx: usize) -> FileEntry {
    unsafe { core::ptr::read_volatile((&raw const TABLE.0).cast::<FileEntry>().add(idx)) }
}

fn set_entry(idx: usize, e: FileEntry) {
    unsafe { core::ptr::write_volatile((&raw mut TABLE.0).cast::<FileEntry>().add(idx), e) }
}

/// Nome como &str (ate o primeiro NUL).
pub fn name_str(name: &[u8; NAME_LEN]) -> &str {
    let len = name.iter().position(|&b| b == 0).unwrap_or(NAME_LEN);
    core::str::from_utf8(&name[..len]).unwrap_or("?")
}

/// Procura um arquivo pelo nome. Retorna o indice.
pub fn find(name: &str) -> Option<usize> {
    for i in 0..count() {
        let e = entry(i);
        if name_str(&e.name) == name {
            return Some(i);
        }
    }
    None
}

/// Le o arquivo `idx` inteiro para o buffer fisico `dst_pa`.
pub fn read_file(idx: usize, dst_pa: u64) -> Option<u32> {
    if idx >= count() {
        return None;
    }
    let e = entry(idx);
    let sectors = e.size_bytes.div_ceil(512).max(1);
    if !blk::read_sectors(e.start_sector as u64, sectors, dst_pa) {
        return None;
    }
    Some(e.size_bytes)
}

/// Persiste superbloco + tabela no disco (apos alterar metadados).
fn flush_meta() -> bool {
    unsafe {
        let sb = SuperBlock {
            magic: PFS_MAGIC,
            version: 2,
            file_count: FILE_COUNT as u32,
            table_sectors: TABLE_SECTORS,
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
        blk::write_sectors(1, TABLE_SECTORS, &raw const TABLE.0 as *const FileEntry as u64)
    }
}

/// Escreve `size` bytes do buffer fisico `src_pa` no arquivo `idx`,
/// truncando/estendendo o tamanho logico (dentro da capacidade).
/// Retorna false se estourar a capacidade reservada.
pub fn write_file(idx: usize, src_pa: u64, size: u32) -> bool {
    if idx >= count() {
        return false;
    }
    let mut e = entry(idx);
    let need = size.div_ceil(512).max(1);
    if need > e.capacity_sec {
        return false; // sem realocacao nesta versao
    }
    if !blk::write_sectors(e.start_sector as u64, need, src_pa) {
        return false;
    }
    e.size_bytes = size;
    set_entry(idx, e);
    flush_meta()
}

/// Cria um arquivo novo com `capacity_sec` setores reservados.
/// Retorna o indice, ou None (sem slot / sem espaco / nome duplicado).
pub fn create(name: &str, kind: u32, capacity_sec: u32) -> Option<usize> {
    if find(name).is_some() || name.len() >= NAME_LEN {
        return None;
    }
    let idx = count();
    if idx >= MAX_FILES {
        return None;
    }
    let start = unsafe { NEXT_FREE_SECTOR };
    let total = 16384u32; // capacidade do disco (8MB / 512)
    if start + capacity_sec > total {
        return None;
    }

    let mut e = FileEntry {
        name: [0; NAME_LEN],
        start_sector: start,
        size_bytes: 0,
        kind,
        capacity_sec,
        _reserved: [0; 16],
    };
    e.name[..name.len()].copy_from_slice(name.as_bytes());
    set_entry(idx, e);
    unsafe {
        FILE_COUNT += 1;
        NEXT_FREE_SECTOR += capacity_sec;
    }
    if flush_meta() {
        Some(idx)
    } else {
        None
    }
}
