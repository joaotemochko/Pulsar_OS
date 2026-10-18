//! Sinais POSIX para a personalidade Linux: tratadores, mascaras, pilha
//! alternativa e rt_sigreturn.
//!
//! O quadro empilhado para o tratador tem o layout do Linux (`rt_sigframe`:
//! siginfo + ucontext com `mcontext` e os registradores de FP), porque ha
//! programas que leem e alteram `uc_mcontext` dentro do tratador (motores
//! de JavaScript fazem isso para tratar falhas de pagina de proposito).
//!
//! Para voltar do tratador: no aarch64 a musl passa `sa_restorer`
//! (SA_RESTORER); no riscv64 o Linux usa um trampolim do vDSO, entao o
//! Pulsar mapeia em todo processo Linux uma pagina de codigo propria
//! (SIGPAGE_VA) que chama rt_sigreturn.
//!
//! As acoes sao por processo (slot do lider); pendentes, mascara e pilha
//! alternativa sao por thread (process::Process::sig). Tudo sob SCHED; a
//! escrita do quadro na memoria do usuario e feita sem trava nenhuma.
use crate::arch::{self, Context, FpState};
use crate::arch::mmu::{self, PageFlags};
use crate::process::{self, MAX_PROCS};
use crate::syscall::user_ok;

pub const SIGILL: u32 = 4;
pub const SIGTRAP: u32 = 5;
pub const SIGBUS: u32 = 7;
pub const SIGFPE: u32 = 8;
pub const SIGKILL: u32 = 9;
pub const SIGSEGV: u32 = 11;
pub const SIGPIPE: u32 = 13;
pub const SIGALRM: u32 = 14;
pub const SIGCHLD: u32 = 17;
pub const SIGSTOP: u32 = 19;
const UNBLOCKABLE: u64 = (1 << (SIGKILL - 1)) | (1 << (SIGSTOP - 1));

const SA_SIGINFO: u64 = 4;
const SA_ONSTACK: u64 = 0x0800_0000;
const SA_RESTORER: u64 = 0x0400_0000;
const SA_NODEFER: u64 = 0x4000_0000;
const SA_RESETHAND: u64 = 0x8000_0000;
const SS_ONSTACK: u64 = 1;
const SS_DISABLE: u64 = 2;

const SIG_DFL: u64 = 0;
const SIG_IGN: u64 = 1;

/// Endereco fixo da pagina de retorno de sinal (logo acima da pilha).
pub const SIGPAGE_VA: u64 = 0x3F_8000_0000;

/// Estado de sinais de uma thread.
#[derive(Clone, Copy)]
pub struct SigThread {
    pub pending: u64,
    pub mask: u64,
    pub saved: u64,         // mascara de antes do rt_sigsuspend
    pub suspended: bool,
    pub alt_sp: u64,        // sigaltstack (0 = nenhuma)
    pub alt_size: u64,
    pub fault_addr: u64,    // si_addr do ultimo SIGSEGV/SIGBUS sincrono
    pub fault_code: i32,    // si_code dele
    pub from_pid: u32,      // si_pid de quem mandou o ultimo sinal
}

impl SigThread {
    pub const fn new() -> Self {
        SigThread { pending: 0, mask: 0, saved: 0, suspended: false, alt_sp: 0, alt_size: 0,
                    fault_addr: 0, fault_code: 0, from_pid: 0 }
    }
}

/// `struct k_sigaction` (o que o rt_sigaction troca com a libc).
#[derive(Clone, Copy)]
#[repr(C)]
pub struct Action {
    pub handler: u64,
    pub flags: u64,
    pub restorer: u64,
    pub mask: u64,
}

const DFL: Action = Action { handler: SIG_DFL, flags: 0, restorer: 0, mask: 0 };
static mut ACTIONS: [[Action; 64]; MAX_PROCS] = [[DFL; 64]; MAX_PROCS];

fn actions(l: usize) -> &'static mut [Action; 64] {
    unsafe { &mut (*(&raw mut ACTIONS))[l] }
}

