//! Personalidade Linux: syscalls com os numeros e as estruturas do Linux
//! (tabela asm-generic, a mesma em aarch64 e riscv64), para binarios ELF
//! estaticos — em especial os ligados a musl sem nenhuma modificacao.
//!
//! Argumentos em x0..x5 (a0..a5), numero em x8 (a7), retorno em x0 (a0);
//! erros sao -errno. Cada chamada e traduzida para os subsistemas do
//! Pulsar (file.rs, vm.rs, process.rs). O que ainda nao existe responde
//! -ENOSYS e e anotado uma vez na serial.
//!
//! Sinais: nao ha tratadores. rt_sigaction/rt_sigprocmask aceitam e nao
//! fazem nada; kill/tgkill de um sinal != 0 encerra o alvo com aquele
//! sinal como status de wait().
use crate::arch::{self, Context};
use crate::file::{self, AT_FDCWD};
use crate::process;
use crate::syscall::user_ok;
use crate::uart::Uart;
use core::fmt::Write;

const EPERM: i64 = 1;
const ENOENT: i64 = 2;
const ESRCH: i64 = 3;
const EAGAIN: i64 = 11;
const ENOMEM: i64 = 12;
const EFAULT: i64 = 14;
const EEXIST: i64 = 17;
const EINVAL: i64 = 22;
const ENOTTY: i64 = 25;
const ERANGE: i64 = 34;
const ENAMETOOLONG: i64 = 36;
const ENOSYS: i64 = 38;
const EOPNOTSUPP: i64 = 95;
const EPIPE: i64 = 32;
const E2BIG: i64 = 7;
const ETIMEDOUT: i64 = 110;

type R = Result<u64, i64>;

#[inline]
fn ret(r: R) -> u64 {
    match r {
        Ok(v) => v,
        Err(e) => (-e) as u64,
    }
}

// ------------------------------------------------------------ memoria do usuario

fn ubuf(ptr: u64, len: u64, write: bool) -> Result<(), i64> {
    if len == 0 { return Ok(()); }
    if ptr == 0 || !user_ok(ptr, len, write) { Err(EFAULT) } else { Ok(()) }
}

fn put<T: Copy>(ptr: u64, v: T) -> R {
    ubuf(ptr, core::mem::size_of::<T>() as u64, true)?;
    unsafe { core::ptr::write_unaligned(ptr as *mut T, v) };
    Ok(0)
}

fn get<T: Copy>(ptr: u64) -> Result<T, i64> {
    ubuf(ptr, core::mem::size_of::<T>() as u64, false)?;
    Ok(unsafe { core::ptr::read_unaligned(ptr as *const T) })
}

/// Copia uma string C do usuario para `buf` (sem o NUL).
fn cstr<'a>(ptr: u64, buf: &'a mut [u8; crate::ext2::PATH_MAX]) -> Result<&'a [u8], i64> {
    if ptr == 0 { return Err(EFAULT); }
    let mut n = 0;
    while n < buf.len() {
        // valida pagina a pagina (a string pode terminar antes do fim dela)
        let page_end = ((ptr + n as u64) | 4095) + 1;
        let chunk = ((page_end - (ptr + n as u64)) as usize).min(buf.len() - n);
        if !user_ok(ptr + n as u64, chunk as u64, false) { return Err(EFAULT); }
        let src = unsafe { core::slice::from_raw_parts((ptr + n as u64) as *const u8, chunk) };
        if let Some(z) = src.iter().position(|&c| c == 0) {
            buf[n..n + z].copy_from_slice(&src[..z]);
            return Ok(&buf[..n + z]);
        }
        buf[n..n + chunk].copy_from_slice(src);
        n += chunk;
    }
    Err(ENAMETOOLONG)
}

// ------------------------------------------------------------ tempo

fn mono_ns() -> u64 {
    ((arch::now() as u128 * 1_000_000_000) / arch::timer_freq() as u128) as u64
}

/// Segundos Unix no boot (RTC menos o tempo monotonic), calculado uma vez.
static mut BOOT_EPOCH: u64 = 0;

fn real_ns() -> u64 {
    unsafe {
        if BOOT_EPOCH == 0 {
            BOOT_EPOCH = arch::rtc_seconds().saturating_sub(mono_ns() / 1_000_000_000);
        }
        BOOT_EPOCH * 1_000_000_000 + mono_ns()
    }
}

#[derive(Clone, Copy)]
#[repr(C)]
struct Timespec { sec: i64, nsec: i64 }

fn ts_of(ns: u64) -> Timespec { Timespec { sec: (ns / 1_000_000_000) as i64, nsec: (ns % 1_000_000_000) as i64 } }
fn ns_of(t: Timespec) -> Result<u64, i64> {
    if t.sec < 0 || !(0..1_000_000_000).contains(&t.nsec) { return Err(EINVAL); }
    Ok((t.sec as u64).saturating_mul(1_000_000_000).saturating_add(t.nsec as u64))
}

fn clock_ns(clk: u64) -> Result<u64, i64> {
    match clk {
        0 | 5 | 8 | 11 => Ok(real_ns()),              // REALTIME, *_COARSE, *_ALARM, TAI
        1 | 4 | 6 | 7 | 9 => Ok(mono_ns()),           // MONOTONIC, RAW, COARSE, BOOTTIME
        2 | 3 => Ok(mono_ns()),                       // CPU de processo/thread (aproximado)
        _ => Err(EINVAL),
    }
}

/// Dorme `ns` nanossegundos (granularidade de milissegundo).
unsafe fn sleep_ns(frame: *mut Context, ns: u64) {
    let ms = ns.div_ceil(1_000_000);
    if ms == 0 {
        unsafe { (*frame).set_ret(0) };
        return;
    }
    unsafe { process::sleep(frame, ms) };
}

// ------------------------------------------------------------ arquivos

/// Flags de open do Linux (por arquitetura) -> flags genericas do file.rs.
fn open_flags(f: u64) -> Result<u32, i64> {
    let f = f as u32;
    #[cfg(target_arch = "aarch64")]
    let (dir, nofollow) = (0o40000u32, 0o100000u32);
    #[cfg(target_arch = "riscv64")]
    let (dir, nofollow) = (0o200000u32, 0o400000u32);
    const O_PATH: u32 = 0o10000000;
    const O_TMPFILE_BIT: u32 = 0o20000000;
    if f & O_TMPFILE_BIT != 0 { return Err(EOPNOTSUPP); }
    let mut out = f & (file::O_ACCMODE | file::O_CREAT | file::O_EXCL | file::O_TRUNC | file::O_APPEND
        | file::O_NONBLOCK | file::O_CLOEXEC);
    if f & O_PATH != 0 { out &= !file::O_ACCMODE; }
    if f & dir != 0 { out |= file::O_DIRECTORY; }
    if f & nofollow != 0 { out |= file::O_NOFOLLOW; }
    Ok(out)
}

fn at_dir(dirfd: u64) -> Result<u32, i64> {
    file::dir_of(dirfd as i64)
}

const AT_SYMLINK_NOFOLLOW: u64 = 0x100;
const AT_REMOVEDIR: u64 = 0x200;
const AT_EMPTY_PATH: u64 = 0x1000;

