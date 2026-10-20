//! plib — biblioteca de userspace do Pulsar OS (Rust).
//!
//! * syscalls (AArch64 `svc` / RISC-V `ecall`, mesma numeracao)
//! * IPC sincrono e protocolos dos servidores (fsd, WM)
//! * graficos (gfx), texto (text/fonts), icones (icons) e temas (theme)
//! * teclado com layouts US e ABNT2 (teclas mortas para acentos)
//! * `App`: esqueleto de cliente do WM (superficie, eventos, tema)
#![no_std]

pub mod keymaps;
pub mod fonts;
pub mod gfx;
pub mod icons;
pub mod text;
pub mod theme;

pub use fonts::*;
pub use gfx::{Fb, Rect};
pub use theme::Theme;

use core::arch::asm;

// ---------------------------------------------------------------- syscalls
// AArch64 `svc #0` (numero em x8); RISC-V `ecall` (numero em a7).
// Argumentos e retorno em x0../a0.. nas duas.

#[cfg(target_arch = "aarch64")]
#[inline]
pub fn syscall3(num: u64, a0: u64, a1: u64, a2: u64) -> u64 {
    let ret: u64;
    unsafe {
        asm!("svc #0", in("x8") num, inout("x0") a0 => ret, in("x1") a1, in("x2") a2,
             options(nostack));
    }
    ret
}

#[cfg(target_arch = "riscv64")]
#[inline]
pub fn syscall3(num: u64, a0: u64, a1: u64, a2: u64) -> u64 {
    let ret: u64;
    unsafe {
        asm!("ecall", in("a7") num, inlateout("a0") a0 => ret, in("a1") a1, in("a2") a2,
             options(nostack));
    }
    ret
}

#[cfg(target_arch = "x86_64")]
#[inline]
pub fn syscall3(num: u64, a0: u64, a1: u64, a2: u64) -> u64 {
    let ret: u64;
    unsafe {
        asm!("syscall", inlateout("rax") num => ret, in("rdi") a0, in("rsi") a1, in("rdx") a2,
             out("rcx") _, out("r11") _, options(nostack));
    }
    ret
}

#[cfg(target_arch = "aarch64")]
#[inline]
pub fn syscall4(num: u64, a0: u64, a1: u64, a2: u64, a3: u64) -> u64 {
    let ret: u64;
    unsafe {
        asm!("svc #0", in("x8") num, inout("x0") a0 => ret, in("x1") a1, in("x2") a2, in("x3") a3,
             options(nostack));
    }
    ret
}

#[cfg(target_arch = "riscv64")]
#[inline]
pub fn syscall4(num: u64, a0: u64, a1: u64, a2: u64, a3: u64) -> u64 {
    let ret: u64;
    unsafe {
        asm!("ecall", in("a7") num, inlateout("a0") a0 => ret, in("a1") a1, in("a2") a2, in("a3") a3,
             options(nostack));
    }
    ret
}

#[cfg(target_arch = "x86_64")]
#[inline]
pub fn syscall4(num: u64, a0: u64, a1: u64, a2: u64, a3: u64) -> u64 {
    let ret: u64;
    unsafe {
        asm!("syscall", inlateout("rax") num => ret, in("rdi") a0, in("rsi") a1, in("rdx") a2, in("r10") a3,
             out("rcx") _, out("r11") _, options(nostack));
    }
    ret
}


#[inline]
fn syscall2(num: u64, a0: u64, a1: u64) -> u64 { syscall3(num, a0, a1, 0) }
#[inline]
fn syscall1(num: u64, a0: u64) -> u64 { syscall3(num, a0, 0, 0) }
#[inline]
fn syscall0(num: u64) -> u64 { syscall3(num, 0, 0, 0) }

pub fn write(s: &str) { syscall2(1, s.as_ptr() as u64, s.len() as u64); }
pub fn write_bytes(s: &[u8]) { syscall2(1, s.as_ptr() as u64, s.len() as u64); }
pub fn yield_now() { syscall0(2); }
pub fn exit() -> ! {
    syscall0(3);
    loop {}
}
pub fn fb_info() -> (u32, u32) {
    let v = syscall0(4);
    ((v >> 32) as u32, v as u32)
}
pub fn fb_map() -> u64 { syscall0(5) }
/// Apresenta um retangulo do back buffer (damage rect).
pub fn fb_present(r: Rect) {
    if r.is_empty() || r.x < 0 || r.y < 0 { return; }
    syscall2(6, ((r.x as u64) << 32) | r.y as u64, ((r.w as u64) << 32) | r.h as u64);
}
/// Altera a resolucao da tela (virtio-gpu scanout 0). Retorna true se aplicou.
pub fn fb_set_mode(w: u32, h: u32) -> bool {
    syscall2(90, w as u64, h as u64) == 0
}
pub fn fs_count() -> usize { syscall0(8) as usize }
/// Preenche `buf` com a entrada `idx`: name[32] start u32 size u32 kind u32.
pub fn fs_stat(idx: usize, buf: &mut [u8; 44]) -> i64 {
    syscall2(9, idx as u64, buf.as_mut_ptr() as u64) as i64
}
pub fn spawn(name: &str) -> i64 {
    syscall2(10, name.as_ptr() as u64, name.len() as u64) as i64
}
pub fn getpid() -> usize { syscall0(14) as usize }

