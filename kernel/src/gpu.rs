//! Driver virtio-gpu (modo 2D) sobre virtio-mmio.
//!
//! Fluxo de inicializacao (spec virtio 1.1, secao 5.7):
//!   1. GET_DISPLAY_INFO      -> resolucao do scanout 0
//!   2. RESOURCE_CREATE_2D    -> cria o recurso host (id=1)
//!   3. RESOURCE_ATTACH_BACKING -> associa nosso framebuffer (RAM guest)
//!   4. SET_SCANOUT           -> liga o recurso ao display
//! E a cada frame:
//!   5. TRANSFER_TO_HOST_2D + RESOURCE_FLUSH -> copia e apresenta.
//!
//! O framebuffer vive em RAM do guest num endereco fixo (ver fb.rs),
//! identity-mapped, entao VA == PA para efeitos de DMA.

use crate::virtio::{self, QueueMem, VirtQueue};
use crate::uart::Uart;
use core::fmt::Write;

const VIRTIO_ID_GPU: u32 = 16;

// Tipos de comando/resposta
const CMD_GET_DISPLAY_INFO: u32 = 0x0100;
const CMD_RESOURCE_CREATE_2D: u32 = 0x0101;
const CMD_SET_SCANOUT: u32 = 0x0103;
const CMD_RESOURCE_FLUSH: u32 = 0x0104;
const CMD_TRANSFER_TO_HOST_2D: u32 = 0x0105;
const CMD_RESOURCE_ATTACH_BACKING: u32 = 0x0106;
const RESP_OK_NODATA: u32 = 0x1100;
const RESP_OK_DISPLAY_INFO: u32 = 0x1101;

/// Formato de pixel: bytes B,G,R,X => u32 little-endian 0x00RRGGBB.
const FORMAT_B8G8R8X8_UNORM: u32 = 2;

const RESOURCE_ID: u32 = 1;

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct CtrlHdr {
    hdr_type: u32,
    flags: u32,
    fence_id: u64,
    ctx_id: u32,
    ring_idx: u8,
    _pad: [u8; 3],
}

