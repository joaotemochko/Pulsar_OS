//! ABI de syscalls do Pulsar OS.
//!
//! Convencao: AArch64 `svc #0` com numero em x8, argumentos em x0..x2 e
//! retorno em x0; RISC-V `ecall` com numero em a7, argumentos em a0..a2 e
//! retorno em a0. Os numeros sao os mesmos nas duas arquiteturas.
//!
//!  1 WRITE(ptr, len)         escreve texto UTF-8 na serial
//!  2 YIELD                   cede a CPU
//!  3 EXIT                    termina o processo atual
//!  4 FB_INFO                 (largura << 32) | altura, 0 se sem video
//!  5 FB_MAP                  mapeia a regiao grafica; retorna VA do back buffer
//!  6 FB_PRESENT(xy, wh)      apresenta um retangulo ((0,0) = tela toda)
//!  7 INPUT_POLL              evento: bit63=valido | type<<48 | code<<32 | value
//!  8 FS_COUNT                numero de arquivos
//!  9 FS_STAT(idx, ptr)       name[32] start u32 size u32 kind u32; ret size
//! 10 SPAWN(ptr, len)         executa <nome> do disco; ret pid
//! 11 IPC_SEND(dst, msg)      envia e bloqueia ate o reply (msg recebe a resposta)
//! 12 IPC_RECV(msg)           bloqueia ate receber; ret pid do remetente
//! 13 IPC_REPLY(to, msg)      responde e desbloqueia o remetente
//! 14 GETPID
//! 15 FS_READ(idx, dst, max)  le arquivo; ret bytes
//! 16 FS_WRITE(idx, src, n)   escreve arquivo; ret 0 / <0
//! 17 FS_CREATE(name, len)    cria arquivo de texto vazio; ret idx / <0
//! 18 SET_FOCUS(pid)          processo que recebe o teclado
//! 19 REGISTER(svc)           registra o processo como servidor do servico
//! 20 LOOKUP(svc)             pid do servidor, ou -1
//! 21 SURF_MAP(slot)          mapeia a superficie do slot; ret VA
//! 22 SURF_INFO               (stride << 16) | altura maxima
//! 23 IPC_TRY_RECV(msg)       como RECV, mas -1 se nao houver mensagem
//! 24 IS_ALIVE(pid)
//! 25 UPTIME                  ticks de 10 ms desde o boot
//! 26 NET_STATUS              IP (a<<24|b<<16|c<<8|d) ou 0
//! 27 NET_UDP_SEND(ip, ports, ptr<<32|len)
//! 28 (removida: o HTTP agora e feito no userspace sobre TCP)
//! 29 SLEEP(ms)               dorme sem consumir CPU
//! 30 TIME_MS                 milissegundos desde o boot (contador de hardware)
//! 31 RTC                     segundos desde a epoca Unix (relogio de parede)
//! 32 SYSINFO(ptr)            ver SysInfo abaixo
//! 33 PROC_INFO(slot, ptr)    ver ProcInfo abaixo; ret 0, -1 slot vazio, -2 fim
//! 34 KILL(pid)               termina um processo
//! 35 CPU_LOAD(cpu)           % de uso da CPU no ultimo segundo, -1 se offline
//! 36 POWEROFF                desliga a maquina
//! 37 CURSOR_SET(img, hot, pos)  img = 64x64 u32 ARGB; hot = hx<<16|hy; pos = x<<32|y
//! 38 CURSOR_MOVE(x, y)       move o cursor de hardware
//! 39 LAST_CRASH              pid do ultimo processo morto por fault, ou -1
//! 40 IPC_RECV_TIMEOUT(msg, ms) como RECV, mas retorna -1 apos `ms` sem mensagem
//! 41 TCP_CONNECT(ip, port)   abre conexao (nao bloqueia); ret handle ou -1
//! 42 TCP_SEND(h, ptr, len)   bytes aceitos (0 = tente de novo), -1 erro
//! 43 TCP_RECV(h, ptr, max)   bytes lidos, 0 fim, -2 nada ainda, -1 erro
//! 44 TCP_CLOSE(h)
//! 45 TCP_STATUS(h)           0 conectando, 1 aberta, 2 fechada pelo par, -1 erro
//! 46 UDP_RECV(port, ptr, max) [ip:4][porta:2][0:2] + dados; ret total ou -2
//! 47 SBRK(inc)               aumenta o heap em `inc` bytes; ret fim anterior ou -1
//! 48 GETRANDOM(ptr, len)     bytes aleatorios (len <= 4096); ret len ou -1.
//!                            GETRANDOM(0, 0): 1 se a fonte e criptografica
//! 49 THREAD_CREATE(entry, stack_top, arg)  nova thread no mesmo processo; ret tid ou -1
//! 50 FUTEX_WAIT(addr, val, ms) dorme se *addr == val; 0 acordado, -1 mudou, -2 tempo
//! 51 FUTEX_WAKE(addr, n)     acorda ate n threads esperando em addr; ret quantas
//! 52 EXIT_GROUP              termina o processo inteiro (todas as threads)