/// Ambiente dos programas abertos pelo sistema (o mesmo do Terminal): os
/// programas Linux acham por ele o servico Wayland, a pasta pessoal e a lingua.
pub const APP_ENV: &[&str] = &[
    "HOME=/home/user", "PATH=/bin:/usr/bin", "USER=usuario", "LANG=C.UTF-8",
    "XDG_RUNTIME_DIR=/run", "WAYLAND_DISPLAY=wayland-0", "XCOMPOSEFILE=/usr/share/pulsar/Compose",
    "GDK_BACKEND=wayland", "MOZ_ENABLE_WAYLAND=1", "NO_AT_BRIDGE=1", "GSETTINGS_BACKEND=memory",
    "DBUS_SESSION_BUS_ADDRESS=unix:path=/run/dbus-session",
    "AT_SPI_BUS_ADDRESS=unix:path=/run/null", "GTK_A11Y=none", "GTK_MODULES=",
    "MOZ_GFX_SPOOF_GL_RENDERER=llvmpipe", "MOZ_GFX_SPOOF_GL_VENDOR=Mesa", "MOZ_GFX_SPOOF_GL_VERSION=3.0",
    "MOZ_FORCE_DISABLE_E10S=1",
    "MOZ_CRASHREPORTER_DISABLE=1", "LIBGL_ALWAYS_SOFTWARE=1",
];

/// Executa o programa do caminho absoluto `path` (ELF Linux ou .pulse), sem
/// argumentos e com APP_ENV (SPAWN_EX). Ret pid ou <0.
pub fn spawn_path(path: &str) -> i64 {
    let mut b = [0u8; 2048];
    let mut n = 8;
    let mut put = |s: &str| {
        if n + s.len() + 1 > b.len() { return false; }
        b[n..n + s.len()].copy_from_slice(s.as_bytes());
        n += s.len() + 1;
        true
    };
    put(path);
    let envc = APP_ENV.iter().take_while(|e| put(e)).count();
    b[0..4].copy_from_slice(&1u32.to_le_bytes());
    b[4..8].copy_from_slice(&(envc as u32).to_le_bytes());
    syscall4(74, path.as_ptr() as u64, path.len() as u64, b.as_ptr() as u64, n as u64) as i64
}

