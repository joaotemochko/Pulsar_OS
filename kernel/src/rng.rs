//! Fonte de aleatoriedade para o userspace (SYS_GETRANDOM).
//!
//! Com um virtio-rng (QEMU: `-device virtio-rng-device`) os bytes vem da
//! entropia do host. Sem ele, cai num gerador misturando o contador de
//! hardware e o RTC — suficiente para nao repetir valores, mas NAO serve
//! para criptografia (o TLS do Vela avisa nesse caso).

use crate::sync::SpinLock;
use crate::uart::Uart;
use crate::virtio::{self, QueueMem, VirtQueue};
use core::fmt::Write;

const VIRTIO_ID_RNG: u32 = 4;
const CHUNK: usize = 256;

#[repr(C, align(64))]
struct Buf([u8; CHUNK]);

static mut RNGQ_MEM: QueueMem = QueueMem::zeroed();
static mut BUF: Buf = Buf([0; CHUNK]);
static mut QUEUE: Option<VirtQueue> = None;
static mut FALLBACK: u64 = 0;
/// Trava folha: nao pega nenhuma outra.
static RNG: SpinLock = SpinLock::new();

pub fn init() -> bool {
    let mut serial = Uart;
    let Some(base) = virtio::probe(VIRTIO_ID_RNG) else {
        serial.write_string("[rng] sem virtio-rng: usando contador (nao criptografico)\n");
        return false;
    };
    if !virtio::init_device(base) {
        return false;
    }
    let Some(q) = (unsafe { VirtQueue::setup(base, 0, &raw mut RNGQ_MEM) }) else { return false };
    virtio::driver_ok(base);
    unsafe { QUEUE = Some(q) };
    let _ = write!(serial, "[rng] virtio-rng em {:#x}\n", base);
    true
}

/// O gerador e de qualidade criptografica (veio do host)?
pub fn is_strong() -> bool {
    unsafe { (*(&raw const QUEUE)).is_some() }
}

/// splitmix64 sobre contador + RTC (so quando nao ha dispositivo).
fn fallback_next() -> u64 {
    unsafe {
        FALLBACK = FALLBACK
            .wrapping_add(0x9E37_79B9_7F4A_7C15)
            .wrapping_add(crate::arch::now() ^ (crate::arch::rtc_seconds() << 32));
        let mut z = FALLBACK;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
}

/// Preenche `out` com bytes aleatorios.
pub fn fill(out: &mut [u8]) {
    let _g = RNG.guard();
    let mut done = 0;
    while done < out.len() {
        let want = (out.len() - done).min(CHUNK);
        let got = unsafe {
            match (*(&raw mut QUEUE)).as_mut() {
                Some(q) => {
                    let pa = (&raw mut BUF.0) as u64;
                    (q.request_sync(&[(pa, want as u32, true)]) as usize).min(want)
                }
                None => {
                    for i in 0..want {
                        BUF.0[i] = (fallback_next() >> 24) as u8;
                    }
                    want
                }
            }
        };
        if got == 0 {
            continue;
        }
        unsafe { out[done..done + got].copy_from_slice(&BUF.0[..got]) };
        done += got;
    }
}