use crate::arch::{self, Context};
use crate::process::{self, Message};
use crate::sync::SpinLock;
use crate::uart::Uart;
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
const SYS_IPC_SEND: u64 = 11;
const SYS_IPC_RECV: u64 = 12;
const SYS_IPC_REPLY: u64 = 13;
const SYS_GETPID: u64 = 14;
const SYS_FS_READ: u64 = 15;
const SYS_FS_WRITE: u64 = 16;
const SYS_FS_CREATE: u64 = 17;
const SYS_SET_FOCUS: u64 = 18;
const SYS_REGISTER: u64 = 19;
const SYS_LOOKUP: u64 = 20;
const SYS_SURF_MAP: u64 = 21;
const SYS_SURF_INFO: u64 = 22;
const SYS_IPC_TRY_RECV: u64 = 23;
const SYS_IS_ALIVE: u64 = 24;
const SYS_UPTIME: u64 = 25;
const SYS_NET_STATUS: u64 = 26;
const SYS_NET_UDP_SEND: u64 = 27;

const SYS_SLEEP: u64 = 29;
const SYS_TIME_MS: u64 = 30;
const SYS_RTC: u64 = 31;
const SYS_SYSINFO: u64 = 32;
const SYS_PROC_INFO: u64 = 33;
const SYS_KILL: u64 = 34;
const SYS_CPU_LOAD: u64 = 35;
const SYS_POWEROFF: u64 = 36;
const SYS_CURSOR_SET: u64 = 37;
const SYS_CURSOR_MOVE: u64 = 38;
const SYS_LAST_CRASH: u64 = 39;
const SYS_IPC_RECV_TIMEOUT: u64 = 40;
const SYS_TCP_CONNECT: u64 = 41;
const SYS_TCP_SEND: u64 = 42;
const SYS_TCP_RECV: u64 = 43;
const SYS_TCP_CLOSE: u64 = 44;
const SYS_TCP_STATUS: u64 = 45;
const SYS_UDP_RECV: u64 = 46;
const SYS_SBRK: u64 = 47;
const SYS_GETRANDOM: u64 = 48;
const SYS_THREAD_CREATE: u64 = 49;
const SYS_FUTEX_WAIT: u64 = 50;
const SYS_FUTEX_WAKE: u64 = 51;
const SYS_EXIT_GROUP: u64 = 52;

const ERR: u64 = u64::MAX; // -1

/// Estrutura preenchida por SYS_SYSINFO (48 bytes).
#[repr(C)]
struct SysInfo {
    arch_id: u32,
    ncpus: u32,
    mem_total_kb: u32,
    mem_free_kb: u32,
    fb_w: u32,
    fb_h: u32,
    uptime_ms: u64,
    arch_name: [u8; 16],
}

/// Estrutura preenchida por SYS_PROC_INFO (48 bytes).
#[repr(C)]
struct ProcInfo {
    pid: u64,
    state: u32, // 1 rodando, 2 pronto, 3 dormindo, 4 bloqueado (IPC)
    cpu: u32,
    cpu_ms: u64,
    name: [u8; 24],
}

/// O intervalo [ptr, ptr+len) pertence ao processo corrente (e e
/// gravavel, se `write`)? Toda syscall que toca memoria de usuario passa
/// por aqui: um ponteiro invalido vira erro, nunca um fault no kernel.
fn user_ok(ptr: u64, len: u64, write: bool) -> bool {
    if len == 0 { return true; }
    let Some(end) = ptr.checked_add(len) else { return false };
    let root = process::current_root();
    let mut va = ptr & !0xFFF;
    while va < end {
        if arch::mmu::translate_user(root, va, write).is_none() {
            return false;
        }
        va += 4096;
    }
    true
}