/// Processo novo: tudo no padrao. (SCHED presa)
pub fn reset_actions(slot: usize) {
    *actions(slot) = [DFL; 64];
}

/// fork: o filho herda as acoes. (SCHED presa)
pub fn copy_actions(from: usize, to: usize) {
    *actions(to) = *actions(from);
}

/// execve: tratadores voltam ao padrao; ignorados continuam ignorados.
pub fn exec_actions(slot: usize) {
    for a in actions(slot).iter_mut() {
        if a.handler != SIG_IGN { *a = DFL; }
    }
}

const SA_RESTART: u64 = 0x1000_0000;

/// O tratador de `sig` pede SA_RESTART? (SCHED presa)
pub fn restarts(l: usize, sig: u32) -> bool {
    actions(l)[sig as usize - 1].flags & SA_RESTART != 0
}

/// O sinal tem efeito no processo do lider `l` (nao e ignorado)? (SCHED presa)
pub fn would_act(l: usize, sig: u32) -> bool {
    if sig == SIGKILL { return true; }
    let a = actions(l)[sig as usize - 1];
    !(a.handler == SIG_IGN || (a.handler == SIG_DFL && default_ignored(sig)))
}

/// Sinais cujo padrao e ignorar (os de parada tambem: nao ha controle de
/// jobs).
fn default_ignored(sig: u32) -> bool {
    matches!(sig, 17 | 18 | 19 | 20 | 21 | 22 | 23 | 28)
}

// ------------------------------------------------------------ pagina de retorno

#[repr(C, align(4096))]
struct SigPage([u32; 1024]);

#[cfg(target_arch = "aarch64")]
static SIGPAGE: SigPage = SigPage({
    let mut c = [0u32; 1024];
    c[0] = 0xD280_1168; // mov x8, #139   (rt_sigreturn)
    c[1] = 0xD400_0001; // svc #0
    c
});
#[cfg(target_arch = "riscv64")]
static SIGPAGE: SigPage = SigPage({
    let mut c = [0u32; 1024];
    c[0] = 0x08B0_0893; // li a7, 139     (rt_sigreturn)
    c[1] = 0x0000_0073; // ecall
    c
});

/// Mapeia a pagina de retorno no espaco `root` (processos Linux).
pub fn map_sigpage(root: u64) {
    let pa = (&raw const SIGPAGE) as u64;
    unsafe { mmu::map_page_in(root, SIGPAGE_VA, pa, PageFlags::user_prot(1 | 4)) };
}

// ------------------------------------------------------------ quadro (layout Linux)

const SI_SIZE: u64 = 128;
const UC_SIGMASK: u64 = 40;
const UC_MCTX: u64 = 176;
#[cfg(target_arch = "aarch64")]
const MCTX_SIZE: u64 = 288 + 4096;
#[cfg(target_arch = "riscv64")]
const MCTX_SIZE: u64 = 256 + 528;
/// rt_sigframe (+ registro de quadro fp/lr no aarch64), multiplo de 16.
const FRAME_SIZE: u64 = (SI_SIZE + UC_MCTX + MCTX_SIZE + 16 + 15) & !15;

#[cfg(target_arch = "aarch64")]
const FPSIMD_MAGIC: u32 = 0x4650_8001;

unsafe fn w64(a: u64, v: u64) { unsafe { core::ptr::write_unaligned(a as *mut u64, v) } }
unsafe fn w32(a: u64, v: u32) { unsafe { core::ptr::write_unaligned(a as *mut u32, v) } }
unsafe fn r64(a: u64) -> u64 { unsafe { core::ptr::read_unaligned(a as *const u64) } }
unsafe fn r32(a: u64) -> u32 { unsafe { core::ptr::read_unaligned(a as *const u32) } }