/// Le o arquivo do caminho absoluto `path` para `buf` (ate encher). Ret
/// bytes lidos ou <0 (-errno).
pub fn read_path(path: &str, buf: &mut [u8]) -> i64 {
    let fd = syscall3(56, path.as_ptr() as u64, path.len() as u64, 0) as i64;   // OPEN, O_RDONLY
    if fd < 0 { return fd; }
    let mut n = 0;
    while n < buf.len() {
        let r = syscall3(58, fd as u64, buf[n..].as_mut_ptr() as u64, (buf.len() - n) as u64) as i64;
        if r <= 0 { break; }
        n += r as usize;
    }
    syscall1(57, fd as u64);                                                   // CLOSE
    n as i64
}
pub fn fs_read(idx: usize, buf: &mut [u8]) -> i64 {
    syscall3(15, idx as u64, buf.as_mut_ptr() as u64, buf.len() as u64) as i64
}
pub fn fs_write(idx: usize, data: &[u8]) -> i64 {
    syscall3(16, idx as u64, data.as_ptr() as u64, data.len() as u64) as i64
}
/// Cria um arquivo de texto vazio. Retorna o indice ou <0 (-3 = ja existe).
pub fn fs_create(name: &[u8]) -> i64 {
    syscall2(17, name.as_ptr() as u64, name.len() as u64) as i64
}
pub fn set_focus(pid: usize) { syscall1(18, pid as u64); }
pub fn register_service(service_id: u64) { syscall1(19, service_id); }
pub fn lookup_service(service_id: u64) -> i64 { syscall1(20, service_id) as i64 }
pub fn surface_map(slot: u64) -> u64 { syscall1(21, slot) }
pub fn is_alive(pid: usize) -> bool { syscall1(24, pid as u64) != 0 }
pub fn net_status() -> u32 { syscall0(26) as u32 }
/// Dorme sem gastar CPU.
pub fn sleep_ms(ms: u64) { syscall1(29, ms); }
/// Milissegundos desde o boot (contador de hardware).
pub fn uptime_ms() -> u64 { syscall0(30) }
/// Segundos desde 1970 (relogio de tempo real).
pub fn rtc_seconds() -> u64 { syscall0(31) }
pub fn kill(pid: usize) -> bool { syscall1(34, pid as u64) == 0 }
/// Uso (%) da CPU `cpu` no ultimo segundo; None se offline.
pub fn cpu_load(cpu: usize) -> Option<u32> {
    let v = syscall1(35, cpu as u64) as i64;
    if v < 0 { None } else { Some(v as u32) }
}
pub fn poweroff() -> ! {
    syscall0(36);
    loop {}
}
/// Define a imagem (64x64 ARGB) e o hotspot do cursor de hardware.
pub fn cursor_set(img: &[u32; 64 * 64], hot_x: u32, hot_y: u32, x: i32, y: i32) {
    syscall3(37, img.as_ptr() as u64, ((hot_x as u64) << 16) | hot_y as u64,
        ((x.max(0) as u64) << 32) | y.max(0) as u64);
}
pub fn cursor_move(x: i32, y: i32) { syscall2(38, x.max(0) as u64, y.max(0) as u64); }
/// Pid do ultimo processo encerrado por fault (o WM avisa o usuario).
pub fn last_crash() -> Option<usize> {
    let v = syscall0(39) as i64;
    if v < 0 { None } else { Some(v as usize) }
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct SysInfo {
    pub arch_id: u32,
    pub ncpus: u32,
    pub mem_total_kb: u32,
    pub mem_free_kb: u32,
    pub fb_w: u32,
    pub fb_h: u32,
    pub uptime_ms: u64,
    pub arch_name: [u8; 16],
}

impl SysInfo {
    pub fn arch(&self) -> &[u8] {
        let n = self.arch_name.iter().position(|&b| b == 0).unwrap_or(16);
        &self.arch_name[..n]
    }
}

pub fn sysinfo() -> SysInfo {
    let mut s = SysInfo { arch_id: 0, ncpus: 0, mem_total_kb: 0, mem_free_kb: 0, fb_w: 0, fb_h: 0,
        uptime_ms: 0, arch_name: [0; 16] };
    syscall1(32, &mut s as *mut SysInfo as u64);
    s
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct ProcInfo {
    pub pid: u64,
    /// 1 rodando, 2 pronto, 3 dormindo, 4 bloqueado (IPC)
    pub state: u32,
    pub cpu: u32,
    pub cpu_ms: u64,
    pub name: [u8; 24],
}

impl ProcInfo {
    pub const fn zero() -> ProcInfo {
        ProcInfo { pid: 0, state: 0, cpu: 0, cpu_ms: 0, name: [0; 24] }
    }
    pub fn name(&self) -> &[u8] {
        let n = self.name.iter().position(|&b| b == 0).unwrap_or(24);
        &self.name[..n]
    }
}

/// Entrada `slot` da tabela de processos: Ok(Some) ocupada, Ok(None)
/// vazia, Err(()) fim da tabela.
pub fn proc_info(slot: usize) -> Result<Option<ProcInfo>, ()> {
    let mut p = ProcInfo::zero();
    match syscall2(33, slot as u64, &mut p as *mut ProcInfo as u64) as i64 {
        0 => Ok(Some(p)),
        -1 => Ok(None),
        _ => Err(()),
    }
}

// ---------------------------------------------------------------- IPC

/// Mensagem de IPC: tag + 6 palavras (56 bytes).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Msg {
    pub tag: u64,
    pub data: [u64; 6],
}
impl Msg {
    pub const fn new(tag: u64) -> Self { Msg { tag, data: [0; 6] } }
}

/// Envia e bloqueia ate a resposta (que sobrescreve `m`). 0 ok, <0 erro.
pub fn send(dst: usize, m: &mut Msg) -> i64 {
    syscall2(11, dst as u64, m as *mut Msg as u64) as i64
}
/// Bloqueia ate receber. Retorna o pid do remetente.
pub fn recv(m: &mut Msg) -> usize { syscall1(12, m as *mut Msg as u64) as usize }
pub fn reply(to: usize, m: &Msg) { syscall2(13, to as u64, m as *const Msg as u64); }
/// RECV nao-bloqueante.
pub fn try_recv(m: &mut Msg) -> Option<usize> {
    let r = syscall1(23, m as *mut Msg as u64) as i64;
    if r < 0 { None } else { Some(r as usize) }
}

/// RECV com prazo: Some(pid) se chegou mensagem, None apos `ms` sem nada.
pub fn recv_timeout(m: &mut Msg, ms: u64) -> Option<usize> {
    let r = syscall2(40, m as *mut Msg as u64, ms.max(1)) as i64;
    if r < 0 { None } else { Some(r as usize) }
}

/// Espera (sem gastar CPU) ate o servico existir.
pub fn wait_service(id: u64) -> usize {
    loop {
        let p = lookup_service(id);
        if p >= 0 { return p as usize; }
        sleep_ms(5);
    }
}

pub const SVC_FS: u64 = 1;
pub const SVC_WM: u64 = 2;

// Protocolo do fsd (tag da mensagem)
pub const FS_OP_COUNT: u64 = 1; // -> data[0] = numero de arquivos
pub const FS_OP_STAT: u64 = 2;  // data[0]=idx -> data: size, kind, nome(4 palavras)
pub const FS_OP_READ: u64 = 3;  // data[0]=idx, data[1]=offset -> ate 40 bytes

// ------------------------------------------------------------- arquivos

/// Metadados de um arquivo.
#[derive(Clone, Copy)]
pub struct FileInfo {
    pub idx: usize,
    pub name: [u8; 28],
    pub nlen: usize,
    pub size: u32,
    pub kind: u32,
}

impl FileInfo {
    pub const fn zero() -> FileInfo {
        FileInfo { idx: 0, name: [0; 28], nlen: 0, size: 0, kind: 0 }
    }
    pub fn name(&self) -> &[u8] { &self.name[..self.nlen] }
    pub fn ext(&self) -> &[u8] {
        let n = self.name();
        match n.iter().rposition(|&b| b == b'.') { Some(p) => &n[p + 1..], None => b"" }
    }
}

pub fn file_info(idx: usize) -> Option<FileInfo> {
    let mut st = [0u8; 44];
    if fs_stat(idx, &mut st) < 0 { return None; }
    let mut name = [0u8; 28];
    name.copy_from_slice(&st[..28]);
    let nlen = name.iter().position(|&b| b == 0).unwrap_or(28);
    Some(FileInfo {
        idx, name, nlen,
        size: u32::from_le_bytes([st[36], st[37], st[38], st[39]]),
        kind: u32::from_le_bytes([st[40], st[41], st[42], st[43]]),
    })
}

pub fn find_file(name: &[u8]) -> Option<usize> {
    (0..fs_count()).find(|&i| file_info(i).map(|f| f.name() == name).unwrap_or(false))
}

// ------------------------------------------------------------------ input

pub const EV_KEY: u16 = 1;
pub const EV_REL: u16 = 2;
pub const EV_ABS: u16 = 3;
pub const ABS_X: u16 = 0;
pub const ABS_Y: u16 = 1;
pub const REL_WHEEL: u16 = 8;
pub const BTN_LEFT: u16 = 0x110;
pub const BTN_RIGHT: u16 = 0x111;
pub const BTN_MIDDLE: u16 = 0x112;

/// Codigos evdev usados pelo sistema.
pub mod key {
    pub const ESC: u16 = 1;
    pub const BACKSPACE: u16 = 14;
    pub const TAB: u16 = 15;
    pub const Q: u16 = 16;
    pub const W: u16 = 17;
    pub const E: u16 = 18;
    pub const T: u16 = 20;
    pub const I: u16 = 23;
    pub const ENTER: u16 = 28;
    pub const LCTRL: u16 = 29;
    pub const A: u16 = 30;
    pub const S: u16 = 31;
    pub const D: u16 = 32;
    pub const LSHIFT: u16 = 42;
    pub const Z: u16 = 44;
    pub const C: u16 = 46;
    pub const V: u16 = 47;
    pub const N: u16 = 49;
    pub const RSHIFT: u16 = 54;
    pub const LALT: u16 = 56;
    pub const SPACE: u16 = 57;
    pub const CAPS: u16 = 58;
    pub const F1: u16 = 59;
    pub const F2: u16 = 60;
    pub const F4: u16 = 62;
    pub const F5: u16 = 63;
    pub const F11: u16 = 87;
    pub const F12: u16 = 88;
    pub const KPENTER: u16 = 96;
    pub const RCTRL: u16 = 97;
    pub const RALT: u16 = 100;
    pub const HOME: u16 = 102;
    pub const UP: u16 = 103;
    pub const PGUP: u16 = 104;
    pub const LEFT: u16 = 105;
    pub const RIGHT: u16 = 106;
    pub const END: u16 = 107;
    pub const DOWN: u16 = 108;
    pub const PGDN: u16 = 109;
    pub const DELETE: u16 = 111;
    pub const LMETA: u16 = 125;
    pub const RMETA: u16 = 126;
}

pub const MOD_SHIFT: u8 = 1;
pub const MOD_CTRL: u8 = 2;
pub const MOD_ALT: u8 = 4;
pub const MOD_META: u8 = 8;
/// AltGr (Alt da direita nos layouts que tem 3o/4o nivel) e Caps Lock ligado.
pub const MOD_ALTGR: u8 = 16;
pub const MOD_CAPS: u8 = 32;

#[derive(Clone, Copy, Default)]
pub struct Event {
    pub ev_type: u16,
    pub code: u16,
    pub value: u32,
}

pub fn input_poll() -> Option<Event> {
    let v = syscall0(7);
    if v >> 63 == 0 { return None; }
    Some(Event { ev_type: ((v >> 48) & 0x7FFF) as u16, code: (v >> 32) as u16, value: v as u32 })
}

/// Tecla traduzida: codigo evdev + caractere (Unicode/Latin-1; 0 se nenhum).
#[derive(Clone, Copy, Default, PartialEq)]
pub struct Key {
    pub code: u16,
    pub ch: u32,
    pub mods: u8,
}

impl Key {
    pub fn ctrl(&self) -> bool { self.mods & MOD_CTRL != 0 }
    pub fn shift(&self) -> bool { self.mods & MOD_SHIFT != 0 }
    pub fn alt(&self) -> bool { self.mods & MOD_ALT != 0 }
    pub fn meta(&self) -> bool { self.mods & MOD_META != 0 }
    pub fn pack(&self) -> u64 {
        self.code as u64 | ((self.mods as u64) << 16) | ((self.ch as u64) << 32)
    }
    pub fn unpack(v: u64) -> Option<Key> {
        if v & 0xFFFF == 0 { return None; }
        Some(Key { code: v as u16, mods: (v >> 16) as u8, ch: (v >> 32) as u32 })
    }
}

/// Layouts de teclado (tabelas geradas por tools/genkeymaps.py, as mesmas
/// que os programas Wayland recebem como mapa XKB).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Layout { Abnt, Abnt2, Us, UsIntl }

impl Layout {
    /// Nome usado no pulsar.cfg (keyboard=...).
    pub fn name(self) -> &'static str { self.def().name }
    pub fn from_name(n: &[u8]) -> Option<Layout> {
        Some(match n { b"abnt" => Layout::Abnt, b"abnt2" => Layout::Abnt2, b"us" => Layout::Us,
                       b"us-intl" => Layout::UsIntl, _ => return None })
    }
    fn def(self) -> &'static keymaps::LayoutDef {
        match self { Layout::Abnt => &keymaps::ABNT, Layout::Abnt2 => &keymaps::ABNT2,
                     Layout::Us => &keymaps::US, Layout::UsIntl => &keymaps::US_INTL }
    }
}

