//! Transporte virtio-mmio (versao 2, "moderno") para a maquina QEMU virt.
//!
//! A `virt` expoe 32 slots virtio-mmio a partir de 0x0a000000, com stride
//! de 0x200. Cada slot pode conter um dispositivo (GPU=16, block=2,
//! input=18, ...) ou estar vazio (DeviceID=0).
//!
//! IMPORTANTE: exige `-global virtio-mmio.force-legacy=false` no QEMU,
//! pois implementamos apenas a interface moderna (Version=2).
//!
//! A fila (virtqueue split) opera por *polling* — sem IRQ por enquanto.
//! Suficiente para a inicializacao sincrona do GPU; quando o compositor
//! em EL0 existir, migramos para notificacao por interrupcao.

use core::ptr::{read_volatile, write_volatile};
use core::sync::atomic::{fence, Ordering};

pub const VIRTIO_MMIO_BASE: u64 = 0x0a00_0000;
pub const VIRTIO_MMIO_STRIDE: u64 = 0x200;
pub const VIRTIO_MMIO_SLOTS: u64 = 32;

// Offsets dos registradores (interface moderna, v2)
const MAGIC_VALUE: u64 = 0x000; // deve ler 0x74726976 ("virt")
const VERSION: u64 = 0x004; // deve ler 2
const DEVICE_ID: u64 = 0x008;
const DEVICE_FEATURES: u64 = 0x010;
const DEVICE_FEATURES_SEL: u64 = 0x014;
const DRIVER_FEATURES: u64 = 0x020;
const DRIVER_FEATURES_SEL: u64 = 0x024;
const QUEUE_SEL: u64 = 0x030;
const QUEUE_NUM_MAX: u64 = 0x034;
const QUEUE_NUM: u64 = 0x038;
const QUEUE_READY: u64 = 0x044;
const QUEUE_NOTIFY: u64 = 0x050;
const INTERRUPT_STATUS: u64 = 0x060;
const INTERRUPT_ACK: u64 = 0x064;
const STATUS: u64 = 0x070;
const QUEUE_DESC_LOW: u64 = 0x080;
const QUEUE_DESC_HIGH: u64 = 0x084;
const QUEUE_DRIVER_LOW: u64 = 0x090; // anel avail
const QUEUE_DRIVER_HIGH: u64 = 0x094;
const QUEUE_DEVICE_LOW: u64 = 0x0a0; // anel used
const QUEUE_DEVICE_HIGH: u64 = 0x0a4;

const MAGIC: u32 = 0x7472_6976;

// Bits de status do dispositivo
const ST_ACKNOWLEDGE: u32 = 1;
const ST_DRIVER: u32 = 2;
const ST_DRIVER_OK: u32 = 4;
const ST_FEATURES_OK: u32 = 8;
const ST_FAILED: u32 = 128;

/// VIRTIO_F_VERSION_1 (bit 32): unico feature que negociamos.
const F_VERSION_1_HI: u32 = 1; // bit 32 => bit 0 da palavra alta

#[inline]
fn reg_read(base: u64, off: u64) -> u32 {
    unsafe { read_volatile((base + off) as *const u32) }
}

#[inline]
fn reg_write(base: u64, off: u64, val: u32) {
    unsafe { write_volatile((base + off) as *mut u32, val) }
}

/// Varre os 32 slots procurando um dispositivo com o DeviceID pedido.
/// Retorna o endereco base do slot, ou None.
pub fn probe(device_id: u32) -> Option<u64> {
    for i in 0..VIRTIO_MMIO_SLOTS {
        let base = VIRTIO_MMIO_BASE + i * VIRTIO_MMIO_STRIDE;
        if reg_read(base, MAGIC_VALUE) != MAGIC {
            continue;
        }
        if reg_read(base, VERSION) != 2 {
            continue; // legado (v1) nao suportado
        }
        if reg_read(base, DEVICE_ID) == device_id {
            return Some(base);
        }
    }
    None
}

/// Varre os slots e retorna ate `N` bases com o DeviceID pedido.
/// (ha dois dispositivos de input: teclado e tablet, ambos ID 18)
pub fn probe_all<const N: usize>(device_id: u32) -> ([u64; N], usize) {
    let mut out = [0u64; N];
    let mut n = 0;
    for i in 0..VIRTIO_MMIO_SLOTS {
        if n == N {
            break;
        }
        let base = VIRTIO_MMIO_BASE + i * VIRTIO_MMIO_STRIDE;
        if reg_read(base, MAGIC_VALUE) == MAGIC
            && reg_read(base, VERSION) == 2
            && reg_read(base, DEVICE_ID) == device_id
        {
            out[n] = base;
            n += 1;
        }
    }
    (out, n)
}