fn fstatat(dirfd: u64, path: u64, statbuf: u64, flags: u64) -> R {
    let mut p = [0u8; crate::ext2::PATH_MAX];
    let path = cstr(path, &mut p)?;
    let st = if path.is_empty() {
        if flags & AT_EMPTY_PATH == 0 { return Err(ENOENT); }
        if dirfd as i64 == AT_FDCWD { file::stat(at_dir(dirfd)?, b".", true)? } else { file::fstat(dirfd)? }
    } else {
        file::stat(at_dir(dirfd)?, path, flags & AT_SYMLINK_NOFOLLOW == 0)?
    };
    put(statbuf, st)
}

#[derive(Clone, Copy)]
#[repr(C)]
struct Iovec { base: u64, len: u64 }

fn rw_vec(fd: u64, iov: u64, cnt: u64, write: bool, at: Option<u64>) -> R {
    if cnt > 1024 { return Err(EINVAL); }
    let mut total = 0u64;
    for i in 0..cnt {
        let v: Iovec = get(iov + i * 16)?;
        if v.len == 0 { continue; }
        ubuf(v.base, v.len, !write)?;
        let off = at.map(|o| o + total);
        let r = if write { file::write(fd, v.base, v.len, off) } else { file::read(fd, v.base, v.len, off) };
        let n = match r {
            Ok(n) => n,
            Err(e) if e == file::EBLOCK && total > 0 => break,   // ja leu/escreveu algo
            Err(e) => return Err(e),
        };
        total += n;
        if n < v.len { break; }
    }
    Ok(total)
}

// ------------------------------------------------------------ estruturas fixas

fn uname(buf: u64) -> R {
    let mut u = [0u8; 65 * 6];
    let fields: [&[u8]; 6] = [
        b"Linux", b"pulsar", b"6.1.0-pulsar", b"Pulsar OS (personalidade Linux)",
        if cfg!(target_arch = "aarch64") { b"aarch64" } else { b"riscv64" }, b"(none)",
    ];
    for (i, f) in fields.iter().enumerate() {
        u[i * 65..i * 65 + f.len()].copy_from_slice(f);
    }
    put(buf, u)
}

fn sysinfo(buf: u64) -> R {
    let mut b = [0u8; 112];
    let w64 = |b: &mut [u8; 112], o: usize, v: u64| b[o..o + 8].copy_from_slice(&v.to_le_bytes());
    w64(&mut b, 0, mono_ns() / 1_000_000_000);
    w64(&mut b, 32, crate::frame_allocator::total_count() as u64 * 4096);
    w64(&mut b, 40, crate::frame_allocator::free_count() as u64 * 4096);
    b[80..82].copy_from_slice(&(process::MAX_PROCS as u16).to_le_bytes());
    b[104..108].copy_from_slice(&1u32.to_le_bytes());
    put(buf, b)
}

fn rlimit(res: u64) -> (u64, u64) {
    const INF: u64 = u64::MAX;
    match res {
        3 => (crate::exec::STACK_SIZE, crate::exec::STACK_SIZE), // RLIMIT_STACK
        7 => (file::MAX_FDS as u64, file::MAX_FDS as u64),       // RLIMIT_NOFILE
        _ => (INF, INF),
    }
}

fn statfs(buf: u64) -> R {
    let [bs, blocks, free, inodes, ifree] = file::fsinfo()?;
    let mut v = [0u64; 15];
    v[0] = 0xEF53;          // f_type (ext2)
    v[1] = bs;
    v[2] = blocks;
    v[3] = free;
    v[4] = free;
    v[5] = inodes;
    v[6] = ifree;
    v[8] = 255;             // f_namelen
    v[9] = bs;              // f_frsize
    put(buf, v)
}

// ------------------------------------------------------------ despacho

pub fn dispatch(frame: *mut Context) {
    let nr = unsafe { (*frame).syscall_num() };
    dispatch_nr(frame, nr);
}

/// Atende a syscall `nr` (numeracao do Linux) com os argumentos do frame.
/// Tambem usada pela ABI nativa do Pulsar para os servicos que as duas
/// compartilham (sockets, poll, mmap de descritor...: ver syscall.rs).
pub fn dispatch_nr(frame: *mut Context, nr: u64) {
    let ctx = unsafe { &mut *frame };
    let a = [ctx.arg(0), ctx.arg(1), ctx.arg(2), ctx.arg(3), ctx.arg(4), ctx.arg(5)];
    // as que bloqueiam/trocam de processo gravam o retorno elas mesmas
    match nr {
        93 => unsafe {                                  // exit
            process::set_status(process::exit_status(a[0]), false);
            return process::exit_current(frame);
        },
        94 => unsafe {                                  // exit_group
            process::set_status(process::exit_status(a[0]), true);
            return process::exit_group(frame);
        },
        124 => unsafe { ctx.set_ret(0); return process::schedule(frame); }, // sched_yield
        101 => {                                        // nanosleep
            match get::<Timespec>(a[0]).and_then(ns_of) {
                Ok(ns) => unsafe { return sleep_ns(frame, ns) },
                Err(e) => { ctx.set_ret((-e) as u64); return; }
            }
        }
        115 => {                                        // clock_nanosleep(clk, flags, req, rem)
            let r = get::<Timespec>(a[2]).and_then(ns_of).and_then(|t| {
                if a[1] & 1 != 0 { clock_ns(a[0]).map(|now| t.saturating_sub(now)) } else { Ok(t) }
            });
            match r {
                Ok(ns) => unsafe { return sleep_ns(frame, ns) },
                Err(e) => { ctx.set_ret((-e) as u64); return; }
            }
        }
        98 => return futex(frame, a),
        260 => {                                        // wait4(pid, status, options, rusage)
            if a[1] != 0 && ubuf(a[1], 4, true).is_err() { ctx.set_ret((-EFAULT) as u64); return; }
            if a[3] != 0 { let _ = put(a[3], [0u8; 144]); }
            let pid = a[0] as i64;
            if pid < -1 || pid == 0 { ctx.set_ret((-ENOSYS) as u64); return; } // grupos de processo
            return unsafe { process::wait_child(frame, pid, a[1], a[2] & 1 != 0) };
        }
        129 | 130 | 131 => return kill(frame, nr, a),
        139 => return crate::signal::sigreturn(frame),       // rt_sigreturn: frame inteiro trocado
        133 => {                                        // rt_sigsuspend(mask, size)
            match get::<u64>(a[0]) {
                Ok(m) => return unsafe { process::sig_suspend(frame, m) },
                Err(e) => { ctx.set_ret((-e) as u64); return; }
            }
        }
        220 if a[0] & 0x10000 == 0 => return fork(frame, a),  // clone sem CLONE_THREAD
        73 | 72 | 22 | 441 => return wait_ready(frame, nr, a), // ppoll, pselect6, epoll_pwait(2)
        221 => {                                        // execve(path, argv, envp)
            match execve(frame, a) {
                Ok(()) => {}                            // frame ja e o da imagem nova
                Err(e) => ctx.set_ret((-e) as u64),
            }
            file::flush_wakes();
            return;
        }
        _ => {}
    }
    let r = call(frame, nr, a);
    if r == Err(file::EBLOCK) {
        // pipe vazia/cheia: dorme e refaz a syscall quando houver novidade
        let (key, seen, deadline) = file::take_block();
        file::flush_wakes();
        return unsafe { process::io_wait_until(frame, key, seen, deadline) };
    }
    // escrever numa pipe/socket sem leitor: SIGPIPE (padrao: termina), a
    // nao ser com MSG_NOSIGNAL (que chega marcado como -EPIPE)
    let r = if r == Err(-EPIPE) {
        Err(EPIPE)
    } else {
        if r == Err(EPIPE) && matches!(nr, 64 | 66 | 68 | 70 | 206 | 211) {
            let (tid, pid, _) = process::current_ids();
            process::post_signal(tid, crate::signal::SIGPIPE, true, pid);
        }
        r
    };
    unsafe { (*frame).set_ret(ret(r)) };
    file::flush_wakes();
}