/// Estado do teclado: modificadores, caps lock e tecla morta pendente.
pub struct Keyboard {
    pub mods: u8,
    caps: bool,
    dead: u32,
    pub layout: Layout,
}

const DEAD_BIT: u32 = 0x8000_0000;

impl Keyboard {
    pub const fn new() -> Keyboard {
        Keyboard { mods: 0, caps: false, dead: 0, layout: Layout::Abnt2 }
    }

    /// Alimenta um evento EV_KEY (value 1 = press, 0 = release, 2 = repeat).
    /// Retorna a tecla traduzida nos pressionamentos.
    pub fn feed(&mut self, code: u16, value: u32) -> Option<Key> {
        let down = value != 0;
        let altgr = self.layout.def().altgr;
        let bit = match code {
            key::LSHIFT | key::RSHIFT => MOD_SHIFT,
            key::LCTRL | key::RCTRL => MOD_CTRL,
            key::LALT => MOD_ALT,
            key::RALT => if altgr { MOD_ALTGR } else { MOD_ALT },
            key::LMETA | key::RMETA => MOD_META,
            _ => 0,
        };
        if bit != 0 {
            if down { self.mods |= bit } else { self.mods &= !bit }
            return if down { Some(Key { code, ch: 0, mods: self.mods_now() }) } else { None };
        }
        if !down { return None; }
        if code == key::CAPS {
            self.caps = !self.caps;
            return None;
        }
        // atalhos (Ctrl/Alt/Super): so o codigo
        if self.mods & (MOD_CTRL | MOD_META | MOD_ALT) != 0 {
            return Some(Key { code, ch: 0, mods: self.mods_now() });
        }
        let v = self.translate(code);
        if v & DEAD_BIT != 0 {
            let d = v & !DEAD_BIT;
            if self.dead != 0 {
                // segunda tecla morta: a mesma escreve o acento; outra troca
                let prev = self.dead;
                self.dead = if prev == d { 0 } else { d };
                return Some(Key { code, ch: prev, mods: self.mods_now() });
            }
            self.dead = d;
            // a tecla vai para o app sem caractere: apps nativos a ignoram;
            // os programas Wayland (XKB) fazem a propria composicao com ela
            return Some(Key { code, ch: 0, mods: self.mods_now() });
        }
        let mut ch = v;
        if self.dead != 0 && ch != 0 {
            let d = self.dead;
            self.dead = 0;
            if ch == ' ' as u32 { ch = d; } else if let Some(c) = compose(d, ch) { ch = c; }
        }
        Some(Key { code, ch, mods: self.mods_now() })
    }

