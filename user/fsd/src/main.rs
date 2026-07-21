//! fsd.pulse — Filesystem Daemon. Servidor de IPC que expoe o PulsarFS.
//! Roda em EL0, registra-se como SVC_FS, e atende count/stat/read via
//! mensagens. Prova o modelo microkernel: os apps nao tocam o disco
//! diretamente — pedem ao servidor.
#![no_std]
#![no_main]
use plib::*;
#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! { write("[fsd] PANIC\n"); exit() }

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    write("[fsd] filesystem daemon iniciando\n");
    register_service(SVC_FS);
    write("[fsd] registrado como SVC_FS, aguardando pedidos\n");

    let mut m = Msg::new(0);
    loop {
        let from = recv(&mut m);
        let mut r = Msg::new(m.tag);
        match m.tag {
            x if x == FS_OP_COUNT => {
                r.data[0] = fs_count() as u64;
            }
            x if x == FS_OP_STAT => {
                let idx = m.data[0] as usize;
                let mut st = [0u8; 44];
                let size = fs_stat(idx, &mut st);
                r.data[0] = size as u64;
                let kind = u32::from_le_bytes([st[40],st[41],st[42],st[43]]);
                r.data[1] = kind as u64;
                // nome em 4 palavras (32 bytes) -> data[2..6]
                for w in 0..4 {
                    let mut word = 0u64;
                    for b in 0..8 {
                        word |= (st[w*8+b] as u64) << (b*8);
                    }
                    r.data[2+w] = word;
                }
            }
            x if x == FS_OP_READ => {
                // le ate 40 bytes do arquivo idx a partir de offset
                let idx = m.data[0] as usize;
                let off = m.data[1] as usize;
                let mut buf = [0u8; 2048];
                let got = fs_read(idx, &mut buf);
                let mut n = 0u64;
                if got > 0 {
                    let total = got as usize;
                    for w in 0..5 { // 5 palavras = 40 bytes
                        let mut word = 0u64;
                        for b in 0..8 {
                            let p = off + w*8 + b;
                            if p < total { word |= (buf[p] as u64) << (b*8); n += 1; }
                        }
                        r.data[1+w] = word;
                    }
                }
                r.data[0] = n; // bytes validos nesta resposta
            }
            _ => { r.data[0] = u64::MAX; }
        }
        reply(from, &r);
    }
}