/// fork() ou vfork() (clone sem CLONE_THREAD).
fn fork(frame: *mut Context, a: [u64; 6]) {
    const CLONE_VM: u64 = 0x100;
    const CLONE_VFORK: u64 = 0x4000;
    let (flags, stack) = (a[0], a[1]);
    let ctx = unsafe { &mut *frame };
    if flags & CLONE_VM != 0 {
        if flags & CLONE_VFORK == 0 { ctx.set_ret((-EINVAL) as u64); return; }
        // vfork: o filho usa o espaco do pai; o pai dorme ate o execve/exit
        if unsafe { process::fork(frame, None, stack) }.is_none() { ctx.set_ret((-EAGAIN) as u64); }
        return;
    }
    let Some((_, root, _)) = process::current_space() else { ctx.set_ret((-EINVAL) as u64); return };
    let Some(new) = arch::mmu::clone_user_space(root) else { ctx.set_ret((-ENOMEM) as u64); return };
    let asid = crate::loader::alloc_asid();
    match unsafe { process::fork(frame, Some((new, asid)), stack) } {
        Some(pid) => ctx.set_ret(pid as u64),
        None => { arch::mmu::free_user_space(new); ctx.set_ret((-EAGAIN) as u64); }
    }
}

/// Copia um vetor de strings do usuario (argv/envp) para `out`, separadas
/// por NUL. Retorna (bytes usados, quantas).
fn copy_strv(vec: u64, out: &mut [u8]) -> Result<(usize, usize), i64> {
    if vec == 0 { return Ok((0, 0)); }
    let mut used = 0;
    let mut n = 0;
    loop {
        let p: u64 = get(vec + n as u64 * 8)?;
        if p == 0 { return Ok((used, n)); }
        let mut tmp = [0u8; crate::ext2::PATH_MAX];
        let s = cstr(p, &mut tmp).map_err(|e| if e == ENAMETOOLONG { E2BIG } else { e })?;
        if used + s.len() + 1 > out.len() || n >= 512 { return Err(E2BIG); }
        out[used..used + s.len()].copy_from_slice(s);
        out[used + s.len()] = 0;
        used += s.len() + 1;
        n += 1;
    }
}

fn execve(frame: *mut Context, a: [u64; 6]) -> Result<(), i64> {
    let mut p = [0u8; crate::ext2::PATH_MAX];
    let path = cstr(a[0], &mut p)?;
    const MAXA: usize = 64 * 1024;
    let pages = MAXA / 4096;
    let buf = crate::frame_allocator::alloc_contig(pages).ok_or(ENOMEM)?;
    let block = unsafe { core::slice::from_raw_parts_mut(buf as *mut u8, MAXA) };
    let r = (|| {
        let (alen, argc) = copy_strv(a[1], &mut block[..MAXA / 2])?;
        let (elen, envc) = copy_strv(a[2], &mut block[MAXA / 2..])?;
        let args = crate::exec::Args {
            argv: &block[..alen], argc, envp: &block[MAXA / 2..MAXA / 2 + elen], envc,
        };
        crate::exec::execve(frame, at_dir(AT_FDCWD as u64)?, path, &args)
    })();
    crate::frame_allocator::free_contig(buf, pages);
    r
}

fn futex(frame: *mut Context, a: [u64; 6]) {
    let ctx = unsafe { &mut *frame };
    let (addr, op, val) = (a[0], a[1] & 0x7F, a[2]);
    if addr & 3 != 0 || ubuf(addr, 4, false).is_err() { ctx.set_ret((-EINVAL) as u64); return; }
    match op {
        0 | 9 => {                                      // WAIT / WAIT_BITSET
            let mut ms = 0;
            if a[3] != 0 {
                let t = match get::<Timespec>(a[3]).and_then(ns_of) {
                    Ok(t) => t,
                    Err(e) => { ctx.set_ret((-e) as u64); return; }
                };
                // WAIT: relativo; WAIT_BITSET: absoluto (MONOTONIC ou REALTIME)
                let rel = if op == 9 {
                    let now = if a[1] & 256 != 0 { real_ns() } else { mono_ns() };
                    t.saturating_sub(now)
                } else { t };
                if rel == 0 {
                    let cur = unsafe { core::ptr::read_volatile(addr as *const u32) };
                    ctx.set_ret((-(if cur == val as u32 { ETIMEDOUT } else { EAGAIN })) as u64);
                    return;
                }
                ms = rel.div_ceil(1_000_000);
            }
            unsafe { process::futex_wait(frame, addr, val as u32, ms) }
        }
        1 | 10 => ctx.set_ret(process::futex_wake(addr, val)), // WAKE / WAKE_BITSET
        3 | 4 => {                                      // REQUEUE / CMP_REQUEUE
            if op == 4 {
                let cur = unsafe { core::ptr::read_volatile(addr as *const u32) };
                if cur != a[5] as u32 { ctx.set_ret((-EAGAIN) as u64); return; }
            }
            // sem fila de espera movel: quem seria MOVIDO para uaddr2 (ate
            // val2 = a[3] threads) e acordado em uaddr (acordar a mais e
            // permitido; os chamadores reconferem). Acordar em uaddr2 nao
            // serve: os movidos ficariam dormindo em uaddr para sempre.
            let n = process::futex_wake(addr, val.saturating_add(a[3] & 0x7FFF_FFFF));
            ctx.set_ret(n);
        }
        _ => ctx.set_ret((-ENOSYS) as u64),
    }
}

fn kill(frame: *mut Context, nr: u64, a: [u64; 6]) {
    let ctx = unsafe { &mut *frame };
    // kill(pid, sig) | tkill(tid, sig) | tgkill(tgid, tid, sig)
    let (target, sig) = match nr { 131 => (a[1], a[2]), _ => (a[0], a[1]) };
    if sig > 64 { ctx.set_ret((-EINVAL) as u64); return; }
    let (tid, me, _) = process::current_ids();
    let target = if nr == 129 && target == 0 { me as u64 } else { target };  // grupo = o proprio
    if (target as i64) <= 0 { ctx.set_ret((-EPERM) as u64); return; }
    if sig == 0 {
        let alive = target as usize == me || process::is_own_thread(target as usize) || process::is_alive(target as usize);
        ctx.set_ret(if alive { 0 } else { (-ESRCH) as u64 });
        return;
    }
    let _ = tid;
    ctx.set_ret(0);
    match crate::signal::send(frame, target as usize, sig as u32, nr != 129) {
        Ok(_) => {}
        Err(e) => unsafe { (*frame).set_ret((-e) as u64) },
    }
}

