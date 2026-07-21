//! ABI de syscalls do Pulsar OS.
//!
//! Convencao: numero em x8, argumentos em x0..x2, retorno em x0.
//!
//!  1 WRITE(ptr, len)        escreve texto UTF-8 na serial
//!  2 YIELD                  cede a CPU voluntariamente
//!  3 EXIT                   termina o processo atual
//!  4 FB_INFO                retorna (largura << 32) | altura, 0 se sem video
//!  5 FB_MAP                 mapeia o framebuffer p/ EL0; retorna endereco base
//!  6 FB_PRESENT             apresenta o framebuffer na tela
//!  7 INPUT_POLL             retorna evento: bit63=valido | type<<40 | code<<32 | value
//!  8 FS_COUNT               numero de arquivos no PulsarFS
//!  9 FS_STAT(idx, ptr)      escreve FileEntry (name+start+size+kind) em ptr; ret size
//! 10 SPAWN(ptr, len)        carrega e executa <nome>.pulse do disco; ret pid
//! 11 IPC_SEND(dst, msgptr)  envia msg e bloqueia ate reply (msgptr recebe resposta)
//! 12 IPC_RECV(msgptr)       bloqueia ate receber; retorna pid do remetente
//! 13 IPC_REPLY(to, msgptr)  responde ao remetente e o desbloqueia
//! 14 GETPID                 retorna o pid do processo atual
//! 15 FS_READ(idx,dst,max)   le arquivo para dst (ate max bytes); ret bytes
//! 16 FS_WRITE(idx,src,size) escreve size bytes de src no arquivo; ret 0/erro
//! 17 FS_CREATE(name,len)    cria arquivo de texto vazio; ret idx/erro
//!
//! NOTA de arquitetura: FB_MAP torna o framebuffer acessivel a TODOS os
//! processos (o espaco de enderecos e compartilhado — TTBR0 unico por
//! enquanto). Isolamento por-processo de verdade exige tabelas por
//! processo + ASID, que e a proxima fronteira do kernel.

use crate::context::Context;
use crate::process;
use crate::uart::Uart;
use core::fmt::Write;
use core::slice;
use core::str;

const SYS_WRITE: u64 = 1;
const SYS_YIELD: u64 = 2;
const SYS_EXIT: u64 = 3;
const SYS_FB_INFO: u64 = 4;
const SYS_FB_MAP: u64 = 5;
const SYS_FB_PRESENT: u64 = 6;
const SYS_INPUT_POLL: u64 = 7;
const SYS_FS_COUNT: u64 = 8;
const SYS_FS_STAT: u64 = 9;
const SYS_SPAWN: u64 = 10;
// IPC
const SYS_IPC_SEND: u64 = 11;   // (dst, msg_ptr) -> preenche msg_ptr com reply
const SYS_IPC_RECV: u64 = 12;   // (msg_ptr) -> retorna remetente
const SYS_IPC_REPLY: u64 = 13;  // (to, msg_ptr)
const SYS_GETPID: u64 = 14;
// Filesystem
const SYS_FS_READ: u64 = 15;    // (idx, dst_ptr, max) -> bytes lidos
const SYS_FS_WRITE: u64 = 16;   // (idx, src_ptr, size) -> 0 ok / <0 erro
const SYS_FS_CREATE: u64 = 17;  // (name_ptr, name_len) -> idx / <0 erro
const SYS_SET_FOCUS: u64 = 18;  // (pid) transfere o foco de teclado
const SYS_REGISTER: u64 = 19;   // (service_id) registra o processo atual como servidor
const SYS_LOOKUP: u64 = 20;     // (service_id) -> pid do servidor, ou -1
const SYS_SURF_MAP: u64 = 21;   // (slot) mapeia a superficie do slot p/ EL0; ret endereco
const SYS_SURF_INFO: u64 = 22;  // () -> (w<<16|h) dimensoes maximas da superficie
const SYS_IPC_TRY_RECV: u64 = 23; // (msg_ptr) -> remetente, ou -1 se vazio (nao bloqueia)
const SYS_IS_ALIVE: u64 = 24;   // (pid) -> 1 se processo vivo, 0 senao
const SYS_UPTIME: u64 = 25;     // () -> ticks desde boot (100Hz, 10ms cada)
const SYS_NET_STATUS: u64 = 26; // () -> IP empacotado (a<<24|b<<16|c<<8|d), 0 se sem rede
const SYS_NET_UDP_SEND: u64 = 27; // (ip_be, ports=(dport<<16|sport), buf_ptr<<32|len) -> 1 ok
const SYS_HTTP_GET: u64 = 28;   // (ip_be, port, req_ptr) req = {host_len,path_len,host,path}; ret len; copia p/ out