/// Grava registradores + FP em `mc` (uc_mcontext).
unsafe fn save_mcontext(mc: u64, f: &Context, fp: &FpState, fault: u64) {
    unsafe {
        #[cfg(target_arch = "aarch64")]
        {
            w64(mc, fault);
            for i in 0..31 { w64(mc + 8 + i as u64 * 8, f.x[i]); }
            w64(mc + 256, f.sp_el0);
            w64(mc + 264, f.elr);
            w64(mc + 272, f.spsr);
            let r = mc + 288;
            core::ptr::write_bytes(r as *mut u8, 0, 4096);
            w32(r, FPSIMD_MAGIC);
            w32(r + 4, 528);
            w32(r + 8, fp.fpsr as u32);
            w32(r + 12, fp.fpcr as u32);
            for i in 0..32 { core::ptr::write_unaligned((r + 16 + i as u64 * 16) as *mut u128, fp.v[i]); }
            // terminador: cabecalho {0, 0} logo depois
        }
        #[cfg(target_arch = "riscv64")]
        {
            let _ = fault;
            w64(mc, f.sepc);
            for i in 1..32 { w64(mc + i as u64 * 8, f.x[i]); }
            let r = mc + 256;
            core::ptr::write_bytes(r as *mut u8, 0, 528);
            for i in 0..32 { w64(r + i as u64 * 8, fp.f[i]); }
            w32(r + 256, fp.fcsr as u32);
        }
    }
}

/// Le registradores + FP de `mc` (rt_sigreturn). So o que o usuario pode
/// mexer: o nivel de privilegio e as interrupcoes nao vem do quadro.
unsafe fn load_mcontext(mc: u64, f: &mut Context, fp: &mut FpState) {
    unsafe {
        #[cfg(target_arch = "aarch64")]
        {
            for i in 0..31 { f.x[i] = r64(mc + 8 + i as u64 * 8); }
            f.sp_el0 = r64(mc + 256);
            f.elr = r64(mc + 264);
            f.spsr = r64(mc + 272) & 0xF000_0000; // so NZCV; EL0t, interrupcoes ligadas
            let r = mc + 288;
            if r32(r) == FPSIMD_MAGIC {
                fp.fpsr = r32(r + 8) as u64;
                fp.fpcr = r32(r + 12) as u64;
                for i in 0..32 { fp.v[i] = core::ptr::read_unaligned((r + 16 + i as u64 * 16) as *const u128); }
            }
        }
        #[cfg(target_arch = "riscv64")]
        {
            f.sepc = r64(mc);
            for i in 1..32 { f.x[i] = r64(mc + i as u64 * 8); }
            let r = mc + 256;
            for i in 0..32 { fp.f[i] = r64(r + i as u64 * 8); }
            fp.fcsr = r32(r + 256) as u64;
        }
    }
}

// ------------------------------------------------------------ entrega

enum Plan {
    None,
    Fatal(u32),
    Handle { sig: u32, act: Action, old_mask: u64, alt: Option<(u64, u64)>, info: (i32, u64, u32), eintr: bool },
}