fn context_sp(f: &Context) -> u64 {
    #[cfg(target_arch = "aarch64")] { f.sp_el0 }
    #[cfg(target_arch = "riscv64")] { f.x[2] }
}

fn call(frame: *mut Context, nr: u64, a: [u64; 6]) -> R {
    let mut p1 = [0u8; crate::ext2::PATH_MAX];
    match nr {
        // ---- arquivos
        56 => {                                         // openat(dirfd, path, flags, mode)
            let path = cstr(a[1], &mut p1)?;
            file::open(at_dir(a[0])?, path, open_flags(a[2])?, a[3] as u16)
        }
        57 => file::close(a[0]),
        63 => { ubuf(a[1], a[2], true)?; file::read(a[0], a[1], a[2], None) }
        64 => { ubuf(a[1], a[2], false)?; file::write(a[0], a[1], a[2], None) }
        65 => rw_vec(a[0], a[1], a[2], false, None),    // readv
        66 => rw_vec(a[0], a[1], a[2], true, None),     // writev
        67 => { ubuf(a[1], a[2], true)?; file::read(a[0], a[1], a[2], Some(a[3])) }   // pread64
        68 => { ubuf(a[1], a[2], false)?; file::write(a[0], a[1], a[2], Some(a[3])) } // pwrite64
        69 => rw_vec(a[0], a[1], a[2], false, Some(a[3])), // preadv
        70 => rw_vec(a[0], a[1], a[2], true, Some(a[3])),  // pwritev
        62 => file::lseek(a[0], a[1] as i64, a[2]),
        80 => put(a[1], file::fstat(a[0])?),
        79 => fstatat(a[0], a[1], a[2], a[3]),          // newfstatat
        291 => Err(ENOSYS),                             // statx: a musl cai no fstatat
        61 => { ubuf(a[1], a[2], true)?; file::getdents(a[0], a[1], a[2]) }
        34 => file::mkdir(at_dir(a[0])?, cstr(a[1], &mut p1)?, a[2] as u16),
        35 => file::unlink(at_dir(a[0])?, cstr(a[1], &mut p1)?, a[2] & AT_REMOVEDIR != 0),
        38 | 276 => {                                   // renameat / renameat2
            let flags = if nr == 276 { a[4] } else { 0 };
            if flags & !1 != 0 { return Err(EINVAL); }
            let mut p2 = [0u8; crate::ext2::PATH_MAX];
            let (fat, tat) = (at_dir(a[0])?, at_dir(a[2])?);
            let from = cstr(a[1], &mut p1)?;
            let to = cstr(a[3], &mut p2)?;
            if flags & 1 != 0 && file::stat(tat, to, false).is_ok() { return Err(EEXIST); }
            file::rename(fat, from, tat, to)
        }
        36 => {                                         // symlinkat(target, dirfd, path)
            let mut p2 = [0u8; crate::ext2::PATH_MAX];
            let target = cstr(a[0], &mut p1)?;
            file::symlink(target, at_dir(a[1])?, cstr(a[2], &mut p2)?)
        }
        78 => {                                         // readlinkat(dirfd, path, buf, size)
            let path = cstr(a[1], &mut p1)?;
            ubuf(a[2], a[3], true)?;
            file::readlink(at_dir(a[0])?, path, a[2], a[3])
        }
        37 => Err(EPERM),                               // linkat: sem hard links ainda
        48 | 439 => {                                   // faccessat / faccessat2
            let path = cstr(a[1], &mut p1)?;
            let follow = !(nr == 439 && a[3] & AT_SYMLINK_NOFOLLOW != 0);
            file::stat(at_dir(a[0])?, path, follow).map(|_| 0)
        }
        46 => file::ftruncate(a[0], a[1]),
        47 => file::fallocate(a[0], a[1], a[2], a[3]),
        45 => {                                         // truncate(path, len)
            let fd = file::open(at_dir(AT_FDCWD as u64)?, cstr(a[0], &mut p1)?, file::O_WRONLY, 0)?;
            let r = file::ftruncate(fd, a[1]);
            let _ = file::close(fd);
            r
        }
        82 | 83 | 81 => Ok(0),                         // fsync, fdatasync, sync (escrita imediata)
        223 => Ok(0),                                   // fadvise64: so uma dica
        23 => file::dup(a[0], u64::MAX),
        24 => {                                         // dup3(old, new, flags)
            if a[0] == a[1] { return Err(EINVAL); }
            if a[1] >= file::MAX_FDS as u64 { return Err(9); } // EBADF (negativo viraria "minimo")
            file::dup_ex(a[0], a[1], a[2] as u32 & file::O_CLOEXEC != 0)
        }
        59 => {                                         // pipe2(fds, flags)
            ubuf(a[0], 8, true)?;
            let (rd, wr) = file::pipe(a[1] as u32 & (file::O_CLOEXEC | file::O_NONBLOCK))?;
            put(a[0], [rd as i32, wr as i32])
        }
        25 => fcntl(a[0], a[1], a[2]),
        29 => ioctl(a[0], a[1], a[2]),
        17 => {                                         // getcwd(buf, size)
            let mut out = [0u8; crate::ext2::PATH_MAX];
            let n = file::getcwd(&mut out)?;
            if (n + 1) as u64 > a[1] { return Err(ERANGE); }
            ubuf(a[0], n as u64 + 1, true)?;
            unsafe {
                core::ptr::copy_nonoverlapping(out.as_ptr(), a[0] as *mut u8, n);
                *((a[0] + n as u64) as *mut u8) = 0;
            }
            Ok(n as u64 + 1)
        }
        49 => file::chdir(at_dir(AT_FDCWD as u64)?, cstr(a[0], &mut p1)?),
        50 => {                                         // fchdir
            let ino = file::dir_of(a[0] as i64)?;
            let l = process::current_leader().ok_or(EINVAL)?;
            file::set_cwd_of(l, ino);
            Ok(0)
        }
        52 | 53 | 54 | 55 | 88 => Ok(0),               // chmod/chown/utimensat: sem permissoes ainda
        166 => Ok(0o022),                               // umask
        43 => { cstr(a[0], &mut p1)?; statfs(a[1]) }    // statfs
        44 => statfs(a[1]),                             // fstatfs

        // ---- eventos e sockets
        19 => file::eventfd(a[0] & 0xFFFF_FFFF, a[1] as u32),        // eventfd2
        20 => file::epoll_create(a[0] as u32),                       // epoll_create1
        21 => {                                                      // epoll_ctl(ep, op, fd, ev)
            let (ev, data) = if a[1] != 2 { let e: [u32; 4] = get(a[3])?; (e[0], e[2] as u64 | (e[3] as u64) << 32) } else { (0, 0) };
            file::epoll_ctl(a[0], a[1], a[2] as i64, ev, data)
        }
        198 => file::socket(a[0], a[1]),
        199 => {                                                     // socketpair(d, t, p, sv)
            ubuf(a[3], 8, true)?;
            let (x, y) = file::socketpair(a[0], a[1])?;
            put(a[3], [x as i32, y as i32])
        }
        200 => file::bind(a[0], any_sockaddr(a[1], a[2])?),
        201 => file::listen(a[0]),
        202 | 242 => {                                               // accept / accept4(fd, addr, len, flags)
            let flags = if nr == 242 { a[3] as u32 } else { 0 };
            let (nfd, peer) = file::accept(a[0], flags & (file::O_NONBLOCK | file::O_CLOEXEC))?;
            if a[1] != 0 {
                match peer {
                    Some(port) => { put_sockaddr(a[1], a[2], file::Addr { ip: [127, 0, 0, 1], port })?; }
                    None => { put_unix_family(a[1], a[2])?; }
                }
            }
            Ok(nfd)
        }
        203 => file::connect(a[0], any_sockaddr(a[1], a[2])?),
        204 | 205 => {                                               // getsockname / getpeername
            match file::sock_names(a[0]) {
                Ok((local, remote)) => put_sockaddr(a[1], a[2], if nr == 204 { local } else { remote }),
                Err(1) => put_unix_family(a[1], a[2]),
                Err(e) => Err(e),
            }
        }
        206 => {                                                     // sendto(fd, buf, len, flags, addr, alen)
            ubuf(a[1], a[2], false)?;
            let to = if a[4] != 0 { Some(sockaddr(a[4], a[5])?) } else { None };
            nosig(file::sendto(a[0], a[1], a[2], to), a[3])
        }
        207 => {                                                     // recvfrom(fd, buf, len, flags, addr, alen*)
            ubuf(a[1], a[2], true)?;
            let mut from = None;
            let n = file::recvfrom(a[0], a[1], a[2], &mut from, a[3] & 0x40 != 0)?;
            if a[4] != 0 { if let Some(f) = from { put_sockaddr(a[4], a[5], f)?; } }
            Ok(n)
        }
        211 => msg_io(a[0], a[1], true, a[2]),                       // sendmsg
        212 => msg_io(a[0], a[1], false, a[2]),                      // recvmsg
        208 => { file::is_socket(a[0])?; Ok(0) }                     // setsockopt: aceita e ignora
        209 => {                                                     // getsockopt(fd, level, opt, val, len*)
            let v: i32 = match (a[1], a[2]) {
                (1, 4) => file::sock_error(a[0])? as i32,            // SO_ERROR
                (1, 3) => 1,                                         // SO_TYPE (aprox.)
                (1, 7) | (1, 8) => 65536,                            // SO_SNDBUF / SO_RCVBUF
                _ => { file::is_socket(a[0])?; 0 }
            };
            put(a[3], v)?;
            put(a[4], 4u32)
        }
        210 => file::shutdown(a[0], a[1]),
        279 => file::memfd(a[1] & 1 != 0),                           // memfd_create(name, flags)

        // ---- memoria
        214 => Ok(process::brk(a[0])),
        222 => mmap(a),
        215 => neg(crate::vm::munmap(a[0], a[1])),
        226 => neg(crate::vm::mprotect(a[0], a[1], a[2] & 7)),
        233 => match a[2] {                             // madvise
            4 | 8 => neg(crate::vm::discard(a[0], a[1])), // DONTNEED, FREE
            _ => Ok(0),
        },
        216 => Err(ENOMEM),                             // mremap: a libc copia
        227 => Ok(0),                                   // msync

        // ---- processos e threads
        220 => clone(frame, a),
        96 => Ok(process::set_clear_tid(a[0]) as u64),  // set_tid_address
        99 => Ok(0),                                    // set_robust_list
        172 => Ok(process::current_ids().1 as u64),     // getpid
        173 => Ok(process::current_ids().2 as u64),     // getppid
        178 => Ok(process::current_ids().0 as u64),     // gettid
        174 | 175 | 176 | 177 => Ok(0),                 // uid/gid: root
        154 | 155 | 157 => Ok(process::current_ids().1 as u64), // setpgid/getpgid/setsid
        167 => Ok(0),                                   // prctl
        123 => {                                        // sched_getaffinity(pid, size, mask)
            if a[1] < 8 { return Err(EINVAL); }
            let n = process::online_cpus();
            put(a[2], if n >= 64 { u64::MAX } else { (1u64 << n) - 1 })?;
            Ok(8)
        }
        122 => Ok(0),                                   // sched_setaffinity
        134 => crate::signal::sys_sigaction(a[0], a[1], a[2]),
        135 => crate::signal::sys_sigprocmask(a[0], a[1], a[2]),
        136 => crate::signal::sys_sigpending(a[0]),
        132 => crate::signal::sys_sigaltstack(a[0], a[1], unsafe { context_sp(&*frame) }),
        261 => {                                        // prlimit64(pid, res, new, old)
            if a[3] != 0 { put(a[3], rlimit(a[1]))?; }
            Ok(0)
        }
        163 => put(a[1], rlimit(a[0])),                 // getrlimit
        103 | 102 => {                                 // setitimer(which, new, old) / getitimer(which, cur)
            if a[0] != 0 { return Err(EINVAL); }       // so ITIMER_REAL
            let tv_ns = |p: u64| -> Result<(u64, u64), i64> {
                let v: [i64; 4] = get(p)?;              // interval {sec, usec}, value {sec, usec}
                let ns = |s: i64, us: i64| (s.max(0) as u64) * 1_000_000_000 + (us.max(0) as u64) * 1000;
                Ok((ns(v[2], v[3]), ns(v[0], v[1])))
            };
            let (val, every, old_ptr) = if nr == 103 {
                let (v, e) = if a[1] != 0 { tv_ns(a[1])? } else { (0, 0) };
                (Some(v), e, a[2])
            } else { (None, 0, a[1]) };
            let (rem, ev) = match val {
                Some(v) => process::set_alarm(v, every),
                None => { let o = process::set_alarm(0, 0); process::set_alarm(o.0, o.1); o }
            };
            if old_ptr != 0 {
                let tv = |ns: u64| [(ns / 1_000_000_000) as i64, ((ns / 1000) % 1_000_000) as i64];
                let (i, v) = (tv(ev), tv(rem));
                put(old_ptr, [i[0], i[1], v[0], v[1]])?;
            }
            Ok(0)
        }
        164 => Ok(0),                                   // setrlimit
        165 => put(a[1], [0u8; 144]),                   // getrusage

        // ---- tempo e sistema
        113 => put(a[1], ts_of(clock_ns(a[0])?)),       // clock_gettime
        114 => { if a[1] != 0 { put(a[1], Timespec { sec: 0, nsec: 1_000_000 })?; } clock_ns(a[0]).map(|_| 0) }
        169 => {                                        // gettimeofday(tv, tz)
            let ns = real_ns();
            if a[0] != 0 { put(a[0], [(ns / 1_000_000_000) as i64, ((ns / 1000) % 1_000_000) as i64])?; }
            Ok(0)
        }
        160 => uname(a[0]),
        179 => sysinfo(a[0]),
        278 => {                                        // getrandom(buf, len, flags)
            ubuf(a[0], a[1], true)?;
            let mut done = 0;
            while done < a[1] {
                let n = (a[1] - done).min(256) as usize;
                let mut tmp = [0u8; 256];
                crate::rng::fill(&mut tmp[..n]);
                unsafe { core::ptr::copy_nonoverlapping(tmp.as_ptr(), (a[0] + done) as *mut u8, n) };
                done += n as u64;
            }
            Ok(a[1])
        }
        // ---- extensoes do Pulsar para processos da camada de compatibilidade
        #[cfg(target_arch = "riscv64")]
        259 => { crate::loader::sync_icache(); Ok(0) }               // riscv_flush_icache (todas as harts)
        0x5000 => Ok(crate::syscall::debug_oops(a[0])),             // pulsar_debug_oops(tipo)
        0x5001 => {                                                  // pulsar_lookup_service(id): pid ou -ESRCH
            let pid = crate::syscall::lookup_service(a[0]);
            if pid < 0 { Err(ESRCH) } else { Ok(pid as u64) }
        }
        _ => {
            note_missing(nr);
            Err(ENOSYS)
        }
    }
}