#[unsafe(no_mangle)]
pub extern "C" fn rust_el0_sync_handler(frame: *mut Context) {
    let ctx = unsafe { &mut *frame };

    let esr_val: u64;
    unsafe {
        core::arch::asm!("mrs {}, esr_el1", out(reg) esr_val,
            options(nomem, nostack, preserves_flags));
    }
    let ec = (esr_val >> 26) & 0x3F;

    if ec != 0x15 {
        // Excecao sincrona de EL0 que nao e SVC = fault do programa.
        let mut serial = Uart;
        let far: u64;
        unsafe {
            core::arch::asm!("mrs {}, far_el1", out(reg) far,
                options(nomem, nostack, preserves_flags));
        }
        let elr = ctx.elr;
        let name = match ec {
            0x20 | 0x21 => "Instruction Abort",
            0x24 | 0x25 => "Data Abort",
            _ => "excecao sincrona",
        };
        let _ = write!(serial,
            "\n[FAULT EL0] {} — EC={:#x} endereco_acessado(FAR)={:#x} instrucao(ELR)={:#x}\n             [kernel] o hardware bloqueou o acesso: o VA nao pertence ao espaco do processo.\n",
            name, ec, far, elr);
        kill_current(frame);
        return;
    }

    let num = ctx.x[8];
    let a1 = ctx.x[0];
    let a2 = ctx.x[1];
    let a3 = ctx.x[2];

    match num {
        SYS_WRITE => ctx.x[0] = sys_write(a1, a2) as u64,
        SYS_YIELD => unsafe { process::schedule(frame) },
        SYS_EXIT => {
            let mut serial = Uart;
            let (dead, has_next) = unsafe { process::exit_current(frame) };
            let _ = write!(serial, "[kernel] processo {} saiu.\n", dead);
            if !has_next {
                let _ = write!(serial, "[kernel] nenhum processo restante. Sistema ocioso.\n");
                loop { unsafe { core::arch::asm!("wfe") }; }
            }
        }
        SYS_FB_INFO => {
            let (w, h) = crate::fb::size();
            ctx.x[0] = ((w as u64) << 32) | h as u64;
        }
        SYS_FB_MAP => {
            crate::fb::map_for_user();
            ctx.x[0] = crate::fb::BACK_BASE;
        }
        SYS_FB_PRESENT => {
            // a1 = x<<32|y, a2 = w<<32|h; (0,0) = tela inteira
            crate::gpu::present_rect(
                (a1 >> 32) as u32, a1 as u32,
                (a2 >> 32) as u32, a2 as u32,
            );
        }
        SYS_INPUT_POLL => {
            ctx.x[0] = match crate::input::poll_for(process::current()) {
                // Layout: bit63=valido | type em [48..62] | code em [32..47] | value em [0..31]
                Some(ev) => (1u64 << 63)
                    | ((ev.ev_type as u64) << 48)
                    | ((ev.code as u64) << 32)
                    | ev.value as u64,
                None => 0,
            };
        }
        SYS_FS_COUNT => ctx.x[0] = crate::fs::count() as u64,
        SYS_FS_STAT => ctx.x[0] = sys_fs_stat(a1, a2) as u64,
        SYS_SPAWN => ctx.x[0] = sys_spawn(a1, a2) as u64,
        SYS_GETPID => ctx.x[0] = process::current() as u64,
        SYS_IPC_SEND => sys_ipc_send(frame, ctx, a1, a2),
        SYS_IPC_RECV => sys_ipc_recv(frame, ctx, a1),
        SYS_IPC_REPLY => sys_ipc_reply(frame, ctx, a1, a2),
        SYS_FS_READ => ctx.x[0] = sys_fs_read(a1, a2, ctx.x[2]) as u64,
        SYS_FS_WRITE => ctx.x[0] = sys_fs_write(a1, a2, ctx.x[2]) as u64,
        SYS_FS_CREATE => ctx.x[0] = sys_fs_create(a1, a2) as u64,
        SYS_SET_FOCUS => { crate::input::set_focus(a1 as usize); ctx.x[0] = 0; }
        SYS_REGISTER => { register_service(a1, process::current()); ctx.x[0] = 0; }
        SYS_LOOKUP => ctx.x[0] = lookup_service(a1) as u64,
        SYS_SURF_MAP => {
            let slot = a1;
            if slot < crate::fb::SURF_SLOTS {
                crate::fb::map_surface_for_user(slot);
                ctx.x[0] = crate::fb::surf_addr(slot);
            } else {
                ctx.x[0] = 0;
            }
        }
        SYS_SURF_INFO => {
            ctx.x[0] = ((512u64) << 16) | 512;
        }
        SYS_IPC_TRY_RECV => {
            unsafe { process::ipc_try_recv(frame, a1); }
        }
        SYS_IS_ALIVE => { ctx.x[0] = if process::is_alive(a1 as usize) {1} else {0}; }
        SYS_UPTIME => { ctx.x[0] = crate::timer::uptime(); }
        SYS_NET_STATUS => {
            crate::net::poll();
            let ip = crate::net::OUR_IP;
            ctx.x[0] = if crate::net::is_up() {
                ((ip[0] as u64)<<24)|((ip[1] as u64)<<16)|((ip[2] as u64)<<8)|(ip[3] as u64)
            } else { 0 };
        }
        SYS_HTTP_GET => {
            let ip = [ (a1>>24) as u8, (a1>>16) as u8, (a1>>8) as u8, a1 as u8 ];
            let port = a2 as u16;
            let ptr = a3 as usize;
            ctx.x[0] = 0;
            if ptr != 0 {
                let hdr = unsafe { core::slice::from_raw_parts(ptr as *const u8, 4) };
                let host_len = ((hdr[0] as usize)<<8)|hdr[1] as usize;
                let path_len = ((hdr[2] as usize)<<8)|hdr[3] as usize;
                if host_len<=256 && path_len<=256 {
                    let host = unsafe { core::slice::from_raw_parts((ptr+4) as *const u8, host_len) };
                    let path = unsafe { core::slice::from_raw_parts((ptr+4+host_len) as *const u8, path_len) };
                    let (rptr, rlen) = crate::net::http_get(&ip, port, host, path);
                    if rlen > 0 {
                        // copia o corpo para o buffer do app a partir do offset 512
                        let n = rlen.min(15000);
                        unsafe {
                            core::ptr::copy_nonoverlapping(rptr as *const u8, (ptr+512) as *mut u8, n);
                        }
                        ctx.x[0] = n as u64;
                    }
                }
            }
        }
        SYS_NET_UDP_SEND => {
            // a1 = ip destino (a<<24|b<<16|c<<8|d)
            // a2 = (dport<<16)|sport
            // a3 = (buf_ptr<<32)|len   -> ptr em EL0, len bytes
            let ip = [ (a1>>24) as u8, (a1>>16) as u8, (a1>>8) as u8, a1 as u8 ];
            let dport = (a2>>16) as u16; let sport = a2 as u16;
            let ptr = (a3>>32) as usize; let len = (a3 & 0xFFFF_FFFF) as usize;
            let ok = if ptr!=0 && len<=1400 {
                let data = unsafe { core::slice::from_raw_parts(ptr as *const u8, len) };
                crate::net::udp_send(&ip, dport, sport, data)
            } else { false };
            ctx.x[0] = if ok {1} else {0};
        }
        _ => ctx.x[0] = (-1i64) as u64,
    }
}