/// Fim de toda entrada no kernel: se a thread corrente (Linux) tem um sinal
/// entregavel, monta o quadro do tratador ou aplica a acao padrao.
pub fn deliver(frame: *mut Context) {
    for _ in 0..64 {
        if !process::signal_hint() { return; }
        let plan = process::with_current(|p, _s| {
            if p.abi != process::ABI_LINUX { p.sig.pending = 0; return Plan::None; }
            let ready = p.sig.pending & !(p.sig.mask & !UNBLOCKABLE);
            if ready == 0 { return Plan::None; }
            let sig = ready.trailing_zeros() + 1;
            p.sig.pending &= !(1u64 << (sig - 1));
            let a = actions(p.leader)[sig as usize - 1];
            if sig == SIGKILL || a.handler == SIG_DFL {
                return if default_ignored(sig) && sig != SIGKILL { Plan::None } else { Plan::Fatal(sig) };
            }
            if a.handler == SIG_IGN { return Plan::None; }
            let old_mask = if p.sig.suspended { p.sig.saved } else { p.sig.mask };
            p.sig.suspended = false;
            let mut m = old_mask | a.mask;
            if a.flags & SA_NODEFER == 0 { m |= 1u64 << (sig - 1); }
            p.sig.mask = m & !UNBLOCKABLE;
            if a.flags & SA_RESETHAND != 0 { actions(p.leader)[sig as usize - 1] = DFL; }
            let sp = unsafe { context_sp(&*frame) };
            let on_alt = p.sig.alt_sp != 0 && sp > p.sig.alt_sp && sp <= p.sig.alt_sp + p.sig.alt_size;
            let alt = if a.flags & SA_ONSTACK != 0 && p.sig.alt_sp != 0 && !on_alt {
                Some((p.sig.alt_sp, p.sig.alt_size))
            } else { None };
            let info = if matches!(sig, SIGSEGV | SIGBUS | SIGILL | SIGFPE | SIGTRAP) && p.sig.fault_code != 0 {
                (p.sig.fault_code, p.sig.fault_addr, 0)
            } else { (0, 0, p.sig.from_pid) };   // SI_USER
            if matches!(sig, SIGSEGV | SIGBUS | SIGILL | SIGFPE | SIGTRAP) { p.sig.fault_code = 0; }
            // parado no meio de um poll/pause (syscall recuada): o tratador
            // interrompe a chamada, que retorna EINTR em vez de recomecar
            let eintr = core::mem::take(&mut p.in_poll);
            if eintr { p.io_deadline = 0; }
            Plan::Handle { sig, act: a, old_mask, alt, info, eintr }
        });
        match plan {
            None | Some(Plan::None) => continue,
            Some(Plan::Fatal(sig)) => {
                process::set_status(sig, true);
                unsafe { process::exit_group(frame) };
                return;
            }
            Some(Plan::Handle { sig, act, old_mask, alt, info, eintr }) => {
                if eintr {
                    unsafe {
                        (*frame).skip_syscall();
                        (*frame).set_ret((-4i64) as u64);
                    }
                }
                if !unsafe { build_frame(frame, sig, &act, old_mask, alt, info) } {
                    // pilha invalida: nao ha como rodar o tratador
                    process::set_status(SIGSEGV, true);
                    unsafe { process::exit_group(frame) };
                }
                return;
            }
        }
    }
}

fn context_sp(f: &Context) -> u64 {
    #[cfg(target_arch = "aarch64")] { f.sp_el0 }
    #[cfg(target_arch = "riscv64")] { f.x[2] }
}

unsafe fn build_frame(frame: *mut Context, sig: u32, act: &Action, old_mask: u64,
                      alt: Option<(u64, u64)>, info: (i32, u64, u32)) -> bool {
    let f = unsafe { &mut *frame };
    let top = match alt { Some((sp, size)) => sp + size, None => context_sp(f) };
    let base = (top - FRAME_SIZE) & !15;
    if !user_ok(base, FRAME_SIZE, true) { return false; }
    let mut fp = FpState::zeroed();
    unsafe { arch::fp_save(&mut fp) };
    let si = base;
    let uc = base + SI_SIZE;
    unsafe {
        core::ptr::write_bytes(base as *mut u8, 0, FRAME_SIZE as usize);
        // siginfo: signo, errno, code; depois si_addr (falhas) ou si_pid
        w32(si, sig);
        w32(si + 8, info.0 as u32);
        if info.0 > 0 { w64(si + 16, info.1); } else { w32(si + 16, info.2); }
        // ucontext: uc_stack, uc_sigmask, uc_mcontext
        if let Some((sp, size)) = alt {
            w64(uc + 16, sp);
            w32(uc + 24, SS_ONSTACK as u32);
            w64(uc + 32, size);
        } else {
            w32(uc + 24, SS_DISABLE as u32);
        }
        w64(uc + UC_SIGMASK, old_mask);
        save_mcontext(uc + UC_MCTX, f, &fp, info.1);
    }
    let restorer = if cfg!(target_arch = "aarch64") && act.flags & SA_RESTORER != 0 { act.restorer } else { SIGPAGE_VA };
    #[cfg(target_arch = "aarch64")]
    {
        // registro de quadro (fp, lr) para quem percorre a pilha
        let rec = uc + UC_MCTX + MCTX_SIZE;
        unsafe { w64(rec, f.x[29]); w64(rec + 8, f.x[30]); }
        f.x[29] = rec;
        f.x[30] = restorer;
        f.sp_el0 = base;
        f.elr = act.handler;
        f.x[0] = sig as u64;
        f.x[1] = si;
        f.x[2] = uc;
        f.spsr &= !(1 << 21);   // sem single-step
    }
    #[cfg(target_arch = "riscv64")]
    {
        f.x[1] = restorer;
        f.x[2] = base;
        f.sepc = act.handler;
        f.x[10] = sig as u64;
        f.x[11] = si;
        f.x[12] = uc;
    }
    let _ = SA_SIGINFO; // x1/x2 vao sempre; tratadores simples os ignoram
    true
}