fn neg(v: i64) -> R {
    if v < 0 { Err(-v) } else { Ok(v as u64) }
}

fn fcntl(fd: u64, cmd: u64, arg: u64) -> R {
    match cmd {
        // F_DUPFD(_CLOEXEC): menor descritor livre >= arg. dup_ex recebe o
        // minimo nos 32 bits altos; minimo 0 e u64::MAX (0 << 32 seria dup2 no 0)
        0 | 1030 => {
            let min = arg.min(file::MAX_FDS as u64);
            file::dup_ex(fd, if min == 0 { u64::MAX } else { min << 32 }, cmd == 1030)
        }
        1 => Ok(file::get_cloexec(fd)? as u64),         // F_GETFD
        2 => file::set_cloexec(fd, arg & 1 != 0),       // F_SETFD
        3 => Ok((file::get_flags(fd)? & (file::O_ACCMODE | file::O_APPEND | file::O_NONBLOCK)) as u64), // F_GETFL
        4 => file::set_flags(fd, arg as u32),           // F_SETFL
        5 | 6 | 7 | 36 | 37 | 38 => Ok(0),              // travas de registro: sempre concedidas
        _ => Err(EINVAL),
    }
}

fn ioctl(fd: u64, req: u64, arg: u64) -> R {
    if !file::is_console(fd)? { return Err(ENOTTY); }
    match req {
        0x5413 => put(arg, [24u16, 80, 0, 0]),          // TIOCGWINSZ
        0x5401 => put(arg, [0u8; 60]),                  // TCGETS
        0x5402 | 0x5403 | 0x5404 | 0x5414 => Ok(0),     // TCSETS*, TIOCSWINSZ
        0x540F => put(arg, process::current_ids().1 as u32), // TIOCGPGRP
        _ => Err(ENOTTY),
    }
}