fn kill_current(frame: *mut Context) {
    let mut serial = Uart;
    let (dead, has_next) = unsafe { process::exit_current(frame) };
    let _ = write!(serial, "[kernel] processo {} terminado.\n", dead);
    if !has_next {
        let _ = write!(serial, "[kernel] nenhum processo restante. Sistema ocioso.\n");
        loop { unsafe { core::arch::asm!("wfe") }; }
    }
}

fn sys_write(ptr: u64, len: u64) -> i64 {
    if ptr == 0 || len == 0 || len > 4096 {
        return -2;
    }
    let bytes = unsafe { slice::from_raw_parts(ptr as *const u8, len as usize) };
    match str::from_utf8(bytes) {
        Ok(s) => {
            Uart.write_string(s);
            len as i64
        }
        Err(_) => -3,
    }
}

/// Copia a entrada `idx` da tabela do PulsarFS para o buffer do usuario.
/// Layout copiado: name[32] + start_sector u32 + size u32 + kind u32 (44 bytes).
fn sys_fs_stat(idx: u64, ptr: u64) -> i64 {
    if ptr == 0 || idx as usize >= crate::fs::count() {
        return -1;
    }
    let e = crate::fs::entry(idx as usize);
    unsafe {
        core::ptr::write_bytes(ptr as *mut u8, 0, 32);
        core::ptr::copy_nonoverlapping(e.name.as_ptr(), ptr as *mut u8, 28);
        core::ptr::write_unaligned((ptr + 32) as *mut u32, e.start_sector);
        core::ptr::write_unaligned((ptr + 36) as *mut u32, e.size_bytes);
        core::ptr::write_unaligned((ptr + 40) as *mut u32, e.kind);
    }
    e.size_bytes as i64
}

fn sys_spawn(ptr: u64, len: u64) -> i64 {
    if ptr == 0 || len == 0 || len > 32 {
        return -1;
    }
    let bytes = unsafe { slice::from_raw_parts(ptr as *const u8, len as usize) };
    let Ok(name) = str::from_utf8(bytes) else { return -1 };
    match crate::loader::spawn_from_fs(name) {
        Some(pid) => pid as i64,
        None => -1,
    }
}

// ---------------------------------------------------------- name server