/// Le um byte do espaco de configuracao do dispositivo (offset 0x100+).
pub fn config_read8(base: u64, off: u64) -> u8 {
    unsafe { read_volatile((base + 0x100 + off) as *const u8) }
}

/// Escreve um byte no espaco de configuracao do dispositivo.
pub fn config_write8(base: u64, off: u64, val: u8) {
    unsafe { write_volatile((base + 0x100 + off) as *mut u8, val) }
}

/// Handshake de inicializacao (secao 3.1.1 da spec virtio):
/// reset -> ACKNOWLEDGE -> DRIVER -> features -> FEATURES_OK.
/// Retorna false se o dispositivo rejeitar os features.
pub fn init_device(base: u64) -> bool {
    reg_write(base, STATUS, 0); // reset
    reg_write(base, STATUS, ST_ACKNOWLEDGE);
    reg_write(base, STATUS, ST_ACKNOWLEDGE | ST_DRIVER);

    // Le features (so por diagnostico do bit VERSION_1)
    reg_write(base, DEVICE_FEATURES_SEL, 1);
    let feats_hi = reg_read(base, DEVICE_FEATURES);
    if feats_hi & F_VERSION_1_HI == 0 {
        reg_write(base, STATUS, ST_FAILED);
        return false;
    }

    // Aceita apenas VERSION_1
    reg_write(base, DRIVER_FEATURES_SEL, 0);
    reg_write(base, DRIVER_FEATURES, 0);
    reg_write(base, DRIVER_FEATURES_SEL, 1);
    reg_write(base, DRIVER_FEATURES, F_VERSION_1_HI);

    reg_write(base, STATUS, ST_ACKNOWLEDGE | ST_DRIVER | ST_FEATURES_OK);
    if reg_read(base, STATUS) & ST_FEATURES_OK == 0 {
        reg_write(base, STATUS, ST_FAILED);
        return false;
    }
    true
}

/// Marca o dispositivo como pronto (apos configurar as filas).
pub fn driver_ok(base: u64) {
    let st = reg_read(base, STATUS);
    reg_write(base, STATUS, st | ST_DRIVER_OK);
}

// ---------------------------------------------------------------------------
// Virtqueue split (spec secao 2.6), tamanho fixo QSIZE, operacao sincrona.
// ---------------------------------------------------------------------------

pub const QSIZE: usize = 64;

const DESC_F_NEXT: u16 = 1;
const DESC_F_WRITE: u16 = 2; // dispositivo escreve neste buffer

#[repr(C, align(16))]
#[derive(Clone, Copy)]
struct Desc {
    addr: u64,
    len: u32,
    flags: u16,
    next: u16,
}

#[repr(C, align(2))]
struct Avail {
    flags: u16,
    idx: u16,
    ring: [u16; QSIZE],
    used_event: u16,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct UsedElem {
    id: u32,
    len: u32,
}

#[repr(C, align(4))]
struct Used {
    flags: u16,
    idx: u16,
    ring: [UsedElem; QSIZE],
    avail_event: u16,
}

/// Memoria de uma fila. Alinhada a pagina para simplificar o DMA
/// (enderecos fisicos == virtuais pelo identity map do kernel).
#[repr(C, align(4096))]
pub struct QueueMem {
    desc: [Desc; QSIZE],
    avail: Avail,
    used: Used,
}

impl QueueMem {
    pub const fn zeroed() -> Self {
        QueueMem {
            desc: [Desc { addr: 0, len: 0, flags: 0, next: 0 }; QSIZE],
            avail: Avail { flags: 0, idx: 0, ring: [0; QSIZE], used_event: 0 },
            used: Used {
                flags: 0,
                idx: 0,
                ring: [UsedElem { id: 0, len: 0 }; QSIZE],
                avail_event: 0,
            },
        }
    }
}

/// Estado de uma virtqueue ja registrada no dispositivo.
pub struct VirtQueue {
    base: u64,       // base MMIO do dispositivo
    index: u32,      // indice da fila (0 = controlq no GPU)
    mem: *mut QueueMem,
    last_used: u16,  // ultimo used.idx consumido
}

impl VirtQueue {
    /// Configura a fila `index` do dispositivo em `base` usando a memoria
    /// estatica `mem`. Retorna None se o dispositivo nao suportar QSIZE.
    pub unsafe fn setup(base: u64, index: u32, mem: *mut QueueMem) -> Option<VirtQueue> {
        reg_write(base, QUEUE_SEL, index);
        let max = reg_read(base, QUEUE_NUM_MAX);
        if (max as usize) < QSIZE {
            return None;
        }
        reg_write(base, QUEUE_NUM, QSIZE as u32);

        let desc_pa = unsafe { &raw const (*mem).desc } as u64;
        let avail_pa = unsafe { &raw const (*mem).avail } as u64;
        let used_pa = unsafe { &raw const (*mem).used } as u64;

        reg_write(base, QUEUE_DESC_LOW, desc_pa as u32);
        reg_write(base, QUEUE_DESC_HIGH, (desc_pa >> 32) as u32);
        reg_write(base, QUEUE_DRIVER_LOW, avail_pa as u32);
        reg_write(base, QUEUE_DRIVER_HIGH, (avail_pa >> 32) as u32);
        reg_write(base, QUEUE_DEVICE_LOW, used_pa as u32);
        reg_write(base, QUEUE_DEVICE_HIGH, (used_pa >> 32) as u32);
        reg_write(base, QUEUE_READY, 1);

        Some(VirtQueue { base, index, mem, last_used: 0 })
    }

