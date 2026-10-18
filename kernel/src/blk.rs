//! Driver virtio-blk sobre virtio-mmio.
//!
//! Requisicao = cadeia de 3 descritores:
//!   [header 16B: tipo + setor]  (device le)
//!   [dados N*512B]              (device le OU escreve, conforme o tipo)
//!   [status 1B]                 (device escreve; 0 = OK)

use crate::virtio::{self, QueueMem, VirtQueue};
use crate::uart::Uart;
use core::fmt::Write;

const VIRTIO_ID_BLK: u32 = 2;
pub const SECTOR_SIZE: usize = 512;

const T_IN: u32 = 0; // leitura (device -> RAM)
const T_OUT: u32 = 1; // escrita (RAM -> device)

#[repr(C)]
struct BlkReq {
    req_type: u32,
    _reserved: u32,
    sector: u64,
}

#[repr(C, align(64))]
struct ReqBuf(BlkReq);

static mut BLKQ_MEM: QueueMem = QueueMem::zeroed();
static mut REQ: ReqBuf = ReqBuf(BlkReq { req_type: 0, _reserved: 0, sector: 0 });
static mut STATUS: u8 = 0xFF;

struct Blk {
    queue: VirtQueue,
    capacity: u64, // em setores
}

static mut BLK: Option<Blk> = None;

/// Inicializa o dispositivo de bloco. Retorna a capacidade em setores.
pub fn init() -> Option<u64> {
    let mut serial = Uart;

    let base = virtio::probe(VIRTIO_ID_BLK)?;
    if !virtio::init_device(base) {
        let _ = write!(serial, "[blk] handshake falhou\n");
        return None;
    }
    let queue = unsafe { VirtQueue::setup(base, 0, &raw mut BLKQ_MEM)? };
    virtio::driver_ok(base);

    // Config: capacidade em setores nos primeiros 8 bytes (LE)
    let mut cap = 0u64;
    for i in 0..8 {
        cap |= (virtio::config_read8(base, i) as u64) << (8 * i);
    }
    let _ = write!(serial, "[blk] virtio-blk em {:#x}, {} setores ({} KB)\n",
                   base, cap, cap * 512 / 1024);

    unsafe { BLK = Some(Blk { queue, capacity: cap }) };
    Some(cap)
}

fn transfer(req_type: u32, lba: u64, count: u32, buf_pa: u64) -> bool {
    let blk = unsafe { (&raw mut BLK).as_mut().unwrap().as_mut() };
    let Some(blk) = blk else { return false };
    if lba + count as u64 > blk.capacity {
        return false;
    }

    unsafe {
        REQ.0 = BlkReq { req_type, _reserved: 0, sector: lba };
        STATUS = 0xFF;

        let hdr = (&raw const REQ.0 as u64, size_of::<BlkReq>() as u32, false);
        let data = (buf_pa, count * SECTOR_SIZE as u32, req_type == T_IN);
        let status = (&raw mut STATUS as u64, 1, true);
        blk.queue.request_sync(&[hdr, data, status]);

        core::ptr::read_volatile(&raw const STATUS) == 0
    }
}

/// Le `count` setores a partir do LBA `lba` para o buffer fisico `buf_pa`.
pub fn read_sectors(lba: u64, count: u32, buf_pa: u64) -> bool {
    transfer(T_IN, lba, count, buf_pa)
}

/// Escreve `count` setores a partir do buffer fisico `buf_pa` no LBA `lba`.
#[allow(dead_code)]
pub fn write_sectors(lba: u64, count: u32, buf_pa: u64) -> bool {
    transfer(T_OUT, lba, count, buf_pa)
}

/// Maximo de segmentos de dados numa requisicao (a fila tem 64 descritores:
/// cabecalho + dados + status).
pub const MAX_SEGS: usize = 62;

/// Le ou escreve setores consecutivos a partir de `lba` usando varios
/// buffers fisicos (scatter-gather), numa unica requisicao ao dispositivo.
/// Cada segmento e (endereco fisico, bytes), multiplo de 512.
pub fn transfer_sg(write: bool, lba: u64, segs: &[(u64, u32)]) -> bool {
    let blk = unsafe { (&raw mut BLK).as_mut().unwrap().as_mut() };
    let Some(blk) = blk else { return false };
    if segs.is_empty() || segs.len() > MAX_SEGS { return false; }
    let total: u64 = segs.iter().map(|s| s.1 as u64).sum();
    if lba + total / SECTOR_SIZE as u64 > blk.capacity { return false; }
    let req_type = if write { T_OUT } else { T_IN };
    unsafe {
        REQ.0 = BlkReq { req_type, _reserved: 0, sector: lba };
        STATUS = 0xFF;
        let mut bufs = [(0u64, 0u32, false); MAX_SEGS + 2];
        bufs[0] = (&raw const REQ.0 as u64, size_of::<BlkReq>() as u32, false);
        for (i, &(pa, len)) in segs.iter().enumerate() {
            bufs[1 + i] = (pa, len, !write);
        }
        bufs[1 + segs.len()] = (&raw mut STATUS as u64, 1, true);
        blk.queue.request_sync(&bufs[..segs.len() + 2]);
        core::ptr::read_volatile(&raw const STATUS) == 0
    }
}
