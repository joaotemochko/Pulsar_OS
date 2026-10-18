//! Descritores de arquivo POSIX sobre o ext2.
//!
//! Cada processo (as threads usam a tabela do lider) tem MAX_FDS
//! descritores. 0, 1 e 2 comecam ligados ao console (escrita vai para a
//! serial, leitura da fim de arquivo). Caminhos relativos partem de um
//! diretorio `at` (o corrente do processo ou um descritor de diretorio); a
//! ABI do Pulsar so aceita caminhos absolutos.
//!
//! As estruturas devolvidas (`stat`, `linux_dirent64`) e os flags de open
//! seguem o layout generico do Linux, identico em aarch64 e riscv64, para a
//! libc poder repassa-las sem traducao.
//!
//! Trava: FS (fs.rs). Nunca pega SCHED com ela presa; o processo corrente
//! e descoberto antes.
use crate::ext2::{self, Res, EINVAL};
use crate::fs::FS;
use crate::process::{self, MAX_PROCS};
use crate::unix;
use core::sync::atomic::{AtomicU32, Ordering};

pub const MAX_FDS: usize = 64;

const EBADF: i64 = 9;
const EAGAIN: i64 = 11;
const ENFILE: i64 = 23;
const EMFILE: i64 = 24;
const ESPIPE: i64 = 29;
const EPIPE: i64 = 32;
const ENOTDIR: i64 = 20;
const EISDIR: i64 = 21;
const ENOSYS: i64 = 38;

/// Erro interno: a operacao precisa esperar (pipe vazia/cheia). A camada de
/// syscall chama `take_block()` e poe o processo em process::io_wait; a
/// syscall roda de novo quando houver novidade.
pub const EBLOCK: i64 = 0x7FFF_0000;

// flags de open (asm-generic)
pub const O_ACCMODE: u32 = 3;
pub const O_WRONLY: u32 = 1;
pub const O_RDWR: u32 = 2;
pub const O_CREAT: u32 = 0o100;
pub const O_EXCL: u32 = 0o200;
pub const O_TRUNC: u32 = 0o1000;
pub const O_APPEND: u32 = 0o2000;
pub const O_NONBLOCK: u32 = 0o4000;
pub const O_DIRECTORY: u32 = 0o200000;
pub const O_NOFOLLOW: u32 = 0o400000;
pub const O_CLOEXEC: u32 = 0o2000000;

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Kind { Free, Console, File, Dir, PipeR, PipeW, DevNull, DevZero, DevRandom, Mem, Event, Epoll, Sock,
    Unix, USock, Listen, Shm }

#[derive(Clone, Copy)]
pub(crate) struct OpenFile {
    kind: Kind,
    ino: u32,        // inode (File/Dir), indice da pipe (PipeR/PipeW) ou do texto (Mem)
    flags: u32,
    cloexec: bool,
    off: u64,
}

const FREE: OpenFile = OpenFile { kind: Kind::Free, ino: 0, flags: 0, cloexec: false, off: 0 };
pub(crate) const FREE_FILE: OpenFile = FREE;
const CONSOLE: OpenFile = OpenFile { kind: Kind::Console, ino: 0, flags: O_RDWR, cloexec: false, off: 0 };
const DEFAULT_TABLE: [OpenFile; MAX_FDS] = {
    let mut t = [FREE; MAX_FDS];
    t[0] = CONSOLE;
    t[1] = CONSOLE;
    t[2] = CONSOLE;
    t
};

static mut FDS: [[OpenFile; MAX_FDS]; MAX_PROCS] = [DEFAULT_TABLE; MAX_PROCS];

/// Diretorio corrente (inode) de cada processo, por slot do lider.
static CWD: [AtomicU32; MAX_PROCS] = [const { AtomicU32::new(ext2::ROOT_INO) }; MAX_PROCS];

/// AT_FDCWD do Linux: "relativo ao diretorio corrente".
pub const AT_FDCWD: i64 = -100;

/// Inodes sem links mas ainda abertos: liberados no ultimo close.
static mut ORPHANS: [u32; 32] = [0; 32];

fn table(l: usize) -> &'static mut [OpenFile; MAX_FDS] {
    unsafe { &mut (*(&raw mut FDS))[l] }
}

fn is_open(ino: u32) -> bool {
    (0..MAX_PROCS).any(|l| table(l).iter().any(|f| (f.kind == Kind::File || f.kind == Kind::Dir) && f.ino == ino))
}

fn add_orphan(ino: u32) {
    let o = unsafe { &mut *(&raw mut ORPHANS) };
    if let Some(s) = o.iter_mut().find(|s| **s == 0) { *s = ino; }
}

/// O descritor saiu: se era o ultimo aberto de um orfao, libera o inode.
fn after_close(ino: u32) {
    let o = unsafe { &mut *(&raw mut ORPHANS) };
    if let Some(s) = o.iter_mut().find(|s| **s == ino) {
        if !is_open(ino) {
            let _ = ext2::release_inode(ino);
            *s = 0;
        }
    }
}

// ------------------------------------------------------------ pipes

const MAX_PIPES: usize = 64;
const PIPE_PAGES: usize = 16;
const PIPE_CAP: u64 = (PIPE_PAGES * 4096) as u64;

#[derive(Clone, Copy)]
struct Pipe {
    buf: u64,       // anel de PIPE_CAP bytes (memoria do kernel)
    head: u64,      // proximo byte a ler
    len: u64,
    readers: u32,
    writers: u32,
}

static mut PIPES: [Pipe; MAX_PIPES] = [Pipe { buf: 0, head: 0, len: 0, readers: 0, writers: 0 }; MAX_PIPES];

fn pipes() -> &'static mut [Pipe; MAX_PIPES] {
    unsafe { &mut *(&raw mut PIPES) }
}

/// Chaves de espera: leitores esperam dados, escritores esperam espaco.
fn key_r(i: u32) -> u64 { 0x5049_5045_0000_0000 | (i as u64) << 1 }
fn key_w(i: u32) -> u64 { key_r(i) | 1 }

/// Acordares pendentes: gerados com FS presa, disparados depois (ordem
/// SCHED -> FS impede acordar alguem segurando FS).
static mut PENDING: [u64; 64] = [0; 64];
static mut PENDING_N: usize = 0;
/// Atalho sem trava: ha algo pendente? (quase sempre nao)
static PENDING_ANY: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

pub(crate) fn push_wake(key: u64) {
    PENDING_ANY.store(true, Ordering::Release);
    unsafe {
        let p = &mut *(&raw mut PENDING);
        if p[..PENDING_N].contains(&key) { return; }
        if PENDING_N < p.len() { p[PENDING_N] = key; PENDING_N += 1; }
    }
}

/// Tira os acordares pendentes (com FS presa).
fn take_wakes() -> ([u64; 64], usize) {
    PENDING_ANY.store(false, Ordering::Release);
    unsafe {
        let n = PENDING_N;
        PENDING_N = 0;
        (*(&raw const PENDING), n)
    }
}

/// Dispara os acordares pendentes. Chamar SEM FS e SEM SCHED (fim de syscall).
pub fn flush_wakes() {
    if !PENDING_ANY.load(Ordering::Acquire) { return; }
    let (keys, n) = { let _g = FS.guard(); take_wakes() };
    for &k in &keys[..n] { process::io_wake(k); }
}

/// Como flush_wakes, para quem ja segura SCHED (fim de processo, exec).
pub fn flush_wakes_sched_held(wake: &mut dyn FnMut(u64)) {
    if !PENDING_ANY.load(Ordering::Acquire) { return; }
    let (keys, n) = { let _g = FS.guard(); take_wakes() };
    for &k in &keys[..n] { wake(k); }
}

/// Mais uma referencia a `f` (dup, fork).
pub(crate) fn add_ref(f: &OpenFile) {
    match f.kind {
        Kind::PipeR => pipes()[f.ino as usize].readers += 1,
        Kind::PipeW => pipes()[f.ino as usize].writers += 1,
        Kind::Mem => mems()[f.ino as usize].refs += 1,
        Kind::Event => events()[f.ino as usize].refs += 1,
        Kind::Epoll => epolls()[f.ino as usize].refs += 1,
        Kind::Sock => socks()[f.ino as usize].refs += 1,
        Kind::Unix => unix::add_ref(f.ino as usize >> 1, f.ino as usize & 1),
        Kind::USock => usocks()[f.ino as usize].refs += 1,
        Kind::Listen => unix::listener_ref(f.ino as usize, 1),
        Kind::Shm => crate::shm::fd_ref(f.ino, 1),
        _ => {}
    }
}

/// Uma referencia a `f` acabou (close, dup2 por cima, exec, fim).
pub(crate) fn drop_ref(f: OpenFile) {
    match f.kind {
        Kind::File | Kind::Dir => after_close(f.ino),
        Kind::PipeR | Kind::PipeW => {
            let i = f.ino;
            let p = &mut pipes()[i as usize];
            if f.kind == Kind::PipeR { p.readers -= 1; push_wake(key_w(i)); } else { p.writers -= 1; push_wake(key_r(i)); }
            if p.readers == 0 && p.writers == 0 {
                crate::frame_allocator::free_contig(p.buf, PIPE_PAGES);
                *p = Pipe { buf: 0, head: 0, len: 0, readers: 0, writers: 0 };
            }
        }
        Kind::Mem => {
            let m = &mut mems()[f.ino as usize];
            m.refs -= 1;
            if m.refs == 0 {
                crate::frame_allocator::free_contig(m.buf, MEM_PAGES);
                *m = MemFile { buf: 0, len: 0, refs: 0 };
            }
        }
        Kind::Event => {
            let e = &mut events()[f.ino as usize];
            e.refs -= 1;
            if e.refs == 0 { e.used = false; }
        }
        Kind::Epoll => {
            let e = &mut epolls()[f.ino as usize];
            e.refs -= 1;
            if e.refs == 0 { e.used = false; e.n = 0; }
        }
        Kind::Sock => {
            let k = &mut socks()[f.ino as usize];
            k.refs -= 1;
            if k.refs == 0 {
                if let Some(h) = k.tcp {
                    let _g = crate::net::NET_LOCK.guard();
                    crate::net::tcp_close(h);
                }
                *k = Sock::EMPTY;
            }
        }
        Kind::Unix => unix::drop_ref(f.ino as usize >> 1, f.ino as usize & 1),
        Kind::USock => {
            let u = &mut usocks()[f.ino as usize];
            u.refs -= 1;
            if u.refs == 0 { *u = USock::EMPTY; }
        }
        Kind::Listen => unix::listener_ref(f.ino as usize, -1),
        Kind::Shm => crate::shm::fd_ref(f.ino, -1),
        _ => {}
    }
}