    /// Envia uma cadeia de descritores e espera (polling) o dispositivo
    /// consumir. Cada item: (endereco fisico, tamanho, device_escreve).
    /// Retorna o total de bytes escritos pelo dispositivo.
    pub fn request_sync(&mut self, bufs: &[(u64, u32, bool)]) -> u32 {
        let q = unsafe { &mut *self.mem };
        let n = bufs.len().min(QSIZE);

        for (i, &(addr, len, dev_writes)) in bufs.iter().take(n).enumerate() {
            let mut flags = 0;
            if i + 1 < n {
                flags |= DESC_F_NEXT;
            }
            if dev_writes {
                flags |= DESC_F_WRITE;
            }
            q.desc[i] = Desc { addr, len, flags, next: (i as u16) + 1 };
        }

        let slot = (q.avail.idx as usize) % QSIZE;
        q.avail.ring[slot] = 0;

        fence(Ordering::SeqCst);
        let new_idx = q.avail.idx.wrapping_add(1);
        unsafe { write_volatile(&raw mut q.avail.idx, new_idx) };
        fence(Ordering::SeqCst);

        reg_write(self.base, QUEUE_NOTIFY, self.index);

        loop {
            fence(Ordering::SeqCst);
            let used_idx = unsafe { read_volatile(&raw const q.used.idx) };
            if used_idx != self.last_used {
                break;
            }
            core::hint::spin_loop();
        }

        let elem = q.used.ring[(self.last_used as usize) % QSIZE];
        self.last_used = self.last_used.wrapping_add(1);
        self.ack_isr();
        elem.len
    }

    /// Disponibiliza o descritor `idx` como buffer de RECEPCAO (o
    /// dispositivo escreve nele). Usado pelas filas de evento do input.
    pub fn post_recv(&mut self, idx: u16, addr: u64, len: u32) {
        let q = unsafe { &mut *self.mem };
        q.desc[idx as usize] = Desc { addr, len, flags: DESC_F_WRITE, next: 0 };
        let slot = (q.avail.idx as usize) % QSIZE;
        q.avail.ring[slot] = idx;
        fence(Ordering::SeqCst);
        let new_idx = q.avail.idx.wrapping_add(1);
        unsafe { write_volatile(&raw mut q.avail.idx, new_idx) };
        fence(Ordering::SeqCst);
        reg_write(self.base, QUEUE_NOTIFY, self.index);
    }

    /// Verifica, SEM bloquear, se o dispositivo devolveu algum buffer.
    /// Retorna (indice do descritor, bytes escritos).
    pub fn poll_used(&mut self) -> Option<(u16, u32)> {
        let q = unsafe { &mut *self.mem };
        fence(Ordering::SeqCst);
        let used_idx = unsafe { read_volatile(&raw const q.used.idx) };
        if used_idx == self.last_used {
            return None;
        }
        let elem = q.used.ring[(self.last_used as usize) % QSIZE];
        self.last_used = self.last_used.wrapping_add(1);
        self.ack_isr();
        Some((elem.id as u16, elem.len))
    }

    fn ack_isr(&self) {
        let isr = reg_read(self.base, INTERRUPT_STATUS);
        if isr != 0 {
            reg_write(self.base, INTERRUPT_ACK, isr);
        }
    }
}