fn mmap(a: [u64; 6]) -> R {
    const MAP_FIXED: u64 = 0x10;
    const MAP_ANON: u64 = 0x20;
    const MAP_FIXED_NOREPLACE: u64 = 0x100000;
    let (addr, len, prot, flags, fd, off) = (a[0], a[1], a[2] & 7, a[3], a[4], a[5]);
    if off & 4095 != 0 { return Err(EINVAL); }
    let mut pf = 0;
    if flags & (MAP_FIXED | MAP_FIXED_NOREPLACE) != 0 { pf |= crate::vm::MAP_FIXED; }
    if flags & MAP_FIXED_NOREPLACE != 0 { pf |= crate::vm::MAP_NOREPLACE; }
    const MAP_SHARED: u64 = 0x01;
    if flags & MAP_ANON != 0 {
        if flags & 0x0F == MAP_SHARED {
            // memoria anonima compartilhada (sobrevive ao fork como a mesma)
            let obj = crate::shm::create_anon((len + 4095) & !4095).ok_or(ENOMEM)?;
            let r = crate::vm::mmap_obj(addr, len, prot, pf, obj, 0);
            if r < 0 { crate::shm::release_unused(obj); }
            return neg(r);
        }
        return neg(crate::vm::mmap(addr, len, prot, pf));
    }
    if flags & 0x0F == MAP_SHARED {
        if let Some(obj) = file::fd_obj(fd)? {
            return neg(crate::vm::mmap_obj(addr, len, prot, pf, obj, off / 4096));
        }
    }
    // arquivo: copia privada (MAP_SHARED de arquivo ainda nao reflete escritas)
    let start = neg(crate::vm::mmap(addr, len, prot | 3, pf))?;
    let fill = (|| -> R {
        ubuf(start, len, true)?;
        file::read(fd, start, len, Some(off))
    })();
    if let Err(e) = fill {
        crate::vm::munmap(start, len);
        return Err(e);
    }
    if prot & 2 == 0 || prot & 1 == 0 { crate::vm::mprotect(start, len, prot); }
    Ok(start)
}

fn clone(frame: *mut Context, a: [u64; 6]) -> R {
    const CLONE_VM: u64 = 0x100;
    const CLONE_THREAD: u64 = 0x10000;
    const CLONE_SETTLS: u64 = 0x80000;
    const CLONE_PARENT_SETTID: u64 = 0x100000;
    const CLONE_CHILD_CLEARTID: u64 = 0x200000;
    const CLONE_CHILD_SETTID: u64 = 0x1000000;
    // aarch64/riscv64: clone(flags, stack, ptid, tls, ctid)
    let (flags, stack, ptid, tls, ctid) = (a[0], a[1], a[2], a[3], a[4]);
    if flags & (CLONE_VM | CLONE_THREAD) != (CLONE_VM | CLONE_THREAD) {
        note_missing(220);
        return Err(ENOSYS);                             // fork/vfork: ainda nao
    }
    if flags & CLONE_PARENT_SETTID != 0 { ubuf(ptid, 4, true)?; }
    if flags & (CLONE_CHILD_SETTID | CLONE_CHILD_CLEARTID) != 0 { ubuf(ctid, 4, true)?; }
    let tls = if flags & CLONE_SETTLS != 0 { Some(tls) } else { None };
    let clear = if flags & CLONE_CHILD_CLEARTID != 0 { ctid } else { 0 };
    let tid = unsafe { process::clone_thread(frame, stack, tls, clear) }.ok_or(EAGAIN)?;
    if flags & CLONE_PARENT_SETTID != 0 { unsafe { core::ptr::write_volatile(ptid as *mut u32, tid as u32) }; }
    if flags & CLONE_CHILD_SETTID != 0 { unsafe { core::ptr::write_volatile(ctid as *mut u32, tid as u32) }; }
    Ok(tid as u64)
}

/// Anota (uma vez por numero) syscalls que ainda nao existem.
fn note_missing(nr: u64) {
    static mut SEEN: [u64; 8] = [0; 8];
    let (w, b) = ((nr / 64) as usize, nr % 64);
    if w >= 8 { return; }
    unsafe {
        if SEEN[w] & (1 << b) != 0 { return; }
        SEEN[w] |= 1 << b;
    }
    let _ = write!(Uart, "[linux] syscall {} nao implementada ({})\n", nr, process::current_name());
}

// ------------------------------------------------------------ sockets: enderecos