// ------------------------------------------------------------ /dev e /proc

/// Texto gerado na abertura (arquivos de /proc): somente leitura.
const MEM_PAGES: usize = 4;
#[derive(Clone, Copy)]
struct MemFile { buf: u64, len: u64, refs: u32 }
static mut MEMS: [MemFile; 16] = [MemFile { buf: 0, len: 0, refs: 0 }; 16];
fn mems() -> &'static mut [MemFile; 16] { unsafe { &mut *(&raw mut MEMS) } }

/// Escritor de texto num buffer fixo.
pub struct Text<'a> { pub buf: &'a mut [u8], pub len: usize }
impl core::fmt::Write for Text<'_> {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        let n = s.len().min(self.buf.len() - self.len);
        self.buf[self.len..self.len + n].copy_from_slice(&s.as_bytes()[..n]);
        self.len += n;
        Ok(())
    }
}

enum Special { Dev(Kind), Proc(&'static str) }

/// /proc/self/... ou /proc/<pid do corrente>/... -> resto do caminho.
fn proc_self(path: &[u8]) -> Option<&[u8]> {
    let rest = path.strip_prefix(b"/proc/")?;
    if let Some(r) = rest.strip_prefix(b"self/") { return Some(r); }
    let slash = rest.iter().position(|&c| c == b'/')?;
    let pid = core::str::from_utf8(&rest[..slash]).ok()?.parse::<usize>().ok()?;
    if pid == process::current_ids().1 { Some(&rest[slash + 1..]) } else { None }
}

fn special(path: &[u8]) -> Option<Special> {
    Some(match path {
        b"/dev/null" => Special::Dev(Kind::DevNull),
        b"/dev/zero" => Special::Dev(Kind::DevZero),
        b"/dev/urandom" | b"/dev/random" => Special::Dev(Kind::DevRandom),
        b"/dev/tty" | b"/dev/console" => Special::Dev(Kind::Console),
        b"/proc/cpuinfo" => Special::Proc("cpuinfo"),
        b"/proc/meminfo" => Special::Proc("meminfo"),
        b"/proc/version" => Special::Proc("version"),
        _ => match proc_self(path)? {
            b"maps" => Special::Proc("maps"),
            b"comm" => Special::Proc("comm"),
            b"cmdline" => Special::Proc("cmdline"),
            b"status" => Special::Proc("status"),
            b"stat" => Special::Proc("stat"),
            _ => return None,
        },
    })
}

/// Conteudo de um arquivo de /proc (gerado SEM a trava FS).
fn proc_text(which: &str, out: &mut [u8]) -> usize {
    use core::fmt::Write;
    let mut t = Text { buf: out, len: 0 };
    match which {
        "cpuinfo" => {
            for c in 0..process::online_cpus() {
                #[cfg(target_arch = "aarch64")]
                let _ = write!(t, "processor\t: {}\nBogoMIPS\t: 125.00\nFeatures\t: fp asimd evtstrm cpuid\n\
                    CPU implementer\t: 0x41\nCPU architecture: 8\nCPU part\t: 0xd03\n\n", c);
                #[cfg(target_arch = "riscv64")]
                let _ = write!(t, "processor\t: {}\nhart\t\t: {}\nisa\t\t: rv64imafdc\nmmu\t\t: sv39\n\n", c, c);
            }
        }
        "meminfo" => {
            let total = crate::frame_allocator::total_count() * 4;
            let free = crate::frame_allocator::free_count() * 4;
            let _ = write!(t, "MemTotal:       {} kB\nMemFree:        {} kB\nMemAvailable:   {} kB\n\
                Buffers:               0 kB\nCached:                0 kB\nSwapTotal:             0 kB\nSwapFree:              0 kB\n",
                total, free, free);
        }
        "version" => { let _ = write!(t, "Linux version 6.1.0-pulsar (Pulsar OS, personalidade Linux)\n"); }
        "maps" => { t.len = crate::vm::maps_text(t.buf); }
        "cmdline" => { t.len = process::cmdline(t.buf); }
        "status" | "stat" => {
            let mut b = [0u8; 256];
            let n = process::exe_path(&mut b);
            let name = core::str::from_utf8(b[..n].rsplit(|&c| c == b'/').next().unwrap_or(b"")).unwrap_or("?");
            let (tid, pid, ppid) = process::current_ids();
            let (threads, ticks) = process::thread_stats();
            let clk = crate::time::ticks_to_ms(ticks) / 10;              // em 1/100 s (CLK_TCK)
            if which == "status" {
                let _ = write!(t, "Name:\t{}\nUmask:\t0022\nState:\tR (running)\nTgid:\t{}\nPid:\t{}\nPPid:\t{}\n\
                    Uid:\t0\t0\t0\t0\nGid:\t0\t0\t0\t0\nThreads:\t{}\nVmRSS:\t0 kB\n\
                    SigQ:\t0/64\nCpus_allowed_list:\t0-{}\n", name, pid, tid, ppid, threads,
                    process::online_cpus().saturating_sub(1));
            } else {
                let _ = write!(t, "{} ({}) R {} {} {} 0 -1 0 0 0 0 0 {} 0 0 0 20 0 {} 0 0 0 0 \
                    18446744073709551615 0 0 0 0 0 0 0 0 0 0 0 0 17 0 0 0 0 0 0 0 0 0 0 0 0 0 0\n",
                    pid, name, ppid, pid, pid, clk, threads);
            }
        }
        "comm" => {
            let mut b = [0u8; 256];
            let n = process::exe_path(&mut b);
            let name = b[..n].rsplit(|&c| c == b'/').next().unwrap_or(b"");
            let _ = t.write_str(core::str::from_utf8(name).unwrap_or("?"));
            let _ = t.write_str("\n");
        }
        _ => {}
    }
    t.len
}

fn special_stat(k: &Special) -> Stat {
    match k {
        Special::Dev(_) => Stat { dev: 0, ino: 0, mode: 0o20666, nlink: 1, blksize: 4096, ..Stat::default() },
        Special::Proc(_) => Stat { dev: 2, ino: 0, mode: 0o100444, nlink: 1, blksize: 4096, ..Stat::default() },
    }
}

/// /proc/self/exe -> caminho do executavel.
fn special_link(path: &[u8], out: &mut [u8]) -> Option<usize> {
    if proc_self(path)? == b"exe" { Some(process::exe_path(out)) } else { None }
}

/// Bloqueio pendente desta CPU: (chave, contador lido sob FS, fatia em ms;
/// 0 = so acorda por evento).
static mut BLOCK_ON: [(u64, u64, u64); crate::arch::platform::MAX_CPUS] = [(0, 0, 0); crate::arch::platform::MAX_CPUS];

pub(crate) fn would_block(key: u64) -> i64 {
    would_block_for(key, 0)
}

/// Como would_block, mas reconfere depois de `ms` (a rede e lida por
/// polling: nao ha evento que acorde quem espera um socket).
fn would_block_for(key: u64, ms: u64) -> i64 {
    unsafe { BLOCK_ON[crate::arch::cpu_id()] = (key, process::io_seq(key), ms); }
    EBLOCK
}

/// (chave, contador, prazo em ticks ou 0) da ultima operacao que devolveu
/// EBLOCK nesta CPU.
pub fn take_block() -> (u64, u64, u64) {
    let (k, s, ms) = unsafe { BLOCK_ON[crate::arch::cpu_id()] };
    (k, s, if ms > 0 { crate::arch::now() + crate::time::ms_to_ticks(ms) } else { 0 })
}

/// Fatia de espera para sockets (ms).
pub const NET_SLICE_MS: u64 = 10;

/// pipe2(): retorna (fd de leitura, fd de escrita).
pub fn pipe(flags: u32) -> Res<(u64, u64)> {
    let l = leader()?;
    let _g = FS.guard();
    let i = pipes().iter().position(|p| p.buf == 0).ok_or(ENFILE)?;
    let buf = crate::frame_allocator::alloc_contig(PIPE_PAGES).ok_or(ext2::ENOSPC)?;
    pipes()[i] = Pipe { buf, head: 0, len: 0, readers: 1, writers: 1 };
    let fl = flags & O_NONBLOCK;
    let cloexec = flags & O_CLOEXEC != 0;
    let r = install(l, OpenFile { kind: Kind::PipeR, ino: i as u32, flags: fl, cloexec, off: 0 }, 0);
    let r = match r { Ok(r) => r, Err(e) => { pipes()[i].readers = 0; pipes()[i].writers = 0; crate::frame_allocator::free_contig(buf, PIPE_PAGES); pipes()[i].buf = 0; return Err(e); } };
    match install(l, OpenFile { kind: Kind::PipeW, ino: i as u32, flags: fl | O_WRONLY, cloexec, off: 0 }, 0) {
        Ok(w) => Ok((r, w)),
        Err(e) => {
            let f = table(l)[r as usize];
            table(l)[r as usize] = FREE;
            pipes()[i].writers = 0;
            drop_ref(f);
            Err(e)
        }
    }
}

fn pipe_read(fd_flags: u32, i: u32, dst: u64, len: u64) -> Res<u64> {
    let p = &mut pipes()[i as usize];
    if p.len == 0 {
        if p.writers == 0 { return Ok(0); }                 // fim: ninguem mais escreve
        if fd_flags & O_NONBLOCK != 0 { return Err(EAGAIN); }
        return Err(would_block(key_r(i)));
    }
    let n = len.min(p.len);
    let mut done = 0;
    while done < n {
        let at = (p.head + done) % PIPE_CAP;
        let chunk = (n - done).min(PIPE_CAP - at);
        unsafe { core::ptr::copy_nonoverlapping((p.buf + at) as *const u8, (dst + done) as *mut u8, chunk as usize) };
        done += chunk;
    }
    p.head = (p.head + n) % PIPE_CAP;
    p.len -= n;
    push_wake(key_w(i));
    Ok(n)
}

fn pipe_write(fd_flags: u32, i: u32, src: u64, len: u64) -> Res<u64> {
    let p = &mut pipes()[i as usize];
    if p.readers == 0 { return Err(EPIPE); }
    let space = PIPE_CAP - p.len;
    if space == 0 {
        if fd_flags & O_NONBLOCK != 0 { return Err(EAGAIN); }
        return Err(would_block(key_w(i)));
    }
    let n = len.min(space);
    let mut done = 0;
    while done < n {
        let at = (p.head + p.len + done) % PIPE_CAP;
        let chunk = (n - done).min(PIPE_CAP - at);
        unsafe { core::ptr::copy_nonoverlapping((src + done) as *const u8, (p.buf + at) as *mut u8, chunk as usize) };
        done += chunk;
    }
    p.len += n;
    push_wake(key_r(i));
    Ok(n)
}

fn leader() -> Res<usize> {
    process::current_leader().ok_or(EINVAL)
}

/// Fim do processo `l`: fecha tudo e volta ao padrao (console em 0..2).
/// Chamado com SCHED presa (SCHED -> FS); os acordares ficam pendentes.
pub fn release(l: usize) {
    let _g = FS.guard();
    let t = *table(l);
    *table(l) = DEFAULT_TABLE;
    CWD[l].store(ext2::ROOT_INO, Ordering::Relaxed);
    for f in t.iter() { drop_ref(*f); }
}

/// fork/vfork: o filho `dst` recebe uma copia da tabela de `src`.
pub fn clone_table(src: usize, dst: usize) {
    let _g = FS.guard();
    let t = *table(src);
    for f in t.iter() { add_ref(f); }
    *table(dst) = t;
}

/// execve: fecha os descritores marcados close-on-exec.
pub fn exec_cleanup(l: usize) {
    let _g = FS.guard();
    for i in 0..MAX_FDS {
        let f = table(l)[i];
        if f.kind != Kind::Free && f.cloexec {
            table(l)[i] = FREE;
            drop_ref(f);
        }
    }
}

fn need_ext2() -> Res<()> {
    if crate::fs::is_ext2() { Ok(()) } else { Err(ENOSYS) }
}

fn get(l: usize, fd: u64) -> Res<OpenFile> {
    let f = *table(l).get(fd as usize).ok_or(EBADF)?;
    if f.kind == Kind::Free { Err(EBADF) } else { Ok(f) }
}

fn install(l: usize, f: OpenFile, min: usize) -> Res<u64> {
    let t = table(l);
    for i in min..MAX_FDS {
        if t[i].kind == Kind::Free {
            t[i] = f;
            return Ok(i as u64);
        }
    }
    Err(EMFILE)
}

pub fn open(at: u32, path: &[u8], flags: u32, mode: u16) -> Res<u64> {
    need_ext2()?;
    let l = leader()?;
    if let Some(name) = shm_name(path) {
        let id = match crate::shm::open_named(name) {
            Some(id) if flags & O_CREAT != 0 && flags & O_EXCL != 0 => { crate::shm::fd_ref(id, -1); return Err(ext2::EEXIST); }
            Some(id) => id,
            None if flags & O_CREAT != 0 => crate::shm::create(Some(name)).ok_or(ENFILE)?,
            None => return Err(ext2::ENOENT),
        };
        if flags & O_TRUNC != 0 { let _ = crate::shm::truncate(id, 0); }
        let r = install_shm(l, id, flags);
        crate::shm::fd_ref(id, -1);            // a referencia agora e do descritor
        return r;
    }
    let mut exe = [0u8; 256];
    if let Some(n) = special_link(path, &mut exe) {
        // /proc/self/exe: abre o proprio executavel
        return open(ext2::ROOT_INO, &exe[..n], flags & !O_CREAT, mode);
    }
    if let Some(sp) = special(path) {
        let cloexec = flags & O_CLOEXEC != 0;
        let fl = flags & (O_ACCMODE | O_NONBLOCK);
        let f = match sp {
            Special::Dev(kind) => OpenFile { kind, ino: 0, flags: fl, cloexec, off: 0 },
            Special::Proc(which) => {
                let buf = crate::frame_allocator::alloc_contig(MEM_PAGES).ok_or(ext2::ENOSPC)?;
                let out = unsafe { core::slice::from_raw_parts_mut(buf as *mut u8, MEM_PAGES * 4096) };
                let len = proc_text(which, out) as u64;
                let _g = FS.guard();
                let Some(i) = mems().iter().position(|m| m.buf == 0) else {
                    crate::frame_allocator::free_contig(buf, MEM_PAGES);
                    return Err(ENFILE);
                };
                mems()[i] = MemFile { buf, len, refs: 1 };
                let f = OpenFile { kind: Kind::Mem, ino: i as u32, flags: fl & !O_ACCMODE, cloexec, off: 0 };
                return install(l, f, 0).inspect_err(|_| drop_ref(f));
            }
        };
        let _g = FS.guard();
        return install(l, f, 0);
    }
    let _g = FS.guard();
    let (ino, created) = if flags & O_CREAT != 0 {
        let r = ext2::create(at, path, mode, flags & O_EXCL != 0)?;
        if r.1 { crate::fs::legacy_dirty(); }
        r
    } else {
        (ext2::resolve_at(at, path, flags & O_NOFOLLOW == 0)?, false)
    };
    let n = ext2::read_inode(ino)?;
    let kind = match n.kind() {
        ext2::S_IFDIR => Kind::Dir,
        ext2::S_IFREG => Kind::File,
        ext2::S_IFLNK => return Err(ext2::ELOOP),   // O_NOFOLLOW num link
        _ => return Err(EINVAL),
    };
    if kind == Kind::Dir && flags & O_ACCMODE != 0 { return Err(EISDIR); }
    if kind != Kind::Dir && flags & O_DIRECTORY != 0 { return Err(ENOTDIR); }
    if kind == Kind::File && flags & O_TRUNC != 0 && flags & O_ACCMODE != 0 && !created {
        ext2::truncate(ino, 0)?;
    }
    let keep = O_ACCMODE | O_APPEND | O_NONBLOCK;
    install(l, OpenFile { kind, ino, flags: flags & keep, cloexec: flags & O_CLOEXEC != 0, off: 0 }, 0)
}

pub fn close(fd: u64) -> Res<u64> {
    let l = leader()?;
    let _g = FS.guard();
    let f = get(l, fd)?;
    table(l)[fd as usize] = FREE;
    drop_ref(f);
    Ok(0)
}

/// DUP(fd, novo): `novo` = u64::MAX escolhe o menor livre; `novo` com bits
/// altos (n << 32) escolhe o menor >= n (F_DUPFD); senao fecha o que
/// estiver em `novo` e duplica ali (dup2). O offset e copiado (nao
/// compartilhado). O descritor novo nunca herda close-on-exec, salvo
/// `cloexec`.
pub fn dup_ex(fd: u64, newfd: u64, cloexec: bool) -> Res<u64> {
    let l = leader()?;
    let _g = FS.guard();
    let mut f = get(l, fd)?;
    f.cloexec = cloexec;
    if newfd == u64::MAX || newfd >= 1 << 32 {
        let min = if newfd == u64::MAX { 0 } else { (newfd >> 32) as usize };
        let r = install(l, f, min)?;
        add_ref(&f);
        return Ok(r);
    }
    if newfd as usize >= MAX_FDS { return Err(EBADF); }
    if newfd == fd { return Ok(fd); }
    let old = table(l)[newfd as usize];
    table(l)[newfd as usize] = f;
    add_ref(&f);
    drop_ref(old);
    Ok(newfd)
}

pub fn dup(fd: u64, newfd: u64) -> Res<u64> {
    dup_ex(fd, newfd, false)
}

/// READ / PREAD (`at` = Some(offset) nao move o offset do descritor).
pub fn read(fd: u64, dst: u64, len: u64, at: Option<u64>) -> Res<u64> {
    let l = leader()?;
    let _g = FS.guard();
    let f = get(l, fd)?;
    match f.kind {
        Kind::Console | Kind::DevNull => Ok(0),
        Kind::DevZero | Kind::DevRandom => {
            let n = len.min(1 << 20);
            if f.kind == Kind::DevZero {
                unsafe { core::ptr::write_bytes(dst as *mut u8, 0, n as usize) };
            } else {
                let mut done = 0;
                while done < n {
                    let mut tmp = [0u8; 256];
                    let k = (n - done).min(256) as usize;
                    crate::rng::fill(&mut tmp[..k]);
                    unsafe { core::ptr::copy_nonoverlapping(tmp.as_ptr(), (dst + done) as *mut u8, k) };
                    done += k as u64;
                }
            }
            Ok(n)
        }
        Kind::Mem => {
            let m = mems()[f.ino as usize];
            let off = at.unwrap_or(f.off);
            if off >= m.len { return Ok(0); }
            let n = len.min(m.len - off);
            unsafe { core::ptr::copy_nonoverlapping((m.buf + off) as *const u8, dst as *mut u8, n as usize) };
            if at.is_none() { table(l)[fd as usize].off = off + n; }
            Ok(n)
        }
        Kind::Dir => Err(EISDIR),
        Kind::File => {
            if f.flags & O_ACCMODE == O_WRONLY { return Err(EBADF); }
            let off = at.unwrap_or(f.off);
            let n = ext2::read(f.ino, off, dst as *mut u8, len as usize)? as u64;
            if at.is_none() { table(l)[fd as usize].off = off + n; }
            Ok(n)
        }
        Kind::PipeR => if at.is_some() { Err(ESPIPE) } else { pipe_read(f.flags, f.ino, dst, len) },
        Kind::Unix => unix_read(f, dst, len, false).map(|(n, _, _)| n),
        Kind::Shm => {
            let off = at.unwrap_or(f.off);
            let n = crate::shm::rw(f.ino, off, dst, len, false)?;
            if at.is_none() { table(l)[fd as usize].off = off + n; }
            Ok(n)
        }
        Kind::USock | Kind::Listen => Err(ENOTCONN),
        Kind::Event => event_read(f.flags, f.ino, dst, len),
        Kind::Sock => sock_recv(f.flags, f.ino, dst, len, None),
        Kind::PipeW | Kind::Epoll => Err(EBADF),
        Kind::Free => Err(EBADF),
    }
}

pub fn write(fd: u64, src: u64, len: u64, at: Option<u64>) -> Res<u64> {
    let l = leader()?;
    let _g = FS.guard();
    let f = get(l, fd)?;
    match f.kind {
        Kind::Console => {
            let s = unsafe { core::slice::from_raw_parts(src as *const u8, len as usize) };
            crate::uart::write_bytes(s);
            Ok(len)
        }
        Kind::DevNull | Kind::DevZero | Kind::DevRandom => Ok(len),
        Kind::Mem => Err(EBADF),
        Kind::Dir => Err(EISDIR),
        Kind::File => {
            if f.flags & O_ACCMODE == 0 { return Err(EBADF); }
            let off = match at {
                Some(o) => o,
                None if f.flags & O_APPEND != 0 => ext2::read_inode(f.ino)?.size(),
                None => f.off,
            };
            let n = ext2::write(f.ino, off, src as *const u8, len as usize)? as u64;
            if at.is_none() { table(l)[fd as usize].off = off + n; }
            Ok(n)
        }
        Kind::PipeW => if at.is_some() { Err(ESPIPE) } else { pipe_write(f.flags, f.ino, src, len) },
        Kind::Unix => unix::write(f.ino as usize >> 1, f.ino as usize & 1, src, len, f.flags & O_NONBLOCK != 0, &[]),
        Kind::Shm => {
            let off = at.unwrap_or(f.off);
            let n = crate::shm::rw(f.ino, off, src, len, true)?;
            if at.is_none() { table(l)[fd as usize].off = off + n; }
            Ok(n)
        }
        Kind::USock | Kind::Listen => Err(ENOTCONN),
        Kind::Event => event_write(f.ino, src, len),
        Kind::Sock => sock_send(f.flags, f.ino, src, len, None),
        Kind::PipeR | Kind::Epoll => Err(EBADF),
        Kind::Free => Err(EBADF),
    }
}

pub fn lseek(fd: u64, off: i64, whence: u64) -> Res<u64> {
    let l = leader()?;
    let _g = FS.guard();
    let f = get(l, fd)?;
    if !matches!(f.kind, Kind::File | Kind::Dir | Kind::Mem | Kind::Shm) { return Err(ESPIPE); }
    let base = match whence {
        0 => 0i64,
        1 => f.off as i64,
        2 if f.kind == Kind::Mem => mems()[f.ino as usize].len as i64,
        2 if f.kind == Kind::Shm => crate::shm::size(f.ino) as i64,
        2 => ext2::read_inode(f.ino)?.size() as i64,
        _ => return Err(EINVAL),
    };
    let new = base.checked_add(off).filter(|&v| v >= 0).ok_or(EINVAL)?;
    table(l)[fd as usize].off = new as u64;
    Ok(new as u64)
}

/// FD_CLOEXEC do descritor (F_GETFD / F_SETFD).
pub fn get_cloexec(fd: u64) -> Res<bool> {
    let l = leader()?;
    let _g = FS.guard();
    Ok(get(l, fd)?.cloexec)
}

pub fn set_cloexec(fd: u64, on: bool) -> Res<u64> {
    let l = leader()?;
    let _g = FS.guard();
    get(l, fd)?;
    table(l)[fd as usize].cloexec = on;
    Ok(0)
}

pub fn ftruncate(fd: u64, len: u64) -> Res<u64> {
    let l = leader()?;
    let _g = FS.guard();
    let f = get(l, fd)?;
    if f.kind == Kind::Shm { crate::shm::truncate(f.ino, len)?; return Ok(0); }
    if f.kind != Kind::File || f.flags & O_ACCMODE == 0 { return Err(EINVAL); }
    ext2::truncate(f.ino, len)?;
    Ok(0)
}

/// fallocate(fd, 0, off, len): garante o arquivo com pelo menos off+len
/// bytes (como ftruncate, so cresce). Outros modos (furos, KEEP_SIZE...)
/// nao existem no ext2 do Pulsar: EOPNOTSUPP, e os programas caem no
/// caminho comum (o glib, por exemplo).
pub fn fallocate(fd: u64, mode: u64, off: u64, len: u64) -> Res<u64> {
    if mode != 0 { return Err(EOPNOTSUPP); }
    let end = off.checked_add(len).ok_or(EINVAL)?;
    let l = leader()?;
    let _g = FS.guard();
    let f = get(l, fd)?;
    if f.kind != Kind::File || f.flags & O_ACCMODE == 0 { return Err(EBADF); }
    if end > ext2::read_inode(f.ino)?.size() { ext2::truncate(f.ino, end)?; }
    Ok(0)
}

// ------------------------------------------------------------ memoria compartilhada

/// /dev/shm/<nome> -> nome (shm_open/shm_unlink da libc usam esse caminho).
fn shm_name(path: &[u8]) -> Option<&[u8]> {
    let n = path.strip_prefix(b"/dev/shm/")?;
    if n.is_empty() || n.contains(&b'/') { None } else { Some(n) }
}

fn install_shm(l: usize, id: u32, flags: u32) -> Res<u64> {
    let _g = FS.guard();
    let f = OpenFile { kind: Kind::Shm, ino: id, flags: flags & (O_ACCMODE | O_NONBLOCK), cloexec: flags & O_CLOEXEC != 0, off: 0 };
    install(l, f, 0).inspect_err(|_| drop_ref(f))
}

/// memfd_create: objeto anonimo com um descritor.
pub fn memfd(cloexec: bool) -> Res<u64> {
    let l = leader()?;
    let id = crate::shm::create(None).ok_or(ENFILE)?;
    install_shm(l, id, O_RDWR | if cloexec { O_CLOEXEC } else { 0 })
}

/// Objeto compartilhado por tras do descritor (mmap MAP_SHARED).
pub fn fd_obj(fd: u64) -> Res<Option<u32>> {
    let l = leader()?;
    let _g = FS.guard();
    let f = get(l, fd)?;
    Ok(if f.kind == Kind::Shm { Some(f.ino) } else { None })
}

/// `struct stat` generico do Linux (aarch64 e riscv64), 128 bytes.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct Stat {
    pub dev: u64,
    pub ino: u64,
    pub mode: u32,
    pub nlink: u32,
    pub uid: u32,
    pub gid: u32,
    pub rdev: u64,
    pub _pad1: u64,
    pub size: i64,
    pub blksize: i32,
    pub _pad2: i32,
    pub blocks: i64,
    pub atime: i64,
    pub atime_ns: u64,
    pub mtime: i64,
    pub mtime_ns: u64,
    pub ctime: i64,
    pub ctime_ns: u64,
    pub _unused: [u32; 2],
}

fn stat_ino(ino: u32) -> Res<Stat> {
    let n = ext2::read_inode(ino)?;
    Ok(Stat {
        dev: 1,
        ino: ino as u64,
        mode: n.mode as u32,
        nlink: n.links as u32,
        uid: n.uid as u32,
        gid: n.gid as u32,
        size: n.size() as i64,
        blksize: ext2::stats().0 as i32,
        blocks: n.blocks512 as i64,
        atime: n.atime as i64,
        mtime: n.mtime as i64,
        ctime: n.ctime as i64,
        ..Stat::default()
    })
}

pub fn fstat(fd: u64) -> Res<Stat> {
    let l = leader()?;
    let _g = FS.guard();
    let f = get(l, fd)?;
    if f.kind == Kind::Console {
        return Ok(Stat { dev: 0, ino: 0, mode: 0o20620, nlink: 1, blksize: 1024, ..Stat::default() });
    }
    match f.kind {
        Kind::PipeR | Kind::PipeW =>
            return Ok(Stat { dev: 0, ino: 0x10000 + f.ino as u64, mode: 0o10600, nlink: 1, blksize: 4096, ..Stat::default() }),
        Kind::DevNull | Kind::DevZero | Kind::DevRandom => return Ok(special_stat(&Special::Dev(f.kind))),
        Kind::Mem => return Ok(Stat { size: 0, ..special_stat(&Special::Proc("")) }),
        Kind::Sock | Kind::Unix | Kind::USock | Kind::Listen =>
            return Ok(Stat { mode: 0o140777, nlink: 1, blksize: 4096, ..Stat::default() }),
        Kind::Shm => return Ok(Stat { dev: 3, ino: f.ino as u64, mode: 0o100600, nlink: 1, blksize: 4096,
                                      size: crate::shm::size(f.ino) as i64, ..Stat::default() }),
        Kind::Event | Kind::Epoll => return Ok(Stat { mode: 0o600, nlink: 1, blksize: 4096, ..Stat::default() }),
        _ => {}
    }
    stat_ino(f.ino)
}

pub fn stat(at: u32, path: &[u8], follow: bool) -> Res<Stat> {
    need_ext2()?;
    if let Some(name) = shm_name(path) {
        let id = crate::shm::open_named(name).ok_or(ext2::ENOENT)?;
        let size = crate::shm::size(id);
        crate::shm::fd_ref(id, -1);
        return Ok(Stat { dev: 3, ino: id as u64, mode: 0o100600, nlink: 1, blksize: 4096, size: size as i64, ..Stat::default() });
    }
    if let Some(sp) = special(path) { return Ok(special_stat(&sp)); }
    let mut exe = [0u8; 256];
    if let Some(n) = special_link(path, &mut exe) {
        if !follow { return Ok(Stat { mode: 0o120777, nlink: 1, size: n as i64, ..Stat::default() }); }
        return stat(ext2::ROOT_INO, &exe[..n], true);
    }
    let _g = FS.guard();
    stat_ino(ext2::resolve_at(at, path, follow)?)
}

/// getdents64: preenche `buf` com `linux_dirent64` a partir do offset do
/// descritor. Retorna os bytes escritos (0 = fim).
pub fn getdents(fd: u64, buf: u64, len: u64) -> Res<u64> {
    let l = leader()?;
    let _g = FS.guard();
    let f = get(l, fd)?;
    if f.kind != Kind::Dir { return Err(ENOTDIR); }
    let mut pos = f.off;
    let mut out = 0u64;
    while let Some(d) = ext2::readdir(f.ino, pos)? {
        let reclen = (19 + d.name_len as u64 + 1 + 7) & !7;
        if out + reclen > len {
            if out == 0 { return Err(EINVAL); }
            break;
        }
        let dtype = match d.ftype { 1 => 8, 2 => 4, 3 => 2, 4 => 6, 5 => 1, 6 => 12, 7 => 10, _ => 0 };
        unsafe {
            let p = (buf + out) as *mut u8;
            core::ptr::write_bytes(p, 0, reclen as usize);
            core::ptr::write_unaligned(p as *mut u64, d.ino as u64);
            core::ptr::write_unaligned(p.add(8) as *mut i64, d.next as i64);
            core::ptr::write_unaligned(p.add(16) as *mut u16, reclen as u16);
            *p.add(18) = dtype;
            core::ptr::copy_nonoverlapping(d.name.as_ptr(), p.add(19), d.name_len);
        }
        out += reclen;
        pos = d.next;
    }
    table(l)[fd as usize].off = pos;
    Ok(out)
}

pub fn mkdir(at: u32, path: &[u8], mode: u16) -> Res<u64> {
    need_ext2()?;
    let _g = FS.guard();
    ext2::mkdir(at, path, mode)?;
    Ok(0)
}

pub fn unlink(at: u32, path: &[u8], rmdir: bool) -> Res<u64> {
    need_ext2()?;
    if let Some(name) = shm_name(path) {
        return if crate::shm::unlink(name) { Ok(0) } else { Err(ext2::ENOENT) };
    }
    let _g = FS.guard();
    if rmdir {
        ext2::rmdir(at, path)?;
    } else if let Some(o) = ext2::unlink(at, path, is_open)? {
        add_orphan(o);
    }
    crate::fs::legacy_dirty();
    Ok(0)
}

pub fn rename(fat: u32, from: &[u8], tat: u32, to: &[u8]) -> Res<u64> {
    need_ext2()?;
    let _g = FS.guard();
    if let Some(o) = ext2::rename(fat, from, tat, to, is_open)? { add_orphan(o); }
    crate::fs::legacy_dirty();
    Ok(0)
}

pub fn symlink(target: &[u8], at: u32, path: &[u8]) -> Res<u64> {
    need_ext2()?;
    let _g = FS.guard();
    ext2::symlink(target, at, path)?;
    Ok(0)
}

pub fn readlink(at: u32, path: &[u8], dst: u64, len: u64) -> Res<u64> {
    need_ext2()?;
    let mut exe = [0u8; 256];
    if let Some(n) = special_link(path, &mut exe) {
        let n = n.min(len as usize);
        unsafe { core::ptr::copy_nonoverlapping(exe.as_ptr(), dst as *mut u8, n) };
        return Ok(n as u64);
    }
    let _g = FS.guard();
    let ino = ext2::resolve_at(at, path, false)?;
    let mut buf = [0u8; ext2::PATH_MAX];
    let n = ext2::readlink(ino, &mut buf)?.min(len as usize);
    unsafe { core::ptr::copy_nonoverlapping(buf.as_ptr(), dst as *mut u8, n) };
    Ok(n as u64)
}

/// statfs simplificado: (bloco, total, livres, inodes, inodes livres).
pub fn fsinfo() -> Res<[u64; 5]> {
    need_ext2()?;
    let _g = FS.guard();
    let (bs, b, fb, i, fi) = ext2::stats();
    Ok([bs, b, fb, i, fi])
}

/// Le um arquivo regular inteiro para memoria fisica contigua (loader).
/// Retorna (endereco, frames alocados, tamanho); o chamador libera com
/// free_contig.
pub fn read_whole(at: u32, path: &[u8], max: u64) -> Res<(u64, usize, u64)> {
    need_ext2()?;
    let _g = FS.guard();
    let ino = ext2::resolve_at(at, path, true)?;
    let n = ext2::read_inode(ino)?;
    if n.is_dir() { return Err(EISDIR); }
    if n.kind() != ext2::S_IFREG { return Err(EINVAL); }
    let size = n.size();
    if size > max { return Err(ext2::EFBIG); }
    let frames = (size.div_ceil(4096) + 1) as usize;
    let buf = crate::frame_allocator::alloc_contig(frames).ok_or(ext2::ENOSPC)?;
    if let Err(e) = ext2::read_all_phys(ino, buf) {
        crate::frame_allocator::free_contig(buf, frames);
        return Err(e);
    }
    Ok((buf, frames, size))
}

// ------------------------------------------------------------ diretorio corrente

/// Inode de partida para caminhos relativos: o diretorio corrente
/// (AT_FDCWD) ou um descritor de diretorio.
pub fn dir_of(dirfd: i64) -> Res<u32> {
    let l = leader()?;
    if dirfd == AT_FDCWD { return Ok(CWD[l].load(Ordering::Relaxed)); }
    let _g = FS.guard();
    let f = get(l, dirfd as u64)?;
    if f.kind != Kind::Dir { return Err(ENOTDIR); }
    Ok(f.ino)
}

pub fn cwd_of(l: usize) -> u32 {
    CWD[l].load(Ordering::Relaxed)
}

pub fn set_cwd_of(l: usize, ino: u32) {
    CWD[l].store(ino, Ordering::Relaxed);
}

pub fn chdir(at: u32, path: &[u8]) -> Res<u64> {
    need_ext2()?;
    let l = leader()?;
    let _g = FS.guard();
    let ino = ext2::resolve_at(at, path, true)?;
    if !ext2::read_inode(ino)?.is_dir() { return Err(ENOTDIR); }
    CWD[l].store(ino, Ordering::Relaxed);
    Ok(0)
}

pub fn getcwd(out: &mut [u8]) -> Res<usize> {
    need_ext2()?;
    let l = leader()?;
    let _g = FS.guard();
    ext2::dir_path(CWD[l].load(Ordering::Relaxed), out)
}

/// O descritor e o console (terminal)?
pub fn is_console(fd: u64) -> Res<bool> {
    let l = leader()?;
    let _g = FS.guard();
    Ok(get(l, fd)?.kind == Kind::Console)
}

/// Flags de status do descritor (F_GETFL / F_SETFL: so O_APPEND muda).
pub fn get_flags(fd: u64) -> Res<u32> {
    let l = leader()?;
    let _g = FS.guard();
    Ok(get(l, fd)?.flags)
}

pub fn set_flags(fd: u64, flags: u32) -> Res<u64> {
    let l = leader()?;
    let _g = FS.guard();
    get(l, fd)?;
    let f = &mut table(l)[fd as usize];
    f.flags = (f.flags & !(O_APPEND | O_NONBLOCK)) | (flags & (O_APPEND | O_NONBLOCK));
    Ok(0)
}

// ------------------------------------------------------------ eventfd

#[derive(Clone, Copy)]
struct EventFd { used: bool, refs: u32, count: u64, sem: bool }
static mut EVENTS: [EventFd; 64] = [EventFd { used: false, refs: 0, count: 0, sem: false }; 64];
fn events() -> &'static mut [EventFd; 64] { unsafe { &mut *(&raw mut EVENTS) } }
fn key_ev(i: u32) -> u64 { 0x4556_0000_0000 | i as u64 }

/// eventfd2(valor, flags): EFD_SEMAPHORE 1, EFD_NONBLOCK, EFD_CLOEXEC.
pub fn eventfd(init: u64, flags: u32) -> Res<u64> {
    let l = leader()?;
    let _g = FS.guard();
    let i = events().iter().position(|e| !e.used).ok_or(ENFILE)?;
    events()[i] = EventFd { used: true, refs: 1, count: init, sem: flags & 1 != 0 };
    let f = OpenFile { kind: Kind::Event, ino: i as u32, flags: O_RDWR | (flags & O_NONBLOCK),
                       cloexec: flags & O_CLOEXEC != 0, off: 0 };
    install(l, f, 0).inspect_err(|_| drop_ref(f))
}

fn event_read(fl: u32, i: u32, dst: u64, len: u64) -> Res<u64> {
    if len < 8 { return Err(EINVAL); }
    let e = &mut events()[i as usize];
    if e.count == 0 {
        if fl & O_NONBLOCK != 0 { return Err(EAGAIN); }
        return Err(would_block(key_ev(i)));
    }
    let v = if e.sem { 1 } else { e.count };
    e.count -= v;
    unsafe { core::ptr::write_unaligned(dst as *mut u64, v) };
    push_wake(key_ev(i));
    Ok(8)
}

fn event_write(i: u32, src: u64, len: u64) -> Res<u64> {
    if len < 8 { return Err(EINVAL); }
    let v = unsafe { core::ptr::read_unaligned(src as *const u64) };
    if v == u64::MAX { return Err(EINVAL); }
    let e = &mut events()[i as usize];
    e.count = e.count.saturating_add(v).min(u64::MAX - 1);
    push_wake(key_ev(i));
    Ok(8)
}

// ------------------------------------------------------------ sockets

const ECONNREFUSED: i64 = 111;
const EINPROGRESS: i64 = 115;
const EALREADY: i64 = 114;
const EISCONN: i64 = 106;
const ENOTCONN: i64 = 107;
const EDESTADDRREQ: i64 = 89;
const EMSGSIZE: i64 = 90;
const ECONNRESET: i64 = 104;
const EOPNOTSUPP: i64 = 95;
const EAFNOSUPPORT: i64 = 97;
const ENOTSOCK: i64 = 88;

/// Chave de espera dos sockets (a rede e lida por polling).
const SOCK_KEY: u64 = 0x534F_434B;

#[derive(Clone, Copy, PartialEq)]
enum SockState { New, Connecting, Connected, Failed }

#[derive(Clone, Copy)]
struct Sock {
    used: bool,
    refs: u32,
    udp: bool,
    tcp: Option<usize>,
    lport: u16,
    rip: [u8; 4],
    rport: u16,
    state: SockState,
    err: i64,
}

impl Sock {
    const EMPTY: Sock = Sock { used: false, refs: 0, udp: false, tcp: None, lport: 0, rip: [0; 4], rport: 0,
                               state: SockState::New, err: 0 };
}

static mut SOCKS: [Sock; 64] = [Sock::EMPTY; 64];
fn socks() -> &'static mut [Sock; 64] { unsafe { &mut *(&raw mut SOCKS) } }

/// Endereco IPv4 (sockaddr_in ja decodificado).
#[derive(Clone, Copy)]
pub struct Addr { pub ip: [u8; 4], pub port: u16 }

/// socket(AF_INET, SOCK_STREAM | SOCK_DGRAM [| SOCK_NONBLOCK | SOCK_CLOEXEC]).
pub fn socket(domain: u64, ty: u64) -> Res<u64> {
    let l = leader()?;
    if domain == 1 {
        let base = ty & 0xF;
        if !matches!(base, 1 | 5) { return Err(if base == 2 { EOPNOTSUPP } else { EINVAL }); }
        let _g = FS.guard();
        let i = usocks().iter().position(|u| !u.used).ok_or(ENFILE)?;
        usocks()[i] = USock { used: true, refs: 1, packet: base == 5, key: unix::Key::None };
        let f = OpenFile { kind: Kind::USock, ino: i as u32, flags: O_RDWR | (ty as u32 & O_NONBLOCK),
                           cloexec: ty as u32 & O_CLOEXEC != 0, off: 0 };
        return install(l, f, 0).inspect_err(|_| drop_ref(f));
    }
    if domain != 2 { return Err(EAFNOSUPPORT); }
    let base = ty & 0xF;
    if base != 1 && base != 2 { return Err(EINVAL); }
    let _g = FS.guard();
    let i = socks().iter().position(|k| !k.used).ok_or(ENFILE)?;
    socks()[i] = Sock { used: true, refs: 1, udp: base == 2, ..Sock::EMPTY };
    let f = OpenFile { kind: Kind::Sock, ino: i as u32, flags: O_RDWR | (ty as u32 & O_NONBLOCK),
                       cloexec: ty as u32 & O_CLOEXEC != 0, off: 0 };
    install(l, f, 0).inspect_err(|_| drop_ref(f))
}

/// socketpair(AF_UNIX, SOCK_STREAM | SOCK_SEQPACKET | SOCK_DGRAM).
pub fn socketpair(domain: u64, ty: u64) -> Res<(u64, u64)> {
    if domain != 1 { return Err(EAFNOSUPPORT); }
    let l = leader()?;
    let base = ty & 0xF;
    if !matches!(base, 1 | 2 | 5) { return Err(EINVAL); }
    let _g = FS.guard();
    let c = unix::conn_new(base != 1)?;
    let mk = |e: usize| OpenFile { kind: Kind::Unix, ino: (c * 2 + e) as u32, flags: O_RDWR | (ty as u32 & O_NONBLOCK),
                                   cloexec: ty as u32 & O_CLOEXEC != 0, off: 0 };
    let a = match install(l, mk(0), 0) { Ok(a) => a, Err(e) => { unix::drop_ref(c, 0); unix::drop_ref(c, 1); return Err(e); } };
    match install(l, mk(1), 0) {
        Ok(b) => Ok((a, b)),
        Err(e) => { table(l)[a as usize] = FREE; unix::drop_ref(c, 0); unix::drop_ref(c, 1); Err(e) }
    }
}

// ------------------------------------------------------------ AF_UNIX

/// Socket AF_UNIX ainda nao conectado (pode receber bind e listen).
#[derive(Clone, Copy)]
struct USock { used: bool, refs: u32, packet: bool, key: unix::Key }
impl USock { const EMPTY: USock = USock { used: false, refs: 0, packet: false, key: unix::Key::None }; }
static mut USOCKS: [USock; 32] = [USock::EMPTY; 32];
fn usocks() -> &'static mut [USock; 32] { unsafe { &mut *(&raw mut USOCKS) } }

/// Endereco de socket ja decodificado.
#[derive(Clone, Copy)]
pub enum SockAddr { Inet(Addr), Path([u8; 108], usize), Abstract([u8; 108], usize) }

const EADDRINUSE: i64 = 98;

/// Chave de um endereco AF_UNIX: caminho (o inode do socket) ou nome abstrato.
/// `l` = leader do chamador, obtido ANTES de FS (leader() trava SCHED, e a
/// ordem e SCHED -> FS: pegar SCHED segurando FS trava com quem faz exec).
fn unix_key(l: usize, a: &SockAddr, create: bool) -> Res<unix::Key> {
    match a {
        SockAddr::Abstract(n, len) => Ok(unix::Key::Abstract(*n, *len)),
        SockAddr::Path(p, len) => {
            let path = &p[..*len];
            let at = cwd_of(l);
            if create {
                return match ext2::mknod_socket(at, path, 0o755) {
                    Ok(ino) => { crate::fs::legacy_dirty(); Ok(unix::Key::Ino(ino)) }
                    Err(ext2::EEXIST) => Err(EADDRINUSE),
                    Err(e) => Err(e),
                };
            }
            let ino = ext2::resolve_at(at, path, true)?;
            if ext2::read_inode(ino)?.kind() != ext2::S_IFSOCK { return Err(ECONNREFUSED); }
            Ok(unix::Key::Ino(ino))
        }
        SockAddr::Inet(_) => Err(EINVAL),
    }
}

/// accept4: (fd novo, porta do outro lado se for loopback TCP).
pub fn accept(fd: u64, flags: u32) -> Res<(u64, Option<u16>)> {
    let l = leader()?;
    let _g = FS.guard();
    let f = get(l, fd)?;
    if f.kind != Kind::Listen { return Err(EINVAL); }
    let c = unix::accept(f.ino as usize, f.flags & O_NONBLOCK != 0)?;
    let nf = OpenFile { kind: Kind::Unix, ino: (c * 2 + 1) as u32, flags: O_RDWR | (flags & O_NONBLOCK),
                        cloexec: flags & O_CLOEXEC != 0, off: 0 };
    let peer = match unix::listener_key(f.ino as usize) { unix::Key::Port(_) => Some(unix::ports(c, 1).1), _ => None };
    install(l, nf, 0).map(|n| (n, peer)).inspect_err(|_| unix::drop_ref(c, 1))
}

fn unix_read(f: OpenFile, dst: u64, len: u64, nonblock: bool) -> Res<(u64, usize, bool)> {
    let mut fds = ([FREE; unix::MAX_FDS], 0);
    let (n, trunc) = unix::read(f.ino as usize >> 1, f.ino as usize & 1, dst, len,
                                nonblock || f.flags & O_NONBLOCK != 0, &mut fds)?;
    // read() comum nao recebe descritores: os anexados sao fechados
    for x in &fds.0[..fds.1] { drop_ref(*x); }
    Ok((n, 0, trunc))
}

/// sendmsg em socket local com descritores (SCM_RIGHTS).
pub fn unix_send(fd: u64, src: u64, len: u64, fds: &[i32], nonblock: bool) -> Res<u64> {
    let l = leader()?;
    let _g = FS.guard();
    let f = get(l, fd)?;
    if f.kind != Kind::Unix { return Err(ENOTSOCK); }
    let mut files = [FREE; unix::MAX_FDS];
    if fds.len() > unix::MAX_FDS { return Err(EINVAL); }
    for (k, &d) in fds.iter().enumerate() {
        let x = get(l, d as u64)?;
        files[k] = OpenFile { cloexec: false, ..x };
    }
    for x in &files[..fds.len()] { add_ref(x); }
    let r = unix::write(f.ino as usize >> 1, f.ino as usize & 1, src, len,
                        nonblock || f.flags & O_NONBLOCK != 0, &files[..fds.len()]);
    if r.is_err() { for x in &files[..fds.len()] { drop_ref(*x); } }
    r
}

/// recvmsg em socket local: instala os descritores recebidos e devolve
/// (bytes, quantos fds, truncou).
pub fn unix_recv(fd: u64, dst: u64, len: u64, nonblock: bool, cloexec: bool, out: &mut [i32]) -> Res<(u64, usize, bool)> {
    let l = leader()?;
    let _g = FS.guard();
    let f = get(l, fd)?;
    if f.kind != Kind::Unix { return Err(ENOTSOCK); }
    let mut fds = ([FREE; unix::MAX_FDS], 0);
    let (n, trunc) = unix::read(f.ino as usize >> 1, f.ino as usize & 1, dst, len,
                                nonblock || f.flags & O_NONBLOCK != 0, &mut fds)?;
    let mut k = 0;
    for x in &fds.0[..fds.1] {
        if k < out.len() {
            if let Ok(nfd) = install(l, OpenFile { cloexec, ..*x }, 0) { out[k] = nfd as i32; k += 1; continue; }
        }
        drop_ref(*x);       // sem espaco: fecha (MSG_CTRUNC)
    }
    Ok((n, k, trunc))
}

/// shutdown(fd, how)
pub fn shutdown(fd: u64, how: u64) -> Res<u64> {
    let l = leader()?;
    let _g = FS.guard();
    let f = get(l, fd)?;
    match f.kind {
        Kind::Unix => { unix::shutdown(f.ino as usize >> 1, f.ino as usize & 1, how); Ok(0) }
        Kind::Sock | Kind::USock | Kind::Listen => Ok(0),
        _ => Err(ENOTSOCK),
    }
}

fn sock_of(l: usize, fd: u64) -> Res<(OpenFile, usize)> {
    let f = get(l, fd)?;
    if f.kind != Kind::Sock { return Err(ENOTSOCK); }
    Ok((f, f.ino as usize))
}

pub fn bind(fd: u64, sa: SockAddr) -> Res<u64> {
    let l = leader()?;
    let _g = FS.guard();
    let f = get(l, fd)?;
    if f.kind == Kind::USock {
        let u = &mut usocks()[f.ino as usize];
        if u.key != unix::Key::None { return Err(EINVAL); }
        u.key = unix_key(l, &sa, true)?;
        return Ok(0);
    }
    let SockAddr::Inet(a) = sa else { return Err(EINVAL) };
    let (_, i) = sock_of(l, fd)?;
    let k = &mut socks()[i];
    k.lport = if a.port != 0 { a.port } else { let _n = crate::net::NET_LOCK.guard(); crate::net::udp_alloc_port() };
    Ok(0)
}

/// connect(): TCP bloqueante espera o handshake (refazendo a syscall);
/// nao bloqueante devolve EINPROGRESS. UDP so fixa o destino.
pub fn connect(fd: u64, sa: SockAddr) -> Res<u64> {
    let l = leader()?;
    let _g = FS.guard();
    let f = get(l, fd)?;
    if f.kind == Kind::USock {
        let u = usocks()[f.ino as usize];
        let key = unix_key(l, &sa, false)?;
        let c = unix::connect(&key, u.packet)?;
        table(l)[fd as usize] = OpenFile { kind: Kind::Unix, ino: (c * 2) as u32, ..f };
        drop_ref(f);
        return Ok(0);
    }
    let SockAddr::Inet(a) = sa else { return Err(EINVAL) };
    if a.ip[0] == 127 && f.kind == Kind::Sock && !socks()[f.ino as usize].udp {
        // loopback TCP: conexao local com quem fez listen na porta
        let c = unix::connect(&unix::Key::Port(a.port), false)?;
        let lport = { let _n = crate::net::NET_LOCK.guard(); crate::net::udp_alloc_port() };
        unix::set_ports(c, lport, a.port);
        table(l)[fd as usize] = OpenFile { kind: Kind::Unix, ino: (c * 2) as u32, ..f };
        drop_ref(f);
        return Ok(0);
    }
    let (f, i) = sock_of(l, fd)?;
    let k = &mut socks()[i];
    if k.udp {
        k.rip = a.ip;
        k.rport = a.port;
        if k.lport == 0 { let _n = crate::net::NET_LOCK.guard(); k.lport = crate::net::udp_alloc_port(); }
        k.state = SockState::Connected;
        return Ok(0);
    }
    let _n = crate::net::NET_LOCK.guard();
    match k.state {
        SockState::Connected => return Err(EISCONN),
        SockState::Failed => return Err(if k.err != 0 { k.err } else { ECONNREFUSED }),
        SockState::New => {
            let h = crate::net::tcp_connect(&a.ip, a.port).ok_or(ENFILE)?;
            k.tcp = Some(h);
            k.rip = a.ip;
            k.rport = a.port;
            k.state = SockState::Connecting;
            if f.flags & O_NONBLOCK != 0 { return Err(EINPROGRESS); }
        }
        SockState::Connecting => if f.flags & O_NONBLOCK != 0 { return Err(EALREADY); },
    }
    sock_progress(k);
    match k.state {
        SockState::Connected => Ok(0),
        SockState::Failed => Err(k.err),
        _ => Err(would_block_for(SOCK_KEY, NET_SLICE_MS)),
    }
}

/// Atualiza o estado de um socket TCP conectando (NET_LOCK presa).
fn sock_progress(k: &mut Sock) {
    if k.state != SockState::Connecting { return; }
    let Some(h) = k.tcp else { return };
    match crate::net::tcp_status(h) {
        0 => {}
        1 | 2 => k.state = SockState::Connected,
        _ => { k.state = SockState::Failed; k.err = ECONNREFUSED; }
    }
}

pub fn sock_send(fl: u32, i: u32, src: u64, len: u64, to: Option<Addr>) -> Res<u64> {
    let k = &mut socks()[i as usize];
    let _n = crate::net::NET_LOCK.guard();
    let data = unsafe { core::slice::from_raw_parts(src as *const u8, len as usize) };
    if k.udp {
        let dst = to.or(if k.rport != 0 { Some(Addr { ip: k.rip, port: k.rport }) } else { None }).ok_or(EDESTADDRREQ)?;
        if len > 1472 { return Err(EMSGSIZE); }
        if k.lport == 0 { k.lport = crate::net::udp_alloc_port(); }
        if !crate::net::udp_send(&dst.ip, dst.port, k.lport, data) { return Err(EAGAIN); }
        return Ok(len);
    }
    sock_progress(k);
    let h = match (k.state, k.tcp) {
        (SockState::Connected, Some(h)) => h,
        (SockState::Connecting, _) => return if fl & O_NONBLOCK != 0 { Err(EAGAIN) } else { Err(would_block_for(SOCK_KEY, NET_SLICE_MS)) },
        _ => return Err(ENOTCONN),
    };
    match crate::net::tcp_send(h, data) {
        n if n > 0 => Ok(n as u64),
        0 if len == 0 => Ok(0),
        0 => if fl & O_NONBLOCK != 0 { Err(EAGAIN) } else { Err(would_block_for(SOCK_KEY, NET_SLICE_MS)) },
        _ => Err(EPIPE),
    }
}

/// recv/recvfrom: devolve tambem o remetente em `from` (UDP).
pub fn sock_recv(fl: u32, i: u32, dst: u64, len: u64, from: Option<&mut Addr>) -> Res<u64> {
    let k = &mut socks()[i as usize];
    let _n = crate::net::NET_LOCK.guard();
    let out = unsafe { core::slice::from_raw_parts_mut(dst as *mut u8, len as usize) };
    if k.udp {
        if k.lport == 0 { return Err(EAGAIN); }
        return match crate::net::udp_recv(k.lport, out) {
            Some((ip, port, n)) => { if let Some(a) = from { *a = Addr { ip, port }; } Ok(n as u64) }
            None => if fl & O_NONBLOCK != 0 { Err(EAGAIN) } else { Err(would_block_for(SOCK_KEY, NET_SLICE_MS)) },
        };
    }
    sock_progress(k);
    let h = match (k.state, k.tcp) {
        (SockState::Connected, Some(h)) => h,
        (SockState::Connecting, _) => return if fl & O_NONBLOCK != 0 { Err(EAGAIN) } else { Err(would_block_for(SOCK_KEY, NET_SLICE_MS)) },
        _ => return Err(ENOTCONN),
    };
    if let Some(a) = from { *a = Addr { ip: k.rip, port: k.rport }; }
    match crate::net::tcp_recv(h, out) {
        n if n >= 0 => Ok(n as u64),
        -2 => if fl & O_NONBLOCK != 0 { Err(EAGAIN) } else { Err(would_block_for(SOCK_KEY, NET_SLICE_MS)) },
        _ => Err(ECONNRESET),
    }
}

/// sendto/recvfrom pelo descritor (os enderecos ja decodificados).
pub fn sendto(fd: u64, src: u64, len: u64, to: Option<Addr>) -> Res<u64> {
    let l = leader()?;
    let _g = FS.guard();
    let f = get(l, fd)?;
    match f.kind {
        Kind::Sock => sock_send(f.flags, f.ino, src, len, to),
        Kind::Unix => unix::write(f.ino as usize >> 1, f.ino as usize & 1, src, len, f.flags & O_NONBLOCK != 0, &[]),
        _ => Err(ENOTSOCK),
    }
}

pub fn recvfrom(fd: u64, dst: u64, len: u64, from: &mut Option<Addr>, nonblock: bool) -> Res<u64> {
    let l = leader()?;
    let _g = FS.guard();
    let f = get(l, fd)?;
    let fl = f.flags | if nonblock { O_NONBLOCK } else { 0 };
    match f.kind {
        Kind::Sock => {
            let mut a = Addr { ip: [0; 4], port: 0 };
            let r = sock_recv(fl, f.ino, dst, len, Some(&mut a));
            if r.is_ok() { *from = Some(a); }
            r
        }
        Kind::Unix => {
            let r = unix_read(f, dst, len, nonblock).map(|(n, _, _)| n);
            let (local, remote) = unix::ports(f.ino as usize >> 1, f.ino as usize & 1);
            if local != 0 { *from = Some(Addr { ip: [127, 0, 0, 1], port: remote }); }
            r
        }
        _ => Err(ENOTSOCK),
    }
}

/// (local, remoto) de um socket INET. Sockets locais AF_UNIX dao Err(1)
/// (o chamador devolve so a familia).
pub fn sock_names(fd: u64) -> Res<(Addr, Addr)> {
    let l = leader()?;
    let _g = FS.guard();
    let f = get(l, fd)?;
    let lo = [127, 0, 0, 1];
    match f.kind {
        Kind::Unix => {
            let (local, remote) = unix::ports(f.ino as usize >> 1, f.ino as usize & 1);
            if local == 0 { return Err(1); }
            return Ok((Addr { ip: lo, port: local }, Addr { ip: lo, port: remote }));
        }
        Kind::Listen => return match unix::listener_key(f.ino as usize) {
            unix::Key::Port(p) => Ok((Addr { ip: lo, port: p }, Addr { ip: [0; 4], port: 0 })),
            _ => Err(1),
        },
        Kind::USock => return Err(1),
        _ => {}
    }
    let (_, i) = sock_of(l, fd)?;
    let k = socks()[i];
    let lport = match k.tcp { Some(h) => { let _n = crate::net::NET_LOCK.guard(); crate::net::tcp_local_port(h) } None => k.lport };
    Ok((Addr { ip: crate::net::OUR_IP, port: lport }, Addr { ip: k.rip, port: k.rport }))
}

/// getsockopt(SO_ERROR): erro pendente (e o limpa).
pub fn sock_error(fd: u64) -> Res<i64> {
    let l = leader()?;
    let _g = FS.guard();
    let (_, i) = sock_of(l, fd)?;
    let k = &mut socks()[i];
    { let _n = crate::net::NET_LOCK.guard(); sock_progress(k); }
    Ok(if k.state == SockState::Failed { core::mem::take(&mut k.err) } else { 0 })
}

pub fn is_socket(fd: u64) -> Res<bool> {
    let l = leader()?;
    let _g = FS.guard();
    Ok(matches!(get(l, fd)?.kind, Kind::Sock | Kind::Unix | Kind::USock | Kind::Listen))
}

pub fn is_unix(fd: u64) -> Res<bool> {
    let l = leader()?;
    let _g = FS.guard();
    Ok(get(l, fd)?.kind == Kind::Unix)
}

/// listen(): o socket vira um listener (AF_UNIX com bind, ou TCP local).
pub fn listen(fd: u64) -> Res<u64> {
    let l = leader()?;
    let _g = FS.guard();
    let f = get(l, fd)?;
    let li = match f.kind {
        Kind::Listen => return Ok(0),
        Kind::USock => {
            let u = usocks()[f.ino as usize];
            if u.key == unix::Key::None { return Err(EINVAL); }
            unix::listen(u.key, u.packet)?
        }
        Kind::Sock if !socks()[f.ino as usize].udp => {
            let k = &mut socks()[f.ino as usize];
            if k.lport == 0 { let _n = crate::net::NET_LOCK.guard(); k.lport = crate::net::udp_alloc_port(); }
            unix::listen(unix::Key::Port(k.lport), false)?
        }
        _ => return Err(EOPNOTSUPP),
    };
    table(l)[fd as usize] = OpenFile { kind: Kind::Listen, ino: li as u32, ..f };
    drop_ref(f);
    Ok(0)
}

// ------------------------------------------------------------ prontidao

pub const POLLIN: u32 = 0x001;
pub const POLLOUT: u32 = 0x004;
pub const POLLERR: u32 = 0x008;
pub const POLLHUP: u32 = 0x010;
pub const POLLNVAL: u32 = 0x020;

/// Eventos prontos de um descritor (FS presa). `net` vira true se a
/// resposta depende da rede (quem espera precisa reconferir de tempo em
/// tempo).
fn ready_locked(l: usize, fd: i64, net: &mut bool) -> u32 {
    let Ok(f) = get(l, fd as u64) else { return POLLNVAL };
    let pr = |i: u32| { let p = pipes()[i as usize]; (p.len > 0, p.writers == 0) };
    let pw = |i: u32| { let p = pipes()[i as usize]; (p.len < PIPE_CAP, p.readers == 0) };
    match f.kind {
        Kind::PipeR => { let (d, hup) = pr(f.ino); (if d || hup { POLLIN } else { 0 }) | if hup { POLLHUP } else { 0 } }
        Kind::PipeW => { let (sp, err) = pw(f.ino); if err { POLLERR } else if sp { POLLOUT } else { 0 } }
        Kind::Unix => {
            let (d, w, hup) = unix::ready(f.ino as usize >> 1, f.ino as usize & 1);
            (if d { POLLIN } else { 0 }) | (if w { POLLOUT } else { 0 }) | (if hup { POLLHUP } else { 0 })
        }
        Kind::Listen => if unix::listener_ready(f.ino as usize) { POLLIN } else { 0 },
        Kind::USock => POLLOUT,
        Kind::Event => {
            let e = events()[f.ino as usize];
            (if e.count > 0 { POLLIN } else { 0 }) | POLLOUT
        }
        Kind::Sock => {
            *net = true;
            let k = &mut socks()[f.ino as usize];
            let _n = crate::net::NET_LOCK.guard();
            if k.udp {
                return POLLOUT | if k.lport != 0 && crate::net::udp_pending(k.lport) { POLLIN } else { 0 };
            }
            sock_progress(k);
            match (k.state, k.tcp) {
                (SockState::Failed, _) => POLLERR | POLLHUP | POLLOUT,
                (SockState::Connected, Some(h)) => {
                    let (r, w, hup) = crate::net::tcp_ready(h);
                    (if r { POLLIN } else { 0 }) | (if w { POLLOUT } else { 0 }) | (if hup { POLLHUP } else { 0 })
                }
                (SockState::New, _) => POLLOUT | POLLHUP,
                _ => 0,
            }
        }
        Kind::Console => POLLOUT,
        Kind::Epoll => 0,
        Kind::Free => POLLNVAL,
        _ => POLLIN | POLLOUT,
    }
}

/// poll(): preenche `revents` de cada (fd, events). Retorna (quantos
/// prontos, depende da rede).
pub fn poll_fds(items: &mut [(i64, u32, u32)]) -> Res<(usize, bool)> {
    let l = leader()?;
    let _g = FS.guard();
    let mut net = false;
    let mut n = 0;
    for it in items.iter_mut() {
        if it.0 < 0 { it.2 = 0; continue; }
        let r = ready_locked(l, it.0, &mut net);
        it.2 = r & (it.1 | POLLERR | POLLHUP | POLLNVAL);
        if it.2 != 0 { n += 1; }
    }
    Ok((n, net))
}

// ------------------------------------------------------------ epoll

const EPOLL_MAX: usize = 128;
pub const EPOLLONESHOT: u32 = 1 << 30;

#[derive(Clone, Copy)]
struct EpItem { fd: i64, events: u32, data: u64, off: bool }
#[derive(Clone, Copy)]
struct Epoll { used: bool, refs: u32, n: usize, items: [EpItem; EPOLL_MAX] }
const EP_EMPTY: Epoll = Epoll { used: false, refs: 0, n: 0, items: [EpItem { fd: -1, events: 0, data: 0, off: false }; EPOLL_MAX] };
static mut EPOLLS: [Epoll; 16] = [EP_EMPTY; 16];
fn epolls() -> &'static mut [Epoll; 16] { unsafe { &mut *(&raw mut EPOLLS) } }

pub fn epoll_create(flags: u32) -> Res<u64> {
    let l = leader()?;
    let _g = FS.guard();
    let i = epolls().iter().position(|e| !e.used).ok_or(ENFILE)?;
    epolls()[i] = Epoll { used: true, refs: 1, ..EP_EMPTY };
    let f = OpenFile { kind: Kind::Epoll, ino: i as u32, flags: O_RDWR, cloexec: flags & O_CLOEXEC != 0, off: 0 };
    install(l, f, 0).inspect_err(|_| drop_ref(f))
}

/// epoll_ctl(ep, op, fd, events, data): ADD 1, DEL 2, MOD 3.
pub fn epoll_ctl(ep: u64, op: u64, fd: i64, events: u32, data: u64) -> Res<u64> {
    let l = leader()?;
    let _g = FS.guard();
    let e = get(l, ep)?;
    if e.kind != Kind::Epoll { return Err(EINVAL); }
    get(l, fd as u64)?;
    if fd as u64 == ep { return Err(EINVAL); }
    let ex = &mut epolls()[e.ino as usize];
    let pos = ex.items[..ex.n].iter().position(|it| it.fd == fd);
    match (op, pos) {
        (1, Some(_)) => Err(ext2::EEXIST),
        (1, None) => {
            if ex.n == EPOLL_MAX { return Err(ext2::ENOSPC); }
            ex.items[ex.n] = EpItem { fd, events, data, off: false };
            ex.n += 1;
            Ok(0)
        }
        (2, Some(p)) => { ex.items[p] = ex.items[ex.n - 1]; ex.n -= 1; Ok(0) }
        (3, Some(p)) => { ex.items[p] = EpItem { fd, events, data, off: false }; Ok(0) }
        (2 | 3, None) => Err(ext2::ENOENT),
        _ => Err(EINVAL),
    }
}

/// Varre o epoll e grava ate `max` eventos (struct epoll_event: u32
/// events, u64 data; 16 bytes no aarch64/riscv64). Retorna (quantos,
/// depende da rede).
pub fn epoll_scan(ep: u64, out: u64, max: usize) -> Res<(usize, bool)> {
    let l = leader()?;
    let _g = FS.guard();
    let e = get(l, ep)?;
    if e.kind != Kind::Epoll { return Err(EINVAL); }
    let mut net = false;
    let mut n = 0;
    let idx = e.ino as usize;
    for k in 0..epolls()[idx].n {
        if n >= max { break; }
        let it = epolls()[idx].items[k];
        if it.off { continue; }
        let r = ready_locked(l, it.fd, &mut net);
        let hit = r & (it.events | POLLERR | POLLHUP);
        if r & POLLNVAL != 0 || hit == 0 { continue; }
        unsafe {
            core::ptr::write_unaligned((out + n as u64 * 16) as *mut u32, hit);
            core::ptr::write_unaligned((out + n as u64 * 16 + 8) as *mut u64, it.data);
        }
        if it.events & EPOLLONESHOT != 0 { epolls()[idx].items[k].off = true; }
        n += 1;
    }
    Ok((n, net))
}