pub fn dispatch(frame: *mut Context) {
    let ctx = unsafe { &mut *frame };
    let num = ctx.syscall_num();
    let a1 = ctx.arg(0);
    let a2 = ctx.arg(1);
    let a3 = ctx.arg(2);

    match num {
        SYS_WRITE => ctx.set_ret(sys_write(a1, a2) as u64),
        SYS_YIELD => unsafe { process::schedule(frame) },
        SYS_EXIT => unsafe { process::exit_current(frame) },
        SYS_FB_INFO => {
            let (w, h) = crate::fb::size();
            ctx.set_ret(((w as u64) << 32) | h as u64);
        }
        SYS_FB_MAP => ctx.set_ret(crate::fb::map_for_user()),
        SYS_FB_PRESENT => {
            // so a trava da GPU: as outras CPUs seguem livres
            crate::gpu::present_rect((a1 >> 32) as u32, a1 as u32, (a2 >> 32) as u32, a2 as u32);
            unsafe { (*frame).set_ret(0) };
        }
        SYS_INPUT_POLL => {
            ctx.set_ret(match crate::input::poll_for(process::current_pid()) {
                Some(ev) => (1u64 << 63)
                    | ((ev.ev_type as u64) << 48)
                    | ((ev.code as u64) << 32)
                    | ev.value as u64,
                None => 0,
            });
        }
        SYS_FS_COUNT => ctx.set_ret(crate::fs::count() as u64),
        SYS_FS_STAT => ctx.set_ret(sys_fs_stat(a1, a2) as u64),
        SYS_SPAWN => ctx.set_ret(sys_spawn(a1, a2) as u64),
        SYS_GETPID => ctx.set_ret(process::current_pid() as u64),
        SYS_IPC_SEND => sys_ipc_send(frame, a1, a2),
        SYS_IPC_RECV => {
            if a1 != 0 && !user_ok(a1, 56, true) { ctx.set_ret(ERR); return; }
            unsafe { process::ipc_recv(frame, a1, 0) }
        }
        SYS_IPC_RECV_TIMEOUT => {
            if a1 != 0 && !user_ok(a1, 56, true) { ctx.set_ret(ERR); return; }
            unsafe { process::ipc_recv(frame, a1, a2.clamp(1, 60_000)) }
        }
        SYS_IPC_REPLY => {
            if a2 != 0 && !user_ok(a2, 56, false) { ctx.set_ret(ERR); return; }
            let msg = if a2 != 0 { unsafe { read_msg(a2) } } else { Message::zero() };
            unsafe { process::ipc_reply(frame, a1 as usize, msg) };
        }
        SYS_IPC_TRY_RECV => {
            if a1 != 0 && !user_ok(a1, 56, true) { ctx.set_ret(ERR); return; }
            unsafe { process::ipc_try_recv(frame, a1) }
        }
        SYS_FS_READ => ctx.set_ret(sys_fs_read(a1, a2, a3) as u64),
        SYS_FS_WRITE => ctx.set_ret(sys_fs_write(a1, a2, a3) as u64),
        SYS_FS_CREATE => ctx.set_ret(sys_fs_create(a1, a2) as u64),
        SYS_SET_FOCUS => { crate::input::set_focus(a1 as usize); ctx.set_ret(0); }
        SYS_REGISTER => { register_service(a1, process::current_pid()); ctx.set_ret(0); }
        SYS_LOOKUP => ctx.set_ret(lookup_service(a1) as u64),
        SYS_SURF_MAP => {
            ctx.set_ret(if a1 < crate::fb::SURF_SLOTS { crate::fb::map_surface_for_user(a1) } else { 0 });
        }
        SYS_SURF_INFO => ctx.set_ret((crate::fb::SURF_W << 16) | crate::fb::SURF_H),
        SYS_IS_ALIVE => ctx.set_ret(process::is_alive(a1 as usize) as u64),
        SYS_UPTIME => ctx.set_ret(crate::time::now_ms() / 10),
        SYS_NET_STATUS => {
            crate::net::NET_LOCK.lock();
            crate::net::poll();
            let up = crate::net::is_up();
            crate::net::NET_LOCK.unlock();
            let ip = crate::net::OUR_IP;
            let v = if up {
                ((ip[0] as u64) << 24) | ((ip[1] as u64) << 16) | ((ip[2] as u64) << 8) | ip[3] as u64
            } else { 0 };
            unsafe { (*frame).set_ret(v) };
        }
        SYS_TCP_CONNECT => {
            let ip = [(a1 >> 24) as u8, (a1 >> 16) as u8, (a1 >> 8) as u8, a1 as u8];
            let _g = crate::net::NET_LOCK.guard();
            ctx.set_ret(match crate::net::tcp_connect(&ip, a2 as u16) {
                Some(h) => h as u64,
                None => ERR,
            });
        }
        SYS_TCP_SEND => {
            if a3 > 0 && !user_ok(a2, a3, false) { ctx.set_ret(ERR); return; }
            let data = unsafe { slice::from_raw_parts(a2 as *const u8, a3 as usize) };
            let _g = crate::net::NET_LOCK.guard();
            ctx.set_ret(crate::net::tcp_send(a1 as usize, data) as u64);
        }
        SYS_TCP_RECV => {
            if a3 == 0 || !user_ok(a2, a3, true) { ctx.set_ret(ERR); return; }
            let out = unsafe { slice::from_raw_parts_mut(a2 as *mut u8, a3 as usize) };
            let _g = crate::net::NET_LOCK.guard();
            ctx.set_ret(crate::net::tcp_recv(a1 as usize, out) as u64);
        }
        SYS_TCP_CLOSE => {
            let _g = crate::net::NET_LOCK.guard();
            crate::net::tcp_close(a1 as usize);
            ctx.set_ret(0);
        }
        SYS_TCP_STATUS => {
            let _g = crate::net::NET_LOCK.guard();
            ctx.set_ret(crate::net::tcp_status(a1 as usize) as u64);
        }
        SYS_UDP_RECV => {
            if a3 < 8 || !user_ok(a2, a3, true) { ctx.set_ret(ERR); return; }
            let out = unsafe { slice::from_raw_parts_mut(a2 as *mut u8, a3 as usize) };
            let _g = crate::net::NET_LOCK.guard();
            let (hdr, body) = out.split_at_mut(8);
            ctx.set_ret(match crate::net::udp_recv(a1 as u16, body) {
                Some((ip, port, n)) => {
                    hdr[..4].copy_from_slice(&ip);
                    hdr[4] = (port >> 8) as u8;
                    hdr[5] = port as u8;
                    hdr[6] = 0;
                    hdr[7] = 0;
                    (n + 8) as u64
                }
                None => (-2i64) as u64,
            });
        }
        SYS_NET_UDP_SEND => {
            let ip = [(a1 >> 24) as u8, (a1 >> 16) as u8, (a1 >> 8) as u8, a1 as u8];
            let dport = (a2 >> 16) as u16;
            let sport = a2 as u16;
            let ptr = a3 >> 32;
            let len = a3 & 0xFFFF_FFFF;
            if ptr == 0 || len > 1400 || !user_ok(ptr, len, false) { ctx.set_ret(0); return; }
            let mut data = [0u8; 1400];
            unsafe { core::ptr::copy_nonoverlapping(ptr as *const u8, data.as_mut_ptr(), len as usize) };
            crate::net::NET_LOCK.lock();
            let ok = crate::net::udp_send(&ip, dport, sport, &data[..len as usize]);
            crate::net::NET_LOCK.unlock();
            unsafe { (*frame).set_ret(ok as u64) };
        }
        SYS_SBRK => ctx.set_ret(process::sbrk(a1).unwrap_or(ERR)),
        SYS_GETRANDOM => ctx.set_ret(sys_getrandom(a1, a2) as u64),
        SYS_THREAD_CREATE => ctx.set_ret(process::thread_create(a1, a2, a3).map(|t| t as u64).unwrap_or(ERR)),
        SYS_FUTEX_WAIT => {
            if a1 & 3 != 0 || !user_ok(a1, 4, false) { ctx.set_ret(ERR); return; }
            unsafe { process::futex_wait(frame, a1, a2 as u32, a3.min(600_000)) }
        }
        SYS_FUTEX_WAKE => ctx.set_ret(process::futex_wake(a1, a2)),
        SYS_EXIT_GROUP => unsafe { process::exit_group(frame) },
        SYS_SLEEP => unsafe { process::sleep(frame, a1.min(60_000)) },
        SYS_TIME_MS => ctx.set_ret(crate::time::now_ms()),
        SYS_RTC => ctx.set_ret(arch::rtc_seconds()),
        SYS_SYSINFO => ctx.set_ret(sys_sysinfo(a1) as u64),
        SYS_PROC_INFO => ctx.set_ret(sys_proc_info(a1, a2) as u64),
        SYS_KILL => {
            let is_self = a1 as usize == process::current_pid();
            if is_self { ctx.set_ret(0); }
            let ok = unsafe { process::kill(frame, a1 as usize) };
            // matando a si mesmo, o frame ja pertence ao proximo processo
            if !is_self {
                unsafe { (*frame).set_ret(if ok { 0 } else { ERR }) };
            }
        }
        SYS_CPU_LOAD => ctx.set_ret(match process::cpu_load(a1 as usize) {
            Some(p) => p as u64,
            None => ERR,
        }),
        SYS_POWEROFF => {
            Uart.write_string("\n[kernel] desligando a pedido do usuario. Ate logo!\n");
            arch::poweroff();
        }
        SYS_CURSOR_SET => {
            let n = (crate::gpu::CURSOR_SIZE * crate::gpu::CURSOR_SIZE) as u64;
            if !user_ok(a1, n * 4, false) { ctx.set_ret(ERR); return; }
            let mut img = [0u32; (crate::gpu::CURSOR_SIZE * crate::gpu::CURSOR_SIZE) as usize];
            unsafe { core::ptr::copy_nonoverlapping(a1 as *const u32, img.as_mut_ptr(), n as usize) };
            let ok = crate::gpu::set_cursor(&img, (a2 >> 16) as u32 & 0xFFFF, a2 as u32 & 0xFFFF,
                (a3 >> 32) as u32, a3 as u32);
            unsafe { (*frame).set_ret(if ok { 0 } else { ERR }) };
        }
        SYS_CURSOR_MOVE => {
            crate::gpu::move_cursor(a1 as u32, a2 as u32);
            unsafe { (*frame).set_ret(0) };
        }
        SYS_LAST_CRASH => ctx.set_ret(match process::take_crash() {
            Some(p) => p as u64,
            None => ERR,
        }),
        _ => ctx.set_ret(ERR),
    }
}