    fn mods_now(&self) -> u8 { self.mods | if self.caps { MOD_CAPS } else { 0 } }

    fn translate(&self, code: u16) -> u32 {
        let def = self.layout.def();
        if (code as usize) < 128 {
            let lv = def.keys[code as usize];
            if lv != [0; 4] {
                let shift = self.mods & MOD_SHIFT != 0;
                let alpha = keymaps::ALPHA.contains(&(code as u8));
                let up = if alpha { shift ^ self.caps } else { shift };
                let ag = self.mods & MOD_ALTGR != 0;
                return match (ag, up) {
                    (false, false) => lv[0],
                    (false, true) => lv[1],
                    (true, false) => lv[2],
                    (true, true) => if lv[3] != 0 { lv[3] } else { lv[2] },
                };
            }
        }
        match code {
            57 => ' ' as u32, 55 => '*' as u32,
            71 => '7' as u32, 72 => '8' as u32, 73 => '9' as u32, 74 => '-' as u32,
            75 => '4' as u32, 76 => '5' as u32, 77 => '6' as u32, 78 => '+' as u32,
            79 => '1' as u32, 80 => '2' as u32, 81 => '3' as u32, 82 => '0' as u32,
            83 => if matches!(self.layout, Layout::Abnt | Layout::Abnt2) { ',' as u32 } else { '.' as u32 },
            98 => '/' as u32,
            _ => 0,
        }
    }
}

/// Combina tecla morta + caractere (tabela gerada: á à â ã ä ç ñ ...).
fn compose(dead: u32, c: u32) -> Option<u32> {
    keymaps::COMPOSE.iter().find(|&&(d, b, _)| d == dead && b == c).map(|&(_, _, r)| r)
}

/// Codifica um codepoint em UTF-8. Retorna o numero de bytes.
pub fn utf8_encode(cp: u32, out: &mut [u8; 4]) -> usize {
    if cp < 0x80 {
        out[0] = cp as u8;
        1
    } else if cp < 0x800 {
        out[0] = 0xC0 | (cp >> 6) as u8;
        out[1] = 0x80 | (cp & 0x3F) as u8;
        2
    } else {
        out[0] = 0xE0 | (cp >> 12) as u8;
        out[1] = 0x80 | ((cp >> 6) & 0x3F) as u8;
        out[2] = 0x80 | (cp & 0x3F) as u8;
        3
    }
}

// ------------------------------------------------------- regiao grafica
// Mesmo VA nas duas arquiteturas (ver kernel/src/fb.rs).