/// rt_sigreturn: restaura o contexto salvo no quadro em sp.
pub fn sigreturn(frame: *mut Context) {
    let f = unsafe { &mut *frame };
    let base = context_sp(f);
    if !user_ok(base, FRAME_SIZE - 16, false) {
        process::set_status(SIGSEGV, true);
        return unsafe { process::exit_group(frame) };
    }
    let uc = base + SI_SIZE;
    let mut fp = FpState::zeroed();
    unsafe {
        load_mcontext(uc + UC_MCTX, f, &mut fp);
        arch::fp_restore(&fp);
    }
    f.mark_fp_dirty();
    let mask = unsafe { r64(uc + UC_SIGMASK) } & !UNBLOCKABLE;
    process::with_current(|p, _| { p.sig.mask = mask; p.sig.suspended = false; });
}

// ------------------------------------------------------------ falhas sincronas

/// Falha do programa (Linux): se ha tratador para o sinal correspondente,
/// enfileira com o endereco e retorna true (a entrega acontece na volta).
pub fn fault_signal(sig: u32, code: i32, addr: u64) -> bool {
    process::with_current(|p, _| {
        if p.abi != process::ABI_LINUX { return false; }
        let a = actions(p.leader)[sig as usize - 1];
        let bit = 1u64 << (sig - 1);
        // bloqueado ou sem tratador: o processo morre (como no Linux)
        if a.handler == SIG_DFL || a.handler == SIG_IGN || p.sig.mask & bit != 0 { return false; }
        if p.sig.fault_code != 0 && p.sig.pending & bit != 0 { return false; } // falha dentro do tratador
        p.sig.pending |= bit;
        p.sig.fault_addr = addr;
        p.sig.fault_code = code;
        true
    }).unwrap_or(false)
}

// ------------------------------------------------------------ syscalls

const EINVAL: i64 = 22;
const EFAULT: i64 = 14;
const ESRCH: i64 = 3;
const ENOMEM: i64 = 12;

type R = Result<u64, i64>;

/// rt_sigaction(sig, act, oldact, sigsetsize)
pub fn sys_sigaction(sig: u64, act: u64, old: u64) -> R {
    if sig == 0 || sig > 64 { return Err(EINVAL); }
    let size = core::mem::size_of::<Action>() as u64;
    if act != 0 && !user_ok(act, size, false) { return Err(EFAULT); }
    if old != 0 && !user_ok(old, size, true) { return Err(EFAULT); }
    let new = if act != 0 { Some(unsafe { core::ptr::read_unaligned(act as *const Action) }) } else { None };
    if new.is_some() && (sig as u32 == SIGKILL || sig as u32 == SIGSTOP) { return Err(EINVAL); }
    let prev = process::with_current(|p, _| {
        let slot = &mut actions(p.leader)[sig as usize - 1];
        let prev = *slot;
        if let Some(mut n) = new {
            n.mask &= !UNBLOCKABLE;
            *slot = n;
            // ignorar descarta o que estava pendente
            if n.handler == SIG_IGN { p.sig.pending &= !(1u64 << (sig - 1)); }
        }
        prev
    }).ok_or(EINVAL)?;
    if old != 0 { unsafe { core::ptr::write_unaligned(old as *mut Action, prev) }; }
    Ok(0)
}