// Registro de servicos por id numerico. Ex.: 1 = filesystem daemon.
static mut SERVICES: [usize; 8] = [usize::MAX; 8];

fn register_service(id: u64, pid: usize) {
    if (id as usize) < 8 {
        unsafe { SERVICES[id as usize] = pid; }
    }
}

fn lookup_service(id: u64) -> i64 {
    if (id as usize) < 8 {
        let pid = unsafe { SERVICES[id as usize] };
        if pid != usize::MAX && process::is_alive(pid) {
            return pid as i64;
        }
    }
    -1
}

// ------------------------------------------------------------------ IPC

use crate::process::Message;

/// Le uma Message do buffer do usuario (tag + 6 palavras = 56 bytes).
unsafe fn read_msg(ptr: u64) -> Message {
    unsafe {
        let mut m = Message::zero();
        m.tag = core::ptr::read_unaligned(ptr as *const u64);
        for i in 0..6 {
            m.data[i] = core::ptr::read_unaligned((ptr + 8 + (i as u64) * 8) as *const u64);
        }
        m
    }
}

fn sys_ipc_send(frame: *mut Context, ctx: &mut Context, dst: u64, msg_ptr: u64) {
    if msg_ptr == 0 {
        ctx.x[0] = (-1i64) as u64;
        return;
    }
    let msg = unsafe { read_msg(msg_ptr) };
    // O retorno (x0=0 e a resposta em msg_ptr) sera gravado no contexto
    // salvo deste processo pelo kernel quando o reply chegar. Se falhar
    // imediatamente (dst invalido), sinaliza -1 no frame corrente.
    if !unsafe { process::ipc_send(frame, dst as usize, msg, msg_ptr) } {
        unsafe { (*frame).x[0] = (-1i64) as u64 };
    }
}

fn sys_ipc_recv(frame: *mut Context, _ctx: &mut Context, msg_ptr: u64) {
    // ipc_recv escreve o remetente em x0 e a msg no buffer — inline se ha
    // emissor pronto, ou via deliver_recv quando um chegar.
    unsafe { process::ipc_recv(frame, msg_ptr) };
}

fn sys_ipc_reply(frame: *mut Context, ctx: &mut Context, to: u64, msg_ptr: u64) {
    let msg = if msg_ptr != 0 { unsafe { read_msg(msg_ptr) } } else { Message::zero() };
    unsafe { process::ipc_reply(frame, to as usize, msg) };
    let _ = ctx;
}

// ------------------------------------------------------------ Filesystem

fn sys_fs_read(idx: u64, dst: u64, max: u64) -> i64 {
    if dst == 0 {
        return -1;
    }
    // le para um buffer fisico temporario e copia ate `max` bytes
    let idx = idx as usize;
    if idx >= crate::fs::count() {
        return -1;
    }
    let size = crate::fs::entry(idx).size_bytes as u64;
    let want = size.min(max);
    // buffer de staging: 1 frame por vez seria complexo; aqui usamos um
    // buffer estatico de 64KB (suficiente para textos do editor).
    static mut STAGE: [u8; 65536] = [0; 65536];
    let stage_pa = unsafe { &raw mut STAGE as *mut u8 as u64 };
    if size as usize > 65536 {
        return -2;
    }
    if crate::fs::read_file(idx, stage_pa).is_none() {
        return -3;
    }
    unsafe {
        core::ptr::copy_nonoverlapping(stage_pa as *const u8, dst as *mut u8, want as usize);
    }
    want as i64
}

fn sys_fs_write(idx: u64, src: u64, size: u64) -> i64 {
    if src == 0 || size > 65536 {
        return -1;
    }
    static mut STAGE: [u8; 65536] = [0; 65536];
    let stage_pa = unsafe { &raw mut STAGE as *mut u8 as u64 };
    unsafe {
        core::ptr::copy_nonoverlapping(src as *const u8, stage_pa as *mut u8, size as usize);
        // zera o resto do ultimo setor para nao vazar lixo
        let secbytes = ((size as usize).div_ceil(512)) * 512;
        core::ptr::write_bytes((stage_pa as *mut u8).add(size as usize), 0, secbytes - size as usize);
    }
    if crate::fs::write_file(idx as usize, stage_pa, size as u32) {
        0
    } else {
        -2
    }
}

fn sys_fs_create(name_ptr: u64, name_len: u64) -> i64 {
    if name_ptr == 0 || name_len == 0 || name_len > 31 {
        return -1;
    }
    let bytes = unsafe { slice::from_raw_parts(name_ptr as *const u8, name_len as usize) };
    let Ok(name) = str::from_utf8(bytes) else { return -1 };
    // 16 setores (8KB) de capacidade para textos novos
    match crate::fs::create(name, 2, 16) {
        Some(idx) => idx as i64,
        None => -2,
    }
}