pub const FB_UVA: u64 = 0x4600_0000;
pub const WALL_ADDR: u64 = FB_UVA + 16 * 1024 * 1024;
pub const WALLBLUR_ADDR: u64 = FB_UVA + 32 * 1024 * 1024;
pub const SURF_ADDR: u64 = FB_UVA + 48 * 1024 * 1024;
pub const SURF_SLOT_BYTES: u64 = 16 * 1024 * 1024;
pub const SURF_SLOTS: usize = 8;
/// Stride e altura maxima das superficies dos apps (ate 2560x1440).
pub const SURF_W: i32 = 2560;
pub const SURF_H: i32 = 1440;

/// Abre a tela (back buffer) — usado pelo compositor.
pub fn screen_open() -> Fb {
    let base = fb_map() as *mut u32;
    let (w, h) = fb_info();
    Fb::new(base, w as i32, h as i32, w as i32)
}

pub fn surface_ptr(slot: u64) -> *mut u32 {
    (SURF_ADDR + slot * SURF_SLOT_BYTES) as *mut u32
}

// ------------------------------------------------------------ protocolo WM
//
// HELLO  app->wm: data[0]=tipo, data[1]=w, data[2]=h, data[3..6]=titulo (24 B)
//        resp:    data[0]=slot, data[1]=w, data[2]=h, data[3]=arquivo+1 (0 = nenhum)
// COMMIT app->wm: data[0]=slot ("desenhei, componha"). Resposta = POLL.
// POLL   resp:    data[0]=flags | w<<16 | h<<32
//                 data[1]=mouse x local, data[2]=mouse y local (i32)
//                 data[3]=botoes (bit0 down, bit1 click, bit2 clique direito,
//                          bit3 duplo) | roda (i8) << 8
//                 data[4]=tecla (Key::pack), data[5]=menu | cor de destaque << 32
// TITLE  app->wm: data[0..3] = titulo (24 B)

pub const WM_OP_HELLO: u64 = 1;
pub const WM_OP_COMMIT: u64 = 2;
pub const WM_OP_POLL: u64 = 3;
pub const WM_OP_TITLE: u64 = 4;
/// `kind` do HELLO das janelas de programas Linux (o wlclient as abre).
pub const KIND_LINUX: usize = 99;
/// O app fecha a propria janela (vai terminar): sem aviso de encerramento.
pub const WM_OP_BYE: u64 = 5;
/// Altera a resolucao da tela pelo WM: data[0]=w, data[1]=h.
pub const WM_OP_SET_RES: u64 = 6;

/// data[2] opcional do COMMIT: retangulo alterado da superficie. Bit 63 =
/// presente; x nos bits 48..62 (15 bits), y em 32..47, w em 16..31 e h em
/// 0..15. Ausente = tudo mudou.
pub fn pack_damage(r: Rect) -> u64 {
    1 << 63 | (r.x as u64 & 0x7FFF) << 48 | (r.y as u64 & 0xFFFF) << 32
        | (r.w as u64 & 0xFFFF) << 16 | (r.h as u64 & 0xFFFF)
}

pub fn commit_damage(v: u64) -> Option<Rect> {
    if v >> 63 == 0 { return None; }
    let r = Rect::new((v >> 48 & 0x7FFF) as i32, (v >> 32 & 0xFFFF) as i32,
        (v >> 16 & 0xFFFF) as i32, (v & 0xFFFF) as i32);
    if r.is_empty() { None } else { Some(r) }
}

pub const WM_FLAG_CLOSE: u64 = 1;
pub const WM_FLAG_FOCUS: u64 = 2;
pub const WM_FLAG_DARK: u64 = 4;
pub const WM_FLAG_MORE: u64 = 8;
pub const WM_FLAG_VISIBLE: u64 = 16;

pub const WM_MOUSE_DOWN: u64 = 1;
pub const WM_MOUSE_CLICK: u64 = 2;
pub const WM_MOUSE_RCLICK: u64 = 4;
pub const WM_MOUSE_DOUBLE: u64 = 8;

// Acoes de menu enviadas pelo WM ao app em foco
pub const APP_MENU_SAVE: u16 = 1;
pub const APP_MENU_NEW: u16 = 2;
pub const APP_MENU_COPY: u16 = 3;
pub const APP_MENU_PASTE: u16 = 4;
pub const APP_MENU_UNDO: u16 = 5;
pub const APP_MENU_CLEAR: u16 = 6;
pub const APP_MENU_PAUSE: u16 = 7;
pub const APP_MENU_RESET: u16 = 8;
pub const APP_MENU_ZOOM_IN: u16 = 9;
pub const APP_MENU_ZOOM_OUT: u16 = 10;
pub const APP_MENU_SELECT_ALL: u16 = 11;
pub const APP_MENU_RELOAD: u16 = 12;
pub const APP_MENU_ADD: u16 = 13;

pub fn pack_title(t: &[u8], out: &mut [u64]) {
    // corta num limite de caractere UTF-8 (nao no meio de um "—")
    let mut n = t.len().min(out.len() * 8);
    while n < t.len() && n > 0 && t[n] & 0xC0 == 0x80 { n -= 1; }
    for (i, w) in out.iter_mut().enumerate() {
        let mut v = 0u64;
        for b in 0..8 {
            let k = i * 8 + b;
            if k < n { v |= (t[k] as u64) << (b * 8); }
        }
        *w = v;
    }
}