impl CtrlHdr {
    fn cmd(t: u32) -> Self {
        CtrlHdr { hdr_type: t, ..Default::default() }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Rect {
    x: u32,
    y: u32,
    width: u32,
    height: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct DisplayOne {
    r: Rect,
    enabled: u32,
    flags: u32,
}

#[repr(C)]
struct RespDisplayInfo {
    hdr: CtrlHdr,
    pmodes: [DisplayOne; 16],
}

#[repr(C)]
struct ResourceCreate2D {
    hdr: CtrlHdr,
    resource_id: u32,
    format: u32,
    width: u32,
    height: u32,
}

#[repr(C)]
struct AttachBacking {
    hdr: CtrlHdr,
    resource_id: u32,
    nr_entries: u32,
    // uma unica entrada, contigua (framebuffer fisico e continuo)
    addr: u64,
    length: u32,
    _pad: u32,
}

#[repr(C)]
struct SetScanout {
    hdr: CtrlHdr,
    r: Rect,
    scanout_id: u32,
    resource_id: u32,
}

#[repr(C)]
struct TransferToHost2D {
    hdr: CtrlHdr,
    r: Rect,
    offset: u64,
    resource_id: u32,
    _pad: u32,
}

#[repr(C)]
struct ResourceFlush {
    hdr: CtrlHdr,
    r: Rect,
    resource_id: u32,
    _pad: u32,
}

// --- Buffers estaticos de requisicao/resposta (DMA) ---------------------

#[repr(C, align(64))]
struct IoBuf([u8; 512]);

static mut CONTROLQ_MEM: QueueMem = QueueMem::zeroed();
static mut REQ: IoBuf = IoBuf([0; 512]);
static mut RESP: IoBuf = IoBuf([0; 512]);

struct Gpu {
    queue: VirtQueue,
    width: u32,
    height: u32,
}

static mut GPU: Option<Gpu> = None;

/// Copia `val` para o buffer de requisicao e devolve (ptr, len) para DMA.
unsafe fn stage_req<T>(val: &T) -> (u64, u32) {
    let len = size_of::<T>();
    unsafe {
        let dst = &raw mut REQ.0 as *mut u8;
        core::ptr::copy_nonoverlapping(val as *const T as *const u8, dst, len);
        (dst as u64, len as u32)
    }
}

/// Executa um comando sincrono e retorna o tipo da resposta.
unsafe fn do_cmd<T>(gpu: &mut Gpu, req: &T, resp_len: u32) -> u32 {
    unsafe {
        let out = stage_req(req);
        let resp_ptr = &raw mut RESP.0 as *mut u8 as u64;
        gpu.queue.request_sync(&[(out.0, out.1, false), (resp_ptr, resp_len, true)]);
        (*(resp_ptr as *const CtrlHdr)).hdr_type
    }
}

/// Inicializa o GPU. Retorna (largura, altura) do display, ou None.
pub fn init() -> Option<(u32, u32)> {
    let mut serial = Uart;

    let base = virtio::probe(VIRTIO_ID_GPU)?;
    let _ = write!(serial, "[gpu] virtio-gpu em {:#x}\n", base);

    if !virtio::init_device(base) {
        let _ = write!(serial, "[gpu] handshake de features falhou\n");
        return None;
    }

    let queue = unsafe { VirtQueue::setup(base, 0, &raw mut CONTROLQ_MEM)? };
    virtio::driver_ok(base);

    let mut gpu = Gpu { queue, width: 0, height: 0 };

    // 1) resolucao do scanout 0
    let req = CtrlHdr::cmd(CMD_GET_DISPLAY_INFO);
    let rtype = unsafe { do_cmd(&mut gpu, &req, size_of::<RespDisplayInfo>() as u32) };
    if rtype != RESP_OK_DISPLAY_INFO {
        let _ = write!(serial, "[gpu] GET_DISPLAY_INFO -> {:#x}\n", rtype);
        return None;
    }
    let info = unsafe { &*(&raw const RESP.0 as *const RespDisplayInfo) };
    let mode = &info.pmodes[0];
    let (w, h) = if mode.enabled != 0 && mode.r.width > 0 {
        (mode.r.width, mode.r.height)
    } else {
        (1024, 768)
    };
    gpu.width = w;
    gpu.height = h;
    let _ = write!(serial, "[gpu] scanout 0: {}x{}\n", w, h);

    // 2) recurso 2D no host
    let req = ResourceCreate2D {
        hdr: CtrlHdr::cmd(CMD_RESOURCE_CREATE_2D),
        resource_id: RESOURCE_ID,
        format: FORMAT_B8G8R8X8_UNORM,
        width: w,
        height: h,
    };
    if unsafe { do_cmd(&mut gpu, &req, size_of::<CtrlHdr>() as u32) } != RESP_OK_NODATA {
        let _ = write!(serial, "[gpu] RESOURCE_CREATE_2D falhou\n");
        return None;
    }

    // 3) backing = nosso framebuffer em RAM
    let fb_bytes = w as u64 * h as u64 * 4;
    let req = AttachBacking {
        hdr: CtrlHdr::cmd(CMD_RESOURCE_ATTACH_BACKING),
        resource_id: RESOURCE_ID,
        nr_entries: 1,
        addr: crate::fb::FB_BASE,
        length: fb_bytes as u32,
        _pad: 0,
    };
    if unsafe { do_cmd(&mut gpu, &req, size_of::<CtrlHdr>() as u32) } != RESP_OK_NODATA {
        let _ = write!(serial, "[gpu] ATTACH_BACKING falhou\n");
        return None;
    }

    // 4) liga recurso ao display
    let req = SetScanout {
        hdr: CtrlHdr::cmd(CMD_SET_SCANOUT),
        r: Rect { x: 0, y: 0, width: w, height: h },
        scanout_id: 0,
        resource_id: RESOURCE_ID,
    };
    if unsafe { do_cmd(&mut gpu, &req, size_of::<CtrlHdr>() as u32) } != RESP_OK_NODATA {
        let _ = write!(serial, "[gpu] SET_SCANOUT falhou\n");
        return None;
    }

    unsafe { GPU = Some(gpu) };
    Some((w, h))
}

/// Apresenta uma regiao: flip back->front + transfer + flush do rect.
/// (0,0,0,0) = tela inteira.
pub fn present_rect(x: u32, y: u32, rw: u32, rh: u32) {
    let gpu = unsafe { (&raw mut GPU).as_mut().unwrap().as_mut() };
    let Some(gpu) = gpu else { return };
    let (fw, fh) = (gpu.width, gpu.height);
    let (x, y, rw, rh) = if rw == 0 || rh == 0 {
        (0, 0, fw, fh)
    } else {
        (x.min(fw), y.min(fh), rw.min(fw - x.min(fw)), rh.min(fh - y.min(fh)))
    };

    crate::fb::flip_rect(x, y, rw, rh);

    let req = TransferToHost2D {
        hdr: CtrlHdr::cmd(CMD_TRANSFER_TO_HOST_2D),
        r: Rect { x, y, width: rw, height: rh },
        offset: ((y as u64 * fw as u64) + x as u64) * 4,
        resource_id: RESOURCE_ID,
        _pad: 0,
    };
    unsafe { do_cmd(gpu, &req, size_of::<CtrlHdr>() as u32) };

    let req = ResourceFlush {
        hdr: CtrlHdr::cmd(CMD_RESOURCE_FLUSH),
        r: Rect { x, y, width: rw, height: rh },
        resource_id: RESOURCE_ID,
        _pad: 0,
    };
    unsafe { do_cmd(gpu, &req, size_of::<CtrlHdr>() as u32) };
}

/// Apresenta a tela inteira (usado pelo splash do kernel).
pub fn present() {
    present_rect(0, 0, 0, 0);
}
