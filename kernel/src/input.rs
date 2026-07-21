//! Driver virtio-input: teclado e mouse absoluto (tablet) do QEMU.
//!
//! Cada dispositivo tem uma *eventq* (fila 0) onde NOS postamos buffers
//! vazios e o dispositivo escreve eventos de 8 bytes:
//!   { type: u16, code: u16, value: u32 }   (semantica evdev do Linux)
//!
//! Ha DOIS dispositivos com DeviceID 18 na linha de comando do QEMU
//! (teclado e tablet); distinguimos consultando o espaco de config:
//! select=EV_BITS(0x11) subsel=EV_ABS(3) -> size>0 significa tablet.
//!
//! Operacao por polling (a syscall SYS_INPUT_POLL drena as filas).

use crate::virtio::{self, QueueMem, VirtQueue};
use crate::uart::Uart;
use core::fmt::Write;

const VIRTIO_ID_INPUT: u32 = 18;
const NBUF: u16 = 16; // buffers de evento postados por dispositivo

// Tipos de evento (evdev)
pub const EV_SYN: u16 = 0;
pub const EV_KEY: u16 = 1;
pub const EV_REL: u16 = 2;
pub const EV_ABS: u16 = 3;

// Config space do virtio-input
const CFG_SELECT: u64 = 0;
const CFG_SUBSEL: u64 = 1;
const CFG_SIZE: u64 = 2;
const SEL_EV_BITS: u8 = 0x11;

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct InputEvent {
    pub ev_type: u16,
    pub code: u16,
    pub value: u32,
}

#[repr(C, align(64))]
struct EventBufs([InputEvent; NBUF as usize]);

struct InputDev {
    queue: VirtQueue,
    bufs: *mut EventBufs,
}

static mut KBD_QMEM: QueueMem = QueueMem::zeroed();
static mut PTR_QMEM: QueueMem = QueueMem::zeroed();
static mut KBD_BUFS: EventBufs = EventBufs([InputEvent { ev_type: 0, code: 0, value: 0 }; NBUF as usize]);
static mut PTR_BUFS: EventBufs = EventBufs([InputEvent { ev_type: 0, code: 0, value: 0 }; NBUF as usize]);

static mut DEVS: [Option<InputDev>; 2] = [None, None];

/// O dispositivo em `base` reporta eventos EV_ABS? (=> e o tablet)
fn has_abs(base: u64) -> bool {
    virtio::config_write8(base, CFG_SELECT, SEL_EV_BITS);
    virtio::config_write8(base, CFG_SUBSEL, EV_ABS as u8);
    virtio::config_read8(base, CFG_SIZE) > 0
}

fn setup_dev(base: u64, qmem: *mut QueueMem, bufs: *mut EventBufs) -> Option<InputDev> {
    if !virtio::init_device(base) {
        return None;
    }
    let mut queue = unsafe { VirtQueue::setup(base, 0, qmem)? };
    virtio::driver_ok(base);

    // Posta todos os buffers de recepcao
    for i in 0..NBUF {
        let addr = unsafe { (&raw mut (*bufs).0).cast::<InputEvent>().add(i as usize) as u64 };
        queue.post_recv(i, addr, size_of::<InputEvent>() as u32);
    }
    Some(InputDev { queue, bufs })
}

/// Inicializa teclado e tablet. Retorna quantos dispositivos achou.
pub fn init() -> usize {
    let mut serial = Uart;
    let (bases, n) = virtio::probe_all::<2>(VIRTIO_ID_INPUT);
    let mut found = 0;

    for &base in bases.iter().take(n) {
        let is_tablet = has_abs(base);
        let (qmem, bufs, slot, label) = if is_tablet {
            (&raw mut PTR_QMEM, &raw mut PTR_BUFS, 1usize, "tablet")
        } else {
            (&raw mut KBD_QMEM, &raw mut KBD_BUFS, 0usize, "teclado")
        };
        if let Some(dev) = setup_dev(base, qmem, bufs) {
            let _ = write!(serial, "[input] {} em {:#x}\n", label, base);
            unsafe { DEVS[slot] = Some(dev) };
            found += 1;
        }
    }
    found
}

// Slot 0 = teclado, slot 1 = tablet(mouse). Filas separadas de eventos.
// O teclado e roteado para o processo com FOCO; o mouse vai para quem
// pedir (o shell/WM sempre le o mouse para mover o cursor).
static mut FOCUS_PID: usize = 0; // processo que recebe o teclado

/// Define qual processo recebe eventos de teclado.
pub fn set_focus(pid: usize) {
    unsafe { FOCUS_PID = pid; }
}

pub fn focus() -> usize {
    unsafe { FOCUS_PID }
}

/// Drena um evento do dispositivo `slot`, sem bloquear. Filtra EV_SYN.
fn drain(slot: usize) -> Option<InputEvent> {
    let dev = unsafe { (&raw mut DEVS[slot]).as_mut().unwrap().as_mut() };
    let dev = dev?;
    while let Some((idx, len)) = dev.queue.poll_used() {
        let ev = unsafe {
            core::ptr::read_volatile(
                (&raw const (*dev.bufs).0).cast::<InputEvent>().add(idx as usize),
            )
        };
        let addr = unsafe { (&raw mut (*dev.bufs).0).cast::<InputEvent>().add(idx as usize) as u64 };
        dev.queue.post_recv(idx, addr, size_of::<InputEvent>() as u32);
        if len as usize >= size_of::<InputEvent>() && ev.ev_type != EV_SYN {
            return Some(ev);
        }
    }
    None
}

/// Poll para o processo `pid`: mouse sempre; teclado so se tiver foco.
/// Quando um processo sem foco chama, so recebe eventos de mouse.
pub fn poll_for(pid: usize) -> Option<InputEvent> {
    // mouse (tablet) primeiro — qualquer um pode ler
    if let Some(ev) = drain(1) {
        return Some(ev);
    }
    // teclado — so o processo com foco drena a fila
    if pid == unsafe { FOCUS_PID } {
        return drain(0);
    }
    None
}

/// Compatibilidade: poll sem roteamento (usado internamente).
pub fn poll_event() -> Option<InputEvent> {
    if let Some(ev) = drain(1) { return Some(ev); }
    drain(0)
}