pub fn unpack_title(words: &[u64], out: &mut [u8; 24]) -> usize {
    for (i, w) in words.iter().enumerate().take(3) {
        for b in 0..8 { out[i * 8 + b] = (w >> (b * 8)) as u8; }
    }
    out.iter().position(|&b| b == 0).unwrap_or(24)
}

/// Evento recebido do WM.
#[derive(Clone, Copy, Default)]
pub struct WmEvent {
    pub close: bool,
    pub focused: bool,
    pub dark: bool,
    pub visible: bool,
    /// ha mais eventos na fila: chame poll de novo antes de desenhar
    pub more: bool,
    /// tamanho ou tema mudaram: redesenhe tudo
    pub changed: bool,
    pub w: i32,
    pub h: i32,
    pub mx: i32,
    pub my: i32,
    pub down: bool,
    pub click: bool,
    pub rclick: bool,
    pub double: bool,
    pub wheel: i32,
    pub key: Option<Key>,
    pub menu: u16,
    pub accent: u32,
}

impl WmEvent {
    fn decode(m: &Msg) -> WmEvent {
        let f = m.data[0];
        let b = m.data[3];
        WmEvent {
            close: f & WM_FLAG_CLOSE != 0,
            focused: f & WM_FLAG_FOCUS != 0,
            dark: f & WM_FLAG_DARK != 0,
            visible: f & WM_FLAG_VISIBLE != 0,
            more: f & WM_FLAG_MORE != 0,
            changed: false,
            w: ((f >> 16) & 0xFFFF) as i32,
            h: ((f >> 32) & 0xFFFF) as i32,
            mx: m.data[1] as i64 as i32,
            my: m.data[2] as i64 as i32,
            down: b & WM_MOUSE_DOWN != 0,
            click: b & WM_MOUSE_CLICK != 0,
            rclick: b & WM_MOUSE_RCLICK != 0,
            double: b & WM_MOUSE_DOUBLE != 0,
            wheel: ((b >> 8) & 0xFF) as u8 as i8 as i32,
            key: Key::unpack(m.data[4]),
            menu: m.data[5] as u16,
            accent: (m.data[5] >> 32) as u32 & 0xFFFFFF,
        }
    }

    /// O evento traz algo que pede redesenho?
    pub fn interesting(&self) -> bool {
        self.changed || self.click || self.rclick || self.wheel != 0 || self.key.is_some() || self.menu != 0
    }
}

/// Cliente do WM: superficie propria + eventos + tema.
pub struct App {
    pub wm: usize,
    pub slot: u64,
    pub fb: Fb,
    pub w: i32,
    pub h: i32,
    pub theme: Theme,
    pub focused: bool,
    /// indice do arquivo a abrir (duplo clique no Finder), se houver
    pub open_file: Option<usize>,
    kind: u64,
    title: [u8; 24],
    redraw: bool,
}

impl App {
    /// Registra-se no WM com tamanho preferido (area cliente) e titulo.
    pub fn new(kind: u64, title: &[u8], w: i32, h: i32) -> App {
        let wm = wait_service(SVC_WM);
        let mut m = Msg::new(WM_OP_HELLO);
        m.data[0] = kind;
        m.data[1] = w as u64;
        m.data[2] = h as u64;
        pack_title(title, &mut m.data[3..6]);
        send(wm, &mut m);
        let slot = m.data[0];
        let (w, h) = ((m.data[1] as i32).min(SURF_W), (m.data[2] as i32).min(SURF_H));
        let open_file = if m.data[3] > 0 { Some(m.data[3] as usize - 1) } else { None };
        let base = surface_map(slot) as *mut u32;
        let fb = Fb::new(base, w, h, SURF_W);
        let mut t = [0u8; 24];
        let n = title.len().min(24);
        t[..n].copy_from_slice(&title[..n]);
        let mut app = App { wm, slot, fb, w, h, theme: Theme::new(false, 0x0A84FF), focused: true, open_file,
                            kind, title: t, redraw: false };
        app.poll();
        app
    }

    /// Fala com o WM. Se ele caiu (o init o reinicia), espera o novo,
    /// registra a janela de novo e repete o pedido: o app sobrevive.
    fn call(&mut self, m: &mut Msg) {
        let orig = *m;
        while send(self.wm, m) < 0 {
            let old = self.wm;
            loop {
                let p = lookup_service(SVC_WM);
                if p >= 0 && p as usize != old && is_alive(p as usize) { self.wm = p as usize; break; }
                sleep_ms(50);
            }
            let mut h = Msg::new(WM_OP_HELLO);
            h.data[0] = self.kind;
            h.data[1] = self.w as u64;
            h.data[2] = self.h as u64;
            pack_title(&self.title, &mut h.data[3..6]);
            if send(self.wm, &mut h) < 0 { continue; }
            self.slot = h.data[0];
            self.w = (h.data[1] as i32).min(SURF_W);
            self.h = (h.data[2] as i32).min(SURF_H);
            self.fb = Fb::new(surface_map(self.slot) as *mut u32, self.w, self.h, SURF_W);
            self.redraw = true;
            *m = orig;
            if m.tag == WM_OP_COMMIT {
                m.data[0] = self.slot;
                m.data[1] = ((self.w as u64) << 16) | self.h as u64;
            }
        }
    }