/// sockaddr_in ou sockaddr_un (caminho ou nome abstrato, que comeca com 0).
fn any_sockaddr(ptr: u64, len: u64) -> Result<file::SockAddr, i64> {
    if len < 2 { return Err(EINVAL); }
    let fam: u16 = get(ptr)?;
    if fam != 1 { return sockaddr(ptr, len).map(file::SockAddr::Inet); }
    let n = (len as usize - 2).min(108);
    let mut b = [0u8; 108];
    if n == 0 { return Err(EINVAL); }
    ubuf(ptr + 2, n as u64, false)?;
    unsafe { core::ptr::copy_nonoverlapping((ptr + 2) as *const u8, b.as_mut_ptr(), n) };
    if b[0] == 0 {
        let mut a = [0u8; 108];
        a[..n - 1].copy_from_slice(&b[1..n]);
        return Ok(file::SockAddr::Abstract(a, n - 1));
    }
    let plen = b[..n].iter().position(|&c| c == 0).unwrap_or(n);
    Ok(file::SockAddr::Path(b, plen))
}

/// sockaddr_un so com a familia (sockets locais sem nome visivel).
fn put_unix_family(ptr: u64, lenp: u64) -> R {
    if ptr == 0 { return Ok(0); }
    let cap: u32 = get(lenp)?;
    if cap >= 2 { put(ptr, 1u16)?; }
    put(lenp, 2u32)
}

/// sockaddr_in -> Addr (so AF_INET).
fn sockaddr(ptr: u64, len: u64) -> Result<file::Addr, i64> {
    if len < 8 { return Err(EINVAL); }
    let b: [u8; 8] = get(ptr)?;
    let fam = u16::from_le_bytes([b[0], b[1]]);
    if fam != 2 { return Err(97); }                           // EAFNOSUPPORT
    Ok(file::Addr { port: u16::from_be_bytes([b[2], b[3]]), ip: [b[4], b[5], b[6], b[7]] })
}

fn put_sockaddr(ptr: u64, lenp: u64, a: file::Addr) -> R {
    if ptr == 0 { return Ok(0); }
    let cap: u32 = get(lenp)?;
    let mut b = [0u8; 16];
    b[0] = 2;
    b[2..4].copy_from_slice(&a.port.to_be_bytes());
    b[4..8].copy_from_slice(&a.ip);
    let n = (cap as usize).min(16);
    ubuf(ptr, n as u64, true)?;
    unsafe { core::ptr::copy_nonoverlapping(b.as_ptr(), ptr as *mut u8, n) };
    put(lenp, 16u32)
}

/// MSG_NOSIGNAL: o EPIPE nao vira SIGPIPE.
fn nosig(r: R, flags: u64) -> R {
    match r {
        Err(EPIPE) if flags & 0x4000 != 0 => Err(-EPIPE),       // marcado; desfeito em dispatch
        other => other,
    }
}

/// sendmsg/recvmsg sem dados auxiliares (msghdr: name, namelen, iov,
/// iovlen, control, controllen, flags).
fn msg_io(fd: u64, msg: u64, send: bool, flags: u64) -> R {
    if file::is_unix(fd)? { return unix_msg(fd, msg, send, flags); }
    let m: [u64; 7] = get(msg)?;
    let (name, namelen, iov, iovlen) = (m[0], m[1] & 0xFFFF_FFFF, m[2], m[3]);
    if iovlen > 1024 { return Err(EINVAL); }
    let mut total = 0u64;
    let mut from = None;
    for i in 0..iovlen {
        let v: Iovec = get(iov + i * 16)?;
        if v.len == 0 { continue; }
        ubuf(v.base, v.len, !send)?;
        let r = if send {
            let to = if name != 0 && total == 0 { Some(sockaddr(name, namelen)?) } else { None };
            file::sendto(fd, v.base, v.len, to)
        } else {
            file::recvfrom(fd, v.base, v.len, &mut from, flags & 0x40 != 0 || total > 0)
        };
        let n = match r {
            Ok(n) => n,
            Err(e) if total > 0 && (e == file::EBLOCK || e == EAGAIN) => break,
            Err(e) => return nosig(Err(e), flags),
        };
        total += n;
        if n < v.len { break; }
    }
    if !send {
        if let (Some(f), true) = (from, name != 0) { put_sockaddr(name, msg + 8, f)?; }
        put(msg + 40, 0u64)?;                                   // controllen = 0
        put(msg + 48, 0u32)?;                                   // flags
    }
    Ok(total)
}

// ------------------------------------------------------------ poll / select / epoll

/// ppoll, pselect6 e epoll_pwait(2): varrem os descritores; sem nada
/// pronto, dormem ate um evento de E/S (ou 10 ms, se ha socket na
/// lista) e refazem a syscall, mantendo o prazo original.
fn wait_ready(frame: *mut Context, nr: u64, a: [u64; 6]) {
    let ctx = unsafe { &mut *frame };
    let seen = process::io_seq(process::POLL_KEY);
    let tick = |ns: u64| ((ns as u128 * arch::timer_freq() as u128) / 1_000_000_000) as u64;
    // prazo pedido (u64::MAX = sem prazo)
    let timeout_ns: Result<Option<u64>, i64> = match nr {
        73 => if a[2] == 0 { Ok(None) } else { get::<Timespec>(a[2]).and_then(ns_of).map(Some) },
        72 => if a[4] == 0 { Ok(None) } else { get::<Timespec>(a[4]).and_then(ns_of).map(Some) },
        22 => Ok(if (a[3] as i32) < 0 { None } else { Some(a[3] as u32 as u64 * 1_000_000) }),
        _ => if a[4] == 0 { Ok(None) } else { get::<Timespec>(a[4]).and_then(ns_of).map(Some) },
    };
    let timeout_ns = match timeout_ns { Ok(t) => t, Err(e) => { ctx.set_ret((-e) as u64); return; } };
    let now = arch::now();
    let deadline = process::io_deadline(match timeout_ns { None => u64::MAX, Some(ns) => now + tick(ns).max(1) });
    let scan = match nr {
        73 => scan_poll(a[0], a[1]),
        72 => scan_select(a[0], a[1], a[2], a[3]),
        _ => {
            if a[2] == 0 || a[2] > 1024 { Err(EINVAL) }
            else { ubuf(a[1], a[2] * 16, true).and_then(|_| file::epoll_scan(a[0], a[1], a[2] as usize)) }
        }
    };
    match scan {
        Err(e) => { process::io_deadline_clear(); ctx.set_ret((-e) as u64); }
        Ok((n, _)) if n > 0 => { process::io_deadline_clear(); ctx.set_ret(n as u64); }
        Ok(_) if deadline != u64::MAX && arch::now() >= deadline => {
            process::io_deadline_clear();
            if nr == 72 { let _ = clear_sets(a[0], a[1], a[2], a[3]); }
            ctx.set_ret(0);
        }
        Ok((_, net)) => {
            let slice = if net { arch::now() + crate::time::ms_to_ticks(file::NET_SLICE_MS) } else { u64::MAX };
            let wake = deadline.min(slice);
            file::flush_wakes();
            unsafe { process::io_wait_until(frame, process::POLL_KEY, seen, if wake == u64::MAX { 0 } else { wake }) };
        }
    }
}