fn sys_write(ptr: u64, len: u64) -> i64 {
    if ptr == 0 || len == 0 || len > 4096 || !user_ok(ptr, len, false) {
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

/// Copia a entrada `idx` da tabela de arquivos para o buffer do usuario.
/// Layout: name[32] + start_sector u32 + size u32 + kind u32 (44 bytes).
fn sys_fs_stat(idx: u64, ptr: u64) -> i64 {
    if ptr == 0 || idx as usize >= crate::fs::count() || !user_ok(ptr, 44, true) {
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
    if ptr == 0 || len == 0 || len > 32 || !user_ok(ptr, len, false) {
        return -1;
    }
    let mut name = [0u8; 32];
    unsafe { core::ptr::copy_nonoverlapping(ptr as *const u8, name.as_mut_ptr(), len as usize) };
    let Ok(name) = str::from_utf8(&name[..len as usize]) else { return -1 };
    match crate::loader::spawn_from_fs(name) {
        Some(pid) => pid as i64,
        None => -1,
    }
}

fn sys_sysinfo(ptr: u64) -> i64 {
    if !user_ok(ptr, core::mem::size_of::<SysInfo>() as u64, true) { return -1; }
    let (w, h) = crate::fb::size();
    let mut name = [0u8; 16];
    let n = arch::NAME.len().min(15);
    name[..n].copy_from_slice(&arch::NAME.as_bytes()[..n]);
    let info = SysInfo {
        arch_id: arch::ARCH_ID,
        ncpus: process::online_cpus() as u32,
        mem_total_kb: (crate::frame_allocator::total_count() * 4) as u32,
        mem_free_kb: (crate::frame_allocator::free_count() * 4) as u32,
        fb_w: w,
        fb_h: h,
        uptime_ms: crate::time::now_ms(),
        arch_name: name,
    };
    unsafe { core::ptr::write_unaligned(ptr as *mut SysInfo, info) };
    0
}

fn sys_proc_info(slot: u64, ptr: u64) -> i64 {
    if slot as usize >= process::MAX_PROCS { return -2; }
    if !user_ok(ptr, core::mem::size_of::<ProcInfo>() as u64, true) { return -2; }
    let Some((pid, state, cpu, cpu_ms, name)) = process::info(slot as usize) else { return -1 };
    let pi = ProcInfo { pid: pid as u64, state, cpu, cpu_ms, name };
    unsafe { core::ptr::write_unaligned(ptr as *mut ProcInfo, pi) };
    0
}

// ---------------------------------------------------------- name server

static mut SERVICES: [usize; 8] = [usize::MAX; 8];
static SERVICES_LOCK: SpinLock = SpinLock::new();

fn register_service(id: u64, pid: usize) {
    if (id as usize) < 8 {
        let _g = SERVICES_LOCK.guard();
        unsafe { SERVICES[id as usize] = pid; }
    }
}

fn lookup_service(id: u64) -> i64 {
    if (id as usize) < 8 {
        let pid = { let _g = SERVICES_LOCK.guard(); unsafe { SERVICES[id as usize] } };
        if pid != usize::MAX && process::is_alive(pid) {
            return pid as i64;
        }
    }
    -1
}

// ------------------------------------------------------------------ IPC

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

fn sys_ipc_send(frame: *mut Context, dst: u64, msg_ptr: u64) {
    if msg_ptr == 0 || !user_ok(msg_ptr, 56, true) {
        unsafe { (*frame).set_ret(ERR) };
        return;
    }
    let msg = unsafe { read_msg(msg_ptr) };
    // A resposta (e o retorno 0) e gravada no contexto salvo deste processo
    // quando o reply chegar. Destino invalido: falha imediata com -1.
    if !unsafe { process::ipc_send(frame, dst as usize, msg, msg_ptr) } {
        unsafe { (*frame).set_ret(ERR) };
    }
}

// ------------------------------------------------------------ Filesystem

fn sys_getrandom(ptr: u64, len: u64) -> i64 {
    if ptr == 0 && len == 0 {
        return crate::rng::is_strong() as i64;
    }
    if ptr == 0 || len > 4096 || !user_ok(ptr, len, true) {
        return -1;
    }
    let mut done = 0u64;
    let mut chunk = [0u8; 256];
    while done < len {
        let n = (len - done).min(256) as usize;
        crate::rng::fill(&mut chunk[..n]);
        unsafe { core::ptr::copy_nonoverlapping(chunk.as_ptr(), (ptr + done) as *mut u8, n) };
        done += n as u64;
    }
    len as i64
}

/// Buffer de passagem para leituras/escritas de arquivo (DMA do disco).
static mut STAGE: [u8; 65536] = [0; 65536];
static STAGE_LOCK: SpinLock = SpinLock::new();

fn sys_fs_read(idx: u64, dst: u64, max: u64) -> i64 {
    let idx = idx as usize;
    if dst == 0 || idx >= crate::fs::count() {
        return -1;
    }
    let size = crate::fs::entry(idx).size_bytes as u64;
    let want = size.min(max);
    if size as usize > 65536 - 512 {
        return -2;
    }
    if !user_ok(dst, want, true) {
        return -1;
    }
    let _g = STAGE_LOCK.guard();
    let stage_pa = (&raw mut STAGE) as u64;
    if crate::fs::read_file(idx, stage_pa).is_none() {
        return -3;
    }
    unsafe {
        core::ptr::copy_nonoverlapping(stage_pa as *const u8, dst as *mut u8, want as usize);
    }
    want as i64
}

fn sys_fs_write(idx: u64, src: u64, size: u64) -> i64 {
    if src == 0 || size > 65536 - 512 || !user_ok(src, size, false) {
        return -1;
    }
    let _g = STAGE_LOCK.guard();
    let stage_pa = (&raw mut STAGE) as u64;
    unsafe {
        core::ptr::copy_nonoverlapping(src as *const u8, stage_pa as *mut u8, size as usize);
        // zera o resto do ultimo setor para nao vazar lixo
        let secbytes = ((size as usize).div_ceil(512)) * 512;
        core::ptr::write_bytes((stage_pa as *mut u8).add(size as usize), 0, secbytes - size as usize);
    }
    if crate::fs::write_file(idx as usize, stage_pa, size as u32) { 0 } else { -2 }
}

fn sys_fs_create(name_ptr: u64, name_len: u64) -> i64 {
    if name_ptr == 0 || name_len == 0 || name_len > 27 || !user_ok(name_ptr, name_len, false) {
        return -1;
    }
    let mut buf = [0u8; 32];
    unsafe { core::ptr::copy_nonoverlapping(name_ptr as *const u8, buf.as_mut_ptr(), name_len as usize) };
    let Ok(name) = str::from_utf8(&buf[..name_len as usize]) else { return -1 };
    if crate::fs::find(name).is_some() {
        return -3;
    }
    // 16 setores (8KB) de capacidade para textos novos
    match crate::fs::create(name, 2, 16) {
        Some(idx) => idx as i64,
        None => -2,
    }
}