    /// Aplica tamanho/tema do evento; marca `changed` se algo mudou.
    fn apply(&mut self, ev: &mut WmEvent) {
        if self.redraw { self.redraw = false; ev.changed = true; }
        if ev.w > 0 && ev.h > 0 && (ev.w != self.w || ev.h != self.h) {
            self.w = ev.w.min(SURF_W);
            self.h = ev.h.min(SURF_H);
            self.fb = Fb::new(self.fb.base, self.w, self.h, SURF_W);
            ev.changed = true;
        }
        if ev.accent != 0 && (ev.dark != self.theme.dark || ev.accent != self.theme.accent) {
            self.theme = Theme::new(ev.dark, ev.accent);
            ev.changed = true;
        }
        if ev.focused != self.focused {
            self.focused = ev.focused;
            ev.changed = true;
        }
    }

    /// Busca o proximo evento (bloqueia ate o WM atender, ~1 quadro).
    pub fn poll(&mut self) -> WmEvent {
        let mut m = Msg::new(WM_OP_POLL);
        self.call(&mut m);
        let mut ev = WmEvent::decode(&m);
        self.apply(&mut ev);
        ev
    }

    /// "Desenhei": o WM compoe a superficie. Retorna eventos como o poll.
    pub fn commit(&mut self) -> WmEvent {
        self.commit_rect(None)
    }

    /// COMMIT informando que so `dmg` (coordenadas da superficie) mudou.
    pub fn commit_rect(&mut self, dmg: Option<Rect>) -> WmEvent {
        let mut m = Msg::new(WM_OP_COMMIT);
        m.data[0] = self.slot;
        m.data[1] = ((self.w as u64) << 16) | self.h as u64;
        m.data[2] = dmg.map(pack_damage).unwrap_or(0);
        self.call(&mut m);
        let mut ev = WmEvent::decode(&m);
        self.apply(&mut ev);
        ev
    }

    pub fn set_title(&mut self, t: &[u8]) {
        let n = t.len().min(24);
        self.title = [0; 24];
        self.title[..n].copy_from_slice(&t[..n]);
        let mut m = Msg::new(WM_OP_TITLE);
        pack_title(t, &mut m.data[0..3]);
        self.call(&mut m);
    }
}

// ------------------------------------------------------------- formatacao

/// Buffer de texto de tamanho fixo (sem alocacao) para montar strings.
#[derive(Clone, Copy)]
pub struct Buf<const N: usize> {
    pub b: [u8; N],
    pub n: usize,
}

impl<const N: usize> Buf<N> {
    pub const fn new() -> Self { Buf { b: [0; N], n: 0 } }
    pub fn clear(&mut self) -> &mut Self { self.n = 0; self }
    pub fn s(&mut self, s: &str) -> &mut Self { self.bytes(s.as_bytes()) }
    pub fn bytes(&mut self, s: &[u8]) -> &mut Self {
        for &c in s {
            if self.n < N { self.b[self.n] = c; self.n += 1; }
        }
        self
    }
    pub fn u(&mut self, v: u64) -> &mut Self {
        let mut t = [0u8; 20];
        let mut i = 20;
        let mut v = v;
        if v == 0 { i -= 1; t[i] = b'0'; }
        while v > 0 { i -= 1; t[i] = b'0' + (v % 10) as u8; v /= 10; }
        self.bytes(&t[i..])
    }
    /// Inteiro com zeros a esquerda ate `w` digitos.
    pub fn u0(&mut self, v: u64, w: usize) -> &mut Self {
        let mut d = 1;
        let mut x = v;
        while x >= 10 { x /= 10; d += 1; }
        for _ in d..w { self.bytes(b"0"); }
        self.u(v)
    }
    pub fn i(&mut self, v: i64) -> &mut Self {
        if v < 0 { self.bytes(b"-"); }
        self.u(v.unsigned_abs())
    }
    pub fn ch(&mut self, cp: u32) -> &mut Self {
        let mut t = [0u8; 4];
        let n = utf8_encode(cp, &mut t);
        self.bytes(&t[..n])
    }
    /// Tamanho em KB/MB legivel.
    pub fn size(&mut self, bytes: u64) -> &mut Self {
        if bytes < 1024 { self.u(bytes).s(" B") }
        else if bytes < 1024 * 1024 { self.u(bytes.div_ceil(1024)).s(" KB") }
        else {
            let tenths = bytes * 10 / (1024 * 1024);
            self.u(tenths / 10).s(",").u(tenths % 10).s(" MB")
        }
    }
    pub fn get(&self) -> &[u8] { &self.b[..self.n] }
}

/// Converte segundos Unix em (ano, mes, dia, hora, min, seg, dia da semana).
pub fn civil_time(secs: u64) -> (u32, u32, u32, u32, u32, u32, u32) {
    let days = (secs / 86400) as i64;
    let rem = secs % 86400;
    let wday = ((days + 4) % 7) as u32; // 01/01/1970 foi quinta-feira
    // algoritmo de Howard Hinnant (days_from_civil inverso)
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y as u32, m as u32, d as u32, (rem / 3600) as u32, ((rem / 60) % 60) as u32, (rem % 60) as u32, wday)
}

pub const WEEKDAYS: [&str; 7] = ["Dom", "Seg", "Ter", "Qua", "Qui", "Sex", "Sáb"];
pub const MONTHS: [&str; 12] = ["jan", "fev", "mar", "abr", "mai", "jun", "jul", "ago", "set", "out", "nov", "dez"];