/// rt_sigprocmask(how, set, oldset)
pub fn sys_sigprocmask(how: u64, set: u64, old: u64) -> R {
    if set != 0 && !user_ok(set, 8, false) { return Err(EFAULT); }
    if old != 0 && !user_ok(old, 8, true) { return Err(EFAULT); }
    let v = if set != 0 { Some(unsafe { r64(set) }) } else { None };
    let prev = process::with_current(|p, _| {
        let prev = p.sig.mask;
        if let Some(v) = v {
            p.sig.mask = match how { 0 => prev | v, 1 => prev & !v, 2 => v, _ => return Err(EINVAL) } & !UNBLOCKABLE;
        }
        Ok(prev)
    }).ok_or(EINVAL)??;
    if old != 0 { unsafe { w64(old, prev) }; }
    Ok(0)
}

pub fn sys_sigpending(set: u64) -> R {
    if !user_ok(set, 8, true) { return Err(EFAULT); }
    let p = process::with_current(|p, _| p.sig.pending & p.sig.mask).unwrap_or(0);
    unsafe { w64(set, p) };
    Ok(0)
}

/// sigaltstack(ss, old): stack_t = { ss_sp, ss_flags (int), ss_size }.
pub fn sys_sigaltstack(ss: u64, old: u64, sp: u64) -> R {
    if ss != 0 && !user_ok(ss, 24, false) { return Err(EFAULT); }
    if old != 0 && !user_ok(old, 24, true) { return Err(EFAULT); }
    let new = if ss != 0 { Some(unsafe { (r64(ss), r32(ss + 8) as u64, r64(ss + 16)) }) } else { None };
    let (osp, osize) = process::with_current(|p, _| {
        let prev = (p.sig.alt_sp, p.sig.alt_size);
        if let Some((nsp, flags, size)) = new {
            if flags & SS_DISABLE != 0 { p.sig.alt_sp = 0; p.sig.alt_size = 0; }
            else if size < 2048 { return Err(ENOMEM); }
            else { p.sig.alt_sp = nsp; p.sig.alt_size = size; }
        }
        Ok(prev)
    }).ok_or(EINVAL)??;
    if old != 0 {
        let on = osp != 0 && sp > osp && sp <= osp + osize;
        let flags = if osp == 0 { SS_DISABLE } else if on { SS_ONSTACK } else { 0 };
        unsafe { w64(old, osp); w32(old + 8, flags as u32); w64(old + 16, osize); }
    }
    Ok(0)
}

/// kill/tkill/tgkill para outro processo ou thread. `thread` = alvo e uma
/// thread especifica. Sem tratador e com acao padrao fatal, mata direto.
pub fn send(frame: *mut Context, target: usize, sig: u32, thread: bool) -> R {
    let Some(t) = process::slot_of_pid(target) else { return Err(ESRCH) };
    let me = process::current_ids().1;
    // a acao e do processo dono da thread
    let leader_pid = process::leader_pid_of(t);
    let act = { let _ = leader_pid; action_of_slot(t) };
    let a = act[sig as usize - 1];
    let fatal = sig == SIGKILL || (a.handler == SIG_DFL && !default_ignored(sig));
    let linux = process::abi_of_slot(t) == process::ABI_LINUX;
    if a.handler == SIG_IGN || (a.handler == SIG_DFL && default_ignored(sig) && sig != SIGKILL) {
        return Ok(0);
    }
    if fatal && !(linux && a.handler > SIG_IGN) && !process::blocks(t, sig) {
        let ok = unsafe { process::kill_sig(frame, leader_pid, sig) };
        return if ok { Ok(0) } else { Err(ESRCH) };
    }
    if !process::post_signal(target, sig, thread, me) { return Err(ESRCH); }
    Ok(0)
}

fn action_of_slot(t: usize) -> [Action; 64] {
    *actions(process::leader_of_slot(t))
}