/// struct pollfd { int fd; short events; short revents; }
fn scan_poll(fds: u64, nfds: u64) -> Result<(usize, bool), i64> {
    if nfds > 1024 { return Err(EINVAL); }
    if nfds == 0 { return Ok((0, false)); }
    ubuf(fds, nfds * 8, true)?;
    let mut items = [(0i64, 0u32, 0u32); 1024];
    for i in 0..nfds as usize {
        let e: [u8; 8] = get(fds + i as u64 * 8)?;
        items[i] = (i32::from_le_bytes([e[0], e[1], e[2], e[3]]) as i64, u16::from_le_bytes([e[4], e[5]]) as u32, 0);
    }
    let r = file::poll_fds(&mut items[..nfds as usize])?;
    for i in 0..nfds as usize {
        unsafe { core::ptr::write_unaligned((fds + i as u64 * 8 + 6) as *mut u16, items[i].2 as u16) };
    }
    Ok(r)
}

/// fd_set de select: bits em palavras de 64.
fn scan_select(n: u64, rd: u64, wr: u64, ex: u64) -> Result<(usize, bool), i64> {
    if n > 1024 { return Err(EINVAL); }
    let words = n.div_ceil(64) as usize;
    let load = |p: u64| -> Result<[u64; 16], i64> {
        let mut s = [0u64; 16];
        if p != 0 { ubuf(p, words as u64 * 8, true)?; for w in 0..words { s[w] = unsafe { r64(p + w as u64 * 8) }; } }
        Ok(s)
    };
    let (r, w, x) = (load(rd)?, load(wr)?, load(ex)?);
    let mut items = [(0i64, 0u32, 0u32); 1024];
    let mut k = 0;
    for fd in 0..n as usize {
        let bit = 1u64 << (fd % 64);
        let mut ev = 0;
        if r[fd / 64] & bit != 0 { ev |= file::POLLIN; }
        if w[fd / 64] & bit != 0 { ev |= file::POLLOUT; }
        if x[fd / 64] & bit != 0 { ev |= 0x002; }
        if ev != 0 { items[k] = (fd as i64, ev, 0); k += 1; }
    }
    let (_, net) = file::poll_fds(&mut items[..k])?;
    let (mut or, mut ow, mut ox) = ([0u64; 16], [0u64; 16], [0u64; 16]);
    let mut count = 0;
    for &(fd, ev, re) in &items[..k] {
        if re & file::POLLNVAL != 0 { return Err(9); }       // EBADF
        let (wd, bit) = (fd as usize / 64, 1u64 << (fd % 64));
        if ev & file::POLLIN != 0 && re & (file::POLLIN | file::POLLHUP | file::POLLERR) != 0 { or[wd] |= bit; count += 1; }
        if ev & file::POLLOUT != 0 && re & (file::POLLOUT | file::POLLERR) != 0 { ow[wd] |= bit; count += 1; }
        if ev & 0x002 != 0 && re & 0x002 != 0 { ox[wd] |= bit; count += 1; }
    }
    if count > 0 {
        for (p, s) in [(rd, or), (wr, ow), (ex, ox)] {
            if p != 0 { for wi in 0..words { unsafe { w64(p + wi as u64 * 8, s[wi]) }; } }
        }
    }
    Ok((count, net))
}

fn clear_sets(n: u64, rd: u64, wr: u64, ex: u64) -> R {
    let words = n.div_ceil(64).min(16);
    for p in [rd, wr, ex] {
        if p != 0 { ubuf(p, words * 8, true)?; for wi in 0..words { unsafe { w64(p + wi * 8, 0) }; } }
    }
    Ok(0)
}

unsafe fn r64(a: u64) -> u64 { unsafe { core::ptr::read_unaligned(a as *const u64) } }
unsafe fn w64(a: u64, v: u64) { unsafe { core::ptr::write_unaligned(a as *mut u64, v) } }

// ------------------------------------------------------------ sendmsg/recvmsg locais

/// sendmsg/recvmsg em socket local: iovecs juntados num buffer (uma
/// mensagem so, no modo pacote) e descritores em SCM_RIGHTS.
fn unix_msg(fd: u64, msg: u64, send: bool, flags: u64) -> R {
    let m: [u64; 7] = get(msg)?;
    let (iov, iovlen, ctl, ctllen) = (m[2], m[3], m[4], m[5]);
    if iovlen > 1024 { return Err(EINVAL); }
    const BOUNCE: u64 = 64 * 1024;
    let buf = crate::frame_allocator::alloc_contig(16).ok_or(ENOMEM)?;
    let r = (|| -> R {
        let nonblock = flags & 0x40 != 0;
        if send {
            // junta os dados
            let mut total = 0u64;
            for i in 0..iovlen {
                let v: Iovec = get(iov + i * 16)?;
                let n = v.len.min(BOUNCE - total);
                if n == 0 { continue; }
                ubuf(v.base, n, false)?;
                unsafe { core::ptr::copy_nonoverlapping(v.base as *const u8, (buf + total) as *mut u8, n as usize) };
                total += n;
            }
            // SCM_RIGHTS: cmsghdr { u64 len; i32 level; i32 type; int fds[] }
            let mut fds = [0i32; crate::unix::MAX_FDS];
            let mut nfds = 0;
            let mut off = 0u64;
            while ctl != 0 && off + 16 <= ctllen {
                let h: [u32; 4] = get(ctl + off)?;
                let clen = h[0] as u64 | (h[1] as u64) << 32;
                if clen < 16 { break; }
                if h[2] == 1 && h[3] == 1 {
                    let n = ((clen - 16) / 4) as usize;
                    for k in 0..n {
                        if nfds == fds.len() { return Err(EINVAL); }
                        fds[nfds] = get(ctl + off + 16 + k as u64 * 4)?;
                        nfds += 1;
                    }
                }
                off += (clen + 7) & !7;
            }
            nosig(file::unix_send(fd, buf, total, &fds[..nfds], nonblock), flags)
        } else {
            let mut cap = 0u64;
            for i in 0..iovlen { let v: Iovec = get(iov + i * 16)?; cap += v.len; }
            let mut fds = [0i32; crate::unix::MAX_FDS];
            let room = if ctl != 0 && ctllen >= 16 { ((ctllen - 16) / 4) as usize } else { 0 };
            let (n, nfds, trunc) = file::unix_recv(fd, buf, cap.min(BOUNCE), nonblock, flags & 0x4000_0000 != 0,
                                                   &mut fds[..room.min(crate::unix::MAX_FDS)])?;
            // espalha nos iovecs
            let mut done = 0u64;
            for i in 0..iovlen {
                if done == n { break; }
                let v: Iovec = get(iov + i * 16)?;
                let k = v.len.min(n - done);
                ubuf(v.base, k, true)?;
                unsafe { core::ptr::copy_nonoverlapping((buf + done) as *const u8, v.base as *mut u8, k as usize) };
                done += k;
            }
            let mut oflags = if trunc { 0x20u32 } else { 0 };    // MSG_TRUNC
            if nfds > 0 {
                let clen = 16 + nfds as u64 * 4;
                ubuf(ctl, (clen + 7) & !7, true)?;
                put(ctl, clen)?;
                put(ctl + 8, [1u32, 1u32])?;
                for k in 0..nfds { put(ctl + 16 + k as u64 * 4, fds[k])?; }
                put(msg + 40, (clen + 7) & !7)?;
            } else {
                put(msg + 40, 0u64)?;
            }
            if room < nfds { oflags |= 0x8; }                   // MSG_CTRUNC
            put(msg + 48, oflags)?;
            Ok(n)
        }
    })();
    crate::frame_allocator::free_contig(buf, 16);
    r
}
