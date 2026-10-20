//! Processos, escalonador SMP e IPC sincrono (rendezvous estilo L4/QNX).
//!
//! Todo estado aqui e protegido pela trava SCHED (ver sync.rs): as funcoes
//! publicas a adquirem; as auxiliares privadas supoem que ela ja esta
//! presa. Disco, GPU, rede e memoria tem travas proprias, entao uma CPU
//! lendo o disco nao impede outra de escalonar ou trocar mensagens. Cada CPU tem o seu
//! "processo corrente" (ou IDLE). Um processo Running esta em exatamente
//! uma CPU; seu contexto salvo em PROCS so e valido quando ele NAO esta
//! Running. Quando nao ha nada pronto, a CPU roda o laco ocioso (wfi) e e
//! acordada pelo timer ou por um IPI quando algum processo fica pronto.
//!
//! PIDs sao unicos e crescentes: pid = slot + MAX_PROCS * geracao, o que
//! evita confundir um processo novo com um morto que usava o mesmo slot.
use crate::arch::{self, platform::MAX_CPUS, Context, FpState};
use crate::sync::SpinLock;
use crate::time;
use core::sync::atomic::{AtomicBool, Ordering};

/// Trava do escalonador, da tabela de processos e do IPC.
pub static SCHED: SpinLock = SpinLock::new();

#[derive(Clone, Copy, PartialEq)]
pub enum State {
    Unused,
    Ready,
    Running,
    SendBlocked,  // esperando o receptor aceitar a mensagem
    RecvBlocked,  // esperando alguem enviar
    ReplyBlocked, // enviou, esperando a resposta
    Sleeping,     // SYS_SLEEP ate `wake_at`
    FutexWait,    // FUTEX_WAIT em `futex_addr` (ate `wake_at`, se != 0)
    WaitChild,    // WAIT por um filho (`wait_pid`, ou -1 = qualquer um)
    VforkWait,    // pai de um vfork: espera o filho fazer execve ou sair
    IoWait,       // esperando E/S em `io_key` (a syscall e reexecutada ao acordar)
    SigWait,      // rt_sigsuspend: dorme ate chegar um sinal
    Zombie,       // lider que saiu, mas com threads ainda vivas (guarda o espaco)
}

/// Mensagem de IPC: tag + 6 palavras (56 bytes), sem alocacao dinamica.
#[derive(Clone, Copy)]
pub struct Message {
    pub tag: u64,
    pub data: [u64; 6],
}

impl Message {
    pub const fn zero() -> Self {
        Message { tag: 0, data: [0; 6] }
    }
}

pub const NAME_LEN: usize = 24;

#[derive(Clone, Copy)]
pub struct Process {
    pub pid: usize,
    pub state: State,
    pub ctx: Context,
    pub fp: FpState,     // registradores de FP (validos quando nao esta Running)
    pub root: u64,
    pub asid: u16,
    pub inbox: Message,
    pub reply: Message,
    pub peer: usize,     // slot com quem esta em rendezvous
    pub send_to: usize,  // slot destino de um send bloqueado
    pub msg_ptr: u64,    // VA do buffer de mensagem da syscall
    pub wake_at: u64,    // instante (contador) de acordar, se Sleeping
    pub cpu: usize,      // ultima CPU em que rodou
    pub cpu_time: u64,   // tempo de CPU acumulado (contador)
    pub killed: bool,    // SYS_KILL pendente (alvo rodando em outra CPU)
    pub brk: u64,        // fim atual do heap (SYS_SBRK; vale o do lider)
    pub leader: usize,   // slot do processo (identidade: pid, descritores, cwd, status)
    pub mm: usize,       // slot dono do espaco de enderecamento (tabelas, VMAs, heap)
    pub members: usize,  // no dono do espaco: slots vivos que o usam (threads, filho de vfork)
    pub vfork_parent: usize, // slot do pai suspenso por vfork (IDLE = nenhum)
    pub io_key: u64,     // em IoWait: o objeto esperado
    pub io_mask: u64,    // em IoWait (POLL_KEY): mascara de buckets monitorados
    pub futex_addr: u64, // VA esperada em FutexWait
    pub abi: u8,         // ABI_PULSAR ou ABI_LINUX (numeros de syscall)
    pub parent: usize,   // pid de quem criou o processo (usize::MAX: o kernel)
    pub status: u32,     // status de termino estilo wait() (no lider)
    pub wait_pid: i64,   // em WaitChild: pid esperado, ou -1
    pub wait_ptr: u64,   // em WaitChild: onde gravar o status (0 = nao grava)
    pub clear_tid: u64,  // set_tid_address: zera e acorda futex ao sair (threads Linux)
    pub alarm_at: u64,   // setitimer(ITIMER_REAL): proximo SIGALRM (contador; 0 = desligado)
    pub io_deadline: u64,// prazo de uma espera refeita (poll/connect...): sobrevive ao refazer
    pub in_poll: bool,   // entre reexecucoes de poll/select/epoll/pause (sinal -> EINTR)
    pub alarm_every: u64,// intervalo de repeticao (contador)
    pub sig: crate::signal::SigThread, // sinais pendentes/bloqueados desta thread
    pub name: [u8; NAME_LEN],
}

impl Process {
    pub const fn empty() -> Self {
        Process {
            pid: 0, state: State::Unused, ctx: Context::zeroed(), fp: FpState::zeroed(), root: 0, asid: 0,
            inbox: Message::zero(), reply: Message::zero(), peer: 0, send_to: 0,
            msg_ptr: 0, wake_at: 0, cpu: 0, cpu_time: 0, killed: false, brk: HEAP_BASE,
            leader: 0, mm: 0, members: 0, vfork_parent: IDLE, io_key: 0, io_mask: 0, futex_addr: 0, abi: ABI_PULSAR, parent: usize::MAX,
            status: 0, wait_pid: 0, wait_ptr: 0, clear_tid: 0, io_deadline: 0, in_poll: false, alarm_at: 0, alarm_every: 0, sig: crate::signal::SigThread::new(),
            name: [0; NAME_LEN],
        }
    }
}

pub const MAX_PROCS: usize = 256;  // processos + threads (o Firefox sozinho usa ~100)

/// Conjunto de syscalls do processo: apps .pulse usam a do Pulsar; binarios
/// ELF usam os numeros do Linux (asm-generic, iguais em aarch64 e riscv64).
pub const ABI_PULSAR: u8 = 0;
pub const ABI_LINUX: u8 = 1;

/// Status de wait(): saida normal (codigo << 8) ou morte por sinal (numero).
pub const SIGKILL: u32 = 9;
pub fn exit_status(code: u64) -> u32 { ((code & 0xFF) as u32) << 8 }

/// Heap de usuario (SYS_SBRK): cresce de HEAP_BASE ate HEAP_LIMIT. Fica num
/// VA alto (64 GiB), longe do codigo e da regiao grafica, com ate 4 GiB.
pub const HEAP_BASE: u64 = 0x10_0000_0000;
pub const HEAP_LIMIT: u64 = HEAP_BASE + 0x1_0000_0000;
pub const IDLE: usize = usize::MAX;

static mut PROCS: [Process; MAX_PROCS] = [Process::empty(); MAX_PROCS];
static mut GENERATION: [usize; MAX_PROCS] = [0; MAX_PROCS];
static mut CURRENT: [usize; MAX_CPUS] = [IDLE; MAX_CPUS];
/// ABI do processo corrente de cada CPU (lida em toda syscall, sem trava).
static mut CUR_ABI: [u8; MAX_CPUS] = [ABI_PULSAR; MAX_CPUS];

/// Processos terminados que ainda nao foram colhidos por wait():
/// (pid, pid do pai, status). Os mais antigos sao descartados.
const EXITS_MAX: usize = 64;
static mut EXITS: [(usize, usize, u32); EXITS_MAX] = [(usize::MAX, 0, 0); EXITS_MAX];
static mut EXITS_NEXT: usize = 0;
static mut RR_NEXT: usize = 0;

// Contabilidade de carga por CPU (janelas de 1 s).
static mut LAST_SWITCH: [u64; MAX_CPUS] = [0; MAX_CPUS];
static mut BUSY: [u64; MAX_CPUS] = [0; MAX_CPUS];
static mut IDLE_T: [u64; MAX_CPUS] = [0; MAX_CPUS];
static mut WINDOW_START: [u64; MAX_CPUS] = [0; MAX_CPUS];
static mut LOAD_PCT: [u32; MAX_CPUS] = [0; MAX_CPUS];

/// CPUs que ja entraram no escalonador.
pub static ONLINE: [AtomicBool; MAX_CPUS] = [const { AtomicBool::new(false) }; MAX_CPUS];

/// Ultimo processo que morreu por fault (para o WM avisar o usuario).
static mut LAST_CRASH: usize = usize::MAX;

#[inline]
fn cpu() -> usize {
    arch::cpu_id()
}

#[inline]
fn procs() -> &'static mut [Process; MAX_PROCS] {
    unsafe { &mut *(&raw mut PROCS) }
}

#[inline]
fn cur_slot() -> usize {
    unsafe { CURRENT[cpu()] }
}

/// pid -> slot, se o processo existe.
fn slot_of(pid: usize) -> Option<usize> {
    let s = pid % MAX_PROCS;
    let p = &procs()[s];
    if p.state != State::Unused && p.state != State::Zombie && p.pid == pid { Some(s) } else { None }
}

pub fn is_alive(pid: usize) -> bool {
    let _g = SCHED.guard();
    slot_of(pid).is_some()
}

// ------------------------------------------------------------- criacao

pub fn create(entry: u64, stack_top: u64, root: u64, asid: u16, name: &str) -> Option<usize> {
    create_ex(entry, stack_top, root, asid, name, ABI_PULSAR, &mut |_| {})
}

/// Cria um processo. `setup(slot)` roda antes de ele ficar pronto (com SCHED
/// presa: pode registrar VMAs, nunca pegar SCHED de novo). O pai e o
/// processo corrente.
pub fn create_ex(entry: u64, stack_top: u64, root: u64, asid: u16, name: &str, abi: u8,
                 setup: &mut dyn FnMut(usize)) -> Option<usize> {
    let _g = SCHED.guard();
    let parent = { let c = cur_slot(); if c == IDLE { usize::MAX } else { procs()[procs()[c].leader].pid } };
    let ps = procs();
    for i in 0..MAX_PROCS {
        if ps[i].state == State::Unused {
            let generation = unsafe { GENERATION[i] };
            unsafe { GENERATION[i] += 1; }
            ps[i] = Process::empty();
            ps[i].pid = i + MAX_PROCS * generation;
            ps[i].ctx = Context::new_user(entry, stack_top);
            ps[i].root = root;
            ps[i].asid = asid;
            ps[i].leader = i;
            ps[i].mm = i;
            ps[i].members = 1;
            ps[i].abi = abi;
            ps[i].parent = parent;
            let n = name.len().min(NAME_LEN - 1);
            ps[i].name[..n].copy_from_slice(&name.as_bytes()[..n]);
            crate::signal::reset_actions(i);
            setup(i);
            ps[i].state = State::Ready;
            kick_idle_cpu();
            return Some(ps[i].pid);
        }
    }
    None
}

/// SYS_THREAD_CREATE: nova thread do processo corrente, no mesmo espaco de
/// enderecamento (e heap), comecando em `entry` com a pilha `stack_top`
/// (alocada pelo proprio processo) e `arg` no primeiro registrador.
pub fn thread_create(entry: u64, stack_top: u64, arg: u64) -> Option<usize> {
    let _g = SCHED.guard();
    let s = cur_slot();
    if s == IDLE { return None; }
    let ps = procs();
    let l = ps[s].leader;
    let m = ps[s].mm;
    for i in 0..MAX_PROCS {
        if ps[i].state == State::Unused {
            let generation = unsafe { GENERATION[i] };
            unsafe { GENERATION[i] += 1; }
            let mut t = Process::empty();
            t.pid = i + MAX_PROCS * generation;
            t.ctx = Context::new_user(entry, stack_top);
            t.ctx.set_ret(arg);
            t.root = ps[m].root;
            t.asid = ps[m].asid;
            t.leader = l;
            t.mm = m;
            t.abi = ps[l].abi;
            t.parent = ps[l].parent;
            t.name = ps[l].name;
            t.state = State::Ready;
            ps[i] = t;
            ps[m].members += 1;
            kick_idle_cpu();
            return Some(t.pid);
        }
    }
    None
}

/// clone() de thread (ABI Linux): copia o frame do chamador (a thread
/// nova volta da mesma syscall com 0), com pilha `stack`, TLS `tls` e
/// `clear_tid` (CLONE_CHILD_CLEARTID). Retorna o tid.
pub unsafe fn clone_thread(frame: *mut Context, stack: u64, tls: Option<u64>, clear_tid: u64) -> Option<usize> {
    let _g = SCHED.guard();
    let s = cur_slot();
    if s == IDLE { return None; }
    let ps = procs();
    let l = ps[s].leader;
    let m = ps[s].mm;
    for i in 0..MAX_PROCS {
        if ps[i].state == State::Unused {
            let generation = unsafe { GENERATION[i] };
            unsafe { GENERATION[i] += 1; }
            let mut t = Process::empty();
            t.pid = i + MAX_PROCS * generation;
            t.ctx = unsafe { *frame };
            t.ctx.set_ret(0);
            if stack != 0 { t.ctx.set_sp(stack); }
            if let Some(v) = tls { t.ctx.set_tls(v); }
            // os registradores de FP vivos sao os do chamador
            unsafe { arch::fp_save(&mut t.fp) };
            t.root = ps[m].root;
            t.asid = ps[m].asid;
            t.leader = l;
            t.mm = m;
            t.abi = ps[l].abi;
            t.parent = ps[l].parent;
            t.name = ps[l].name;
            t.clear_tid = clear_tid;
            t.sig.mask = ps[s].sig.mask;
            t.state = State::Ready;
            ps[i] = t;
            ps[m].members += 1;
            kick_idle_cpu();
            return Some(t.pid);
        }
    }
    None
}

// ------------------------------------------------------------- consultas

/// (tid, pid, ppid) do corrente: tid = pid da thread, pid = do lider.
#[inline]
pub fn current_ids() -> (usize, usize, usize) {
    let s = cur_slot();
    if s == IDLE { return (0, 0, 0); }
    let l = procs()[s].leader;
    let ppid = procs()[l].parent;
    (procs()[s].pid, procs()[l].pid, if ppid == usize::MAX { 0 } else { ppid })
}

/// set_tid_address: guarda o endereco a zerar na saida; retorna o tid.
pub fn set_clear_tid(addr: u64) -> usize {
    let _g = SCHED.guard();
    let s = cur_slot();
    if s == IDLE { return 0; }
    procs()[s].clear_tid = addr;
    procs()[s].pid
}

/// pid -> e uma thread (ou o lider) do processo corrente?
pub fn is_own_thread(tid: usize) -> bool {
    let _g = SCHED.guard();
    let s = cur_slot();
    match slot_of(tid) {
        Some(t) => s != IDLE && procs()[t].leader == procs()[s].leader,
        None => false,
    }
}

/// ABI do processo corrente desta CPU.
#[inline]
pub fn current_abi() -> u8 {
    unsafe { CUR_ABI[cpu()] }
}

pub fn current_pid() -> usize {
    let s = cur_slot();
    if s == IDLE { usize::MAX } else { procs()[s].pid }
}

/// (slot dono, raiz, ASID) do espaco de enderecamento corrente.
#[inline]
pub fn current_space() -> Option<(usize, u64, u16)> {
    let s = cur_slot();
    if s == IDLE { return None; }
    Some((procs()[s].mm, procs()[s].root, procs()[s].asid))
}

/// Slot do processo corrente (identidade: descritores, cwd).
#[inline]
pub fn current_leader() -> Option<usize> {
    let s = cur_slot();
    if s == IDLE { None } else { Some(procs()[s].leader) }
}

pub fn current_root() -> u64 {
    let s = cur_slot();
    if s == IDLE { arch::mmu::kernel_root() } else { procs()[s].root }
}

pub fn current_name() -> &'static str {
    let s = cur_slot();
    if s == IDLE { return "idle"; }
    name_str(&procs()[s].name)
}

pub fn set_current_name(name: &[u8]) {
    let _g = SCHED.guard();
    let s = cur_slot();
    if s != IDLE {
        let p = &mut procs()[s];
        p.name = [0u8; NAME_LEN];
        let n = name.len().min(NAME_LEN - 1);
        p.name[..n].copy_from_slice(&name[..n]);
    }
}

fn name_str(n: &[u8; NAME_LEN]) -> &str {
    let len = n.iter().position(|&b| b == 0).unwrap_or(NAME_LEN);
    core::str::from_utf8(&n[..len]).unwrap_or("?")
}

/// Se o processo corrente foi marcado por SYS_KILL (em outra CPU),
/// termina-o agora. Chamado em toda entrada no kernel.
pub unsafe fn exit_if_killed(frame: *mut Context) -> bool {
    let _g = SCHED.guard();
    let s = cur_slot();
    if s != IDLE && procs()[s].killed {
        unsafe { exit_locked(frame) };
        return true;
    }
    false
}

pub fn note_crash(pid: usize) {
    let _g = SCHED.guard();
    unsafe { LAST_CRASH = pid; }
}

/// Retorna (e limpa) o pid do ultimo processo morto por fault.
pub fn take_crash() -> Option<usize> {
    let _g = SCHED.guard();
    unsafe {
        let p = LAST_CRASH;
        LAST_CRASH = usize::MAX;
        if p == usize::MAX { None } else { Some(p) }
    }
}

/// Informacoes para o monitor do sistema: (pid, estado, cpu, ms de CPU, nome).
pub fn info(slot: usize) -> Option<(usize, u32, u32, u64, [u8; NAME_LEN])> {
    if slot >= MAX_PROCS { return None; }
    let _g = SCHED.guard();
    let p = &procs()[slot];
    if p.state == State::Unused || p.state == State::Zombie { return None; }
    let st = match p.state {
        State::Running => 1,
        State::Ready => 2,
        State::Sleeping | State::FutexWait | State::WaitChild | State::VforkWait | State::IoWait
            | State::SigWait => 3,
        _ => 4, // bloqueado em IPC
    };
    Some((p.pid, st, p.cpu as u32, time::ticks_to_ms(p.cpu_time), p.name))
}

pub fn cpu_load(c: usize) -> Option<u32> {
    if c >= MAX_CPUS || !ONLINE[c].load(Ordering::Relaxed) { return None; }
    let _g = SCHED.guard();
    Some(unsafe { LOAD_PCT[c] })
}

pub fn online_cpus() -> usize {
    ONLINE.iter().filter(|o| o.load(Ordering::Relaxed)).count()
}

// ------------------------------------------------------------- escalonador

/// Acorda ate `max` CPUs ociosas (outras que nao esta) para pegar trabalho novo.
fn kick_idle_cpus(mut max: usize) {
    if max == 0 { return; }
    let me = cpu();
    for c in 0..MAX_CPUS {
        if c != me && ONLINE[c].load(Ordering::Relaxed) && unsafe { CURRENT[c] } == IDLE {
            arch::send_ipi(c);
            max -= 1;
            if max == 0 { return; }
        }
    }
}

#[inline]
fn kick_idle_cpu() {
    kick_idle_cpus(1);
}

fn account(c: usize, now: u64) {
    unsafe {
        let dt = now.saturating_sub(LAST_SWITCH[c]);
        LAST_SWITCH[c] = now;
        let s = CURRENT[c];
        if s == IDLE {
            IDLE_T[c] += dt;
        } else {
            BUSY[c] += dt;
            procs()[s].cpu_time += dt;
        }
        if now.saturating_sub(WINDOW_START[c]) >= arch::timer_freq() {
            let total = (BUSY[c] + IDLE_T[c]).max(1);
            LOAD_PCT[c] = ((BUSY[c] * 100) / total) as u32;
            BUSY[c] = 0;
            IDLE_T[c] = 0;
            WINDOW_START[c] = now;
        }
    }
}

fn wake_sleepers(now: u64) {
    for i in 0..MAX_PROCS {
        let p = &mut procs()[i];
        if p.alarm_at != 0 && p.alarm_at <= now && p.state != State::Unused && p.state != State::Zombie {
            p.alarm_at = if p.alarm_every != 0 { now + p.alarm_every } else { 0 };
            let pid = p.pid;
            post_signal_locked(pid, crate::signal::SIGALRM, false, 0);
        }
    }
    for p in procs().iter_mut() {
        if p.state == State::Sleeping && p.wake_at <= now {
            p.state = State::Ready;
            p.ctx.set_ret(0);
        } else if p.state == State::FutexWait && p.wake_at != 0 && p.wake_at <= now {
            p.state = State::Ready;
            p.wake_at = 0;
            // tempo esgotado: -2 no Pulsar, -ETIMEDOUT no Linux
            p.ctx.set_ret(if p.abi == ABI_LINUX { (-110i64) as u64 } else { (-2i64) as u64 });
        } else if p.state == State::IoWait && p.wake_at != 0 && p.wake_at <= now {
            // prazo da espera: a syscall (ja recuada) roda de novo e decide
            p.state = State::Ready;
            p.wake_at = 0;
        } else if p.state == State::RecvBlocked && p.wake_at != 0 && p.wake_at <= now {
            // IPC_RECV_TIMEOUT expirou sem mensagem
            p.state = State::Ready;
            p.wake_at = 0;
            p.ctx.set_ret(u64::MAX);
        }
    }
}

/// Programa o proximo IRQ de timer desta CPU: fim do quantum ou o
/// processo dormindo que acorda primeiro, o que vier antes.
fn program_timer(now: u64) {
    let mut deadline = now + time::ms_to_ticks(time::QUANTUM_MS);
    for p in procs().iter() {
        let timed = p.state == State::Sleeping || ((p.state == State::RecvBlocked || p.state == State::FutexWait
            || p.state == State::IoWait) && p.wake_at != 0);
        if timed && p.wake_at < deadline {
            deadline = p.wake_at.max(now + 1);
        }
        if p.alarm_at != 0 && p.alarm_at < deadline && p.state != State::Unused {
            deadline = p.alarm_at.max(now + 1);
        }
    }
    arch::set_timer_deadline(deadline);
}

/// Escolhe o proximo processo para esta CPU e o carrega em `frame`. O
/// contexto do corrente ja deve ter sido salvo pelo chamador.
unsafe fn dispatch(frame: *mut Context) {
    let c = cpu();
    let now = arch::now();
    account(c, now);
    wake_sleepers(now);

    let ps = procs();
    let mut chosen = IDLE;
    unsafe {
        let prev = CURRENT[c];
        for off in 0..MAX_PROCS {
            let i = (RR_NEXT + off) % MAX_PROCS;
            if ps[i].state == State::Ready {
                chosen = i;
                RR_NEXT = (i + 1) % MAX_PROCS;
                break;
            }
        }
        // FP: o kernel nao usa esses registradores, entao eles ainda sao do
        // processo anterior. So trocam quando o dono muda (o ocioso nao tem
        // FP; quem saiu via exit_locked ja deixou CURRENT = IDLE).
        if chosen != prev {
            if prev != IDLE && ps[prev].ctx.fp_take_dirty() {
                arch::fp_save(&mut ps[prev].fp);
            }
            if chosen != IDLE {
                arch::fp_restore(&ps[chosen].fp);
            }
        }
        CURRENT[c] = chosen;
        CUR_ABI[c] = if chosen == IDLE { ABI_PULSAR } else { ps[chosen].abi };
        if chosen == IDLE {
            *frame = Context::new_idle(c);
            arch::mmu::switch_space(arch::mmu::kernel_root(), 0);
        } else {
            ps[chosen].state = State::Running;
            ps[chosen].cpu = c;
            *frame = ps[chosen].ctx;
            arch::mmu::switch_space(ps[chosen].root, ps[chosen].asid);
        }
    }
    program_timer(now);
}

/// Salva o corrente (se Running, volta a Ready) e reescalona.
pub unsafe fn schedule(frame: *mut Context) {
    let _g = SCHED.guard();
    unsafe { schedule_locked(frame) };
}

unsafe fn schedule_locked(frame: *mut Context) {
    let s = cur_slot();
    if s != IDLE {
        let p = &mut procs()[s];
        p.ctx = unsafe { *frame };
        if p.state == State::Running {
            p.state = State::Ready;
        }
    }
    unsafe { dispatch(frame) };
}

/// Salva o corrente, que ja mudou de estado (bloqueou), e reescalona.
unsafe fn block(frame: *mut Context) {
    let s = cur_slot();
    procs()[s].ctx = unsafe { *frame };
    unsafe { dispatch(frame) };
}

/// Tick do timer: preempcao round-robin (ou sai do ocioso).
pub unsafe fn preempt(frame: *mut Context) {
    unsafe { schedule(frame) };
}


/// IPI: se esta CPU esta ociosa, procura trabalho.
pub unsafe fn on_ipi(frame: *mut Context) {
    let _g = SCHED.guard();
    if cur_slot() == IDLE {
        unsafe { dispatch(frame) };
    }
}

/// Primeira entrada de uma CPU no escalonador.
pub unsafe fn enter_scheduler(c: usize) -> Context {
    let _g = SCHED.guard();
    unsafe {
        LAST_SWITCH[c] = arch::now();
        WINDOW_START[c] = LAST_SWITCH[c];
        ONLINE[c].store(true, Ordering::Relaxed);
        CURRENT[c] = IDLE;
        let mut ctx = Context::new_idle(c);
        dispatch(&mut ctx);
        ctx
    }
}

/// SYS_SLEEP: dorme `ms` milissegundos.
pub unsafe fn sleep(frame: *mut Context, ms: u64) {
    unsafe { sleep_ticks(frame, time::ms_to_ticks(ms)) };
}

/// Dorme por `ticks` do contador de tempo (resolucao de nanosleep).
pub unsafe fn sleep_ticks(frame: *mut Context, ticks: u64) {
    let _g = SCHED.guard();
    let s = cur_slot();
    let p = &mut procs()[s];
    p.wake_at = arch::now() + ticks;
    p.state = State::Sleeping;
    unsafe {
        (*frame).set_ret(0);
        block(frame);
    }
}

// ------------------------------------------------------------- termino

/// Libera o slot `s`: desbloqueia parceiros de IPC e devolve a memoria.
/// O processo NAO pode estar rodando em nenhuma CPU. Se `s` e o lider, as
/// threads dele morrem junto; o espaco de enderecamento so e liberado quando
/// o ultimo slot que o usa termina.
fn reap(s: usize) {
    let ps = procs();
    let l = ps[s].leader;
    ps[s].killed = false;
    ps[s].state = State::Unused;          // nao casa mais como parceiro de IPC
    for i in 0..MAX_PROCS {
        let p = &mut ps[i];
        let waiting_reply = p.state == State::ReplyBlocked && p.peer == s;
        let waiting_send = p.state == State::SendBlocked && p.send_to == s;
        if waiting_reply || waiting_send {
            p.reply = Message::zero();
            p.ctx.set_ret(u64::MAX); // -1: parceiro morreu
            p.state = State::Ready;
        }
    }
    crate::input::forget(ps[s].pid);
    wake_vfork_parent(s);
    if l == s {
        record_exit(s);
        crate::file::release(s);
        crate::file::flush_wakes_sched_held(&mut io_wake_locked);
        for i in 0..MAX_PROCS {
            if i == s || ps[i].leader != s { continue; }
            match ps[i].state {
                State::Unused | State::Zombie => {}
                State::Running => { ps[i].killed = true; arch::send_ipi(ps[i].cpu); }
                _ => reap(i),
            }
        }
    }
    release(s);
    kick_idle_cpu();
}

/// O processo do lider `s` terminou: guarda o status para wait() e acorda
/// o pai, se estiver esperando.
fn record_exit(s: usize) {
    let ps = procs();
    let (pid, parent, status) = (ps[s].pid, ps[s].parent, ps[s].status);

    if ps[s].abi == ABI_LINUX {
        use core::fmt::Write;
        let _ = write!(crate::uart::Uart, "[linux] '{}' (pid {}) terminou: {}\n", name_str(&ps[s].name), pid,
            if status & 0x7F != 0 { "sinal" } else { "saida" });
        let _ = write!(crate::uart::Uart, "[linux]   codigo {}\n", if status & 0x7F != 0 { status & 0x7F } else { status >> 8 });
    }
    let mut redo = false;
    for p in ps.iter_mut() {
        if p.state == State::WaitChild && p.pid != pid && procs_pid_of_leader(p) == parent
            && (p.wait_pid == -1 || p.wait_pid == pid as i64)
        {
            if p.wait_ptr == WAIT_REDO {
                // waitid: roda de novo e acha o registro abaixo (talvez sem colher)
                p.ctx.rewind_syscall();
                p.state = State::Ready;
                redo = true;
                continue;
            }
            if p.wait_ptr != 0 {
                if let Some(pa) = arch::mmu::translate_user(p.root, p.wait_ptr, true) {
                    unsafe { core::ptr::write_unaligned(pa as *mut u32, status) };
                }
            }
            p.ctx.set_ret(pid as u64);
            p.state = State::Ready;
            kick_idle_cpu();
            // colhido: nao entra na lista; o SIGCHLD vem depois da colheita
            if parent != usize::MAX { post_signal_locked(parent, crate::signal::SIGCHLD, false, pid); }
            return;
        }
    }
    if redo { kick_idle_cpu(); }
    unsafe {
        EXITS[EXITS_NEXT] = (pid, parent, status);
        EXITS_NEXT = (EXITS_NEXT + 1) % EXITS_MAX;
    }
    if parent != usize::MAX { post_signal_locked(parent, crate::signal::SIGCHLD, false, pid); }
}

fn procs_pid_of_leader(p: &Process) -> usize {
    procs()[p.leader].pid
}

/// Define o status de termino do processo corrente (antes de sair). Sem
/// `group`, so vale se quem sai e o lider (exit de uma thread nao conta).
pub fn set_status(status: u32, group: bool) {
    let _g = SCHED.guard();
    let s = cur_slot();
    if s != IDLE {
        let l = procs()[s].leader;
        if group || l == s { procs()[l].status = status; }
    }
}

/// Status de termino de `pid` para quando ele for morto de fora.
fn set_status_of(s: usize, status: u32) {
    let l = procs()[s].leader;
    procs()[l].status = status;
}

/// SYS_WAIT: espera um filho terminar. `pid` = -1: qualquer filho. Com
/// `nohang`, retorna 0 se nenhum terminou ainda. Retorna o pid colhido (e
/// grava o status em `status_ptr`, se != 0), ou -ECHILD (-10).
pub unsafe fn wait_child(frame: *mut Context, pid: i64, status_ptr: u64, nohang: bool) {
    let _g = SCHED.guard();
    let s = cur_slot();
    let me = procs()[procs()[s].leader].pid;
    unsafe {
        for e in (*(&raw mut EXITS)).iter_mut() {
            if e.0 != usize::MAX && e.1 == me && (pid == -1 || e.0 as i64 == pid) {
                if status_ptr != 0 { core::ptr::write_unaligned(status_ptr as *mut u32, e.2); }
                (*frame).set_ret(e.0 as u64);
                e.0 = usize::MAX;
                return;
            }
        }
    }
    let alive = procs().iter().any(|p| {
        p.state != State::Unused && p.state != State::Zombie && p.leader == slot_index(p) && p.parent == me
            && (pid == -1 || p.pid as i64 == pid)
    });
    if !alive {
        unsafe { (*frame).set_ret((-10i64) as u64) };
        return;
    }
    if nohang {
        unsafe { (*frame).set_ret(0) };
        return;
    }
    let p = &mut procs()[s];
    p.state = State::WaitChild;
    p.wait_pid = pid;
    p.wait_ptr = status_ptr;
    unsafe { block(frame) };
}

/// Resultado de waitid_child.
pub enum Waited { Found(usize, u32), Nothing, NoChild, Blocked }

/// waitid(): como wait_child, mas devolve (pid, status) a quem chama (que
/// preenche o siginfo) e, com `nowait` (WNOWAIT), so espia: o filho continua
/// por colher. Quem bloqueia fica em WaitChild com wait_ptr = WAIT_REDO e,
/// quando um filho termina, a syscall roda de novo e acha o registro.
pub unsafe fn waitid_child(frame: *mut Context, pid: i64, nohang: bool, nowait: bool) -> Waited {
    let _g = SCHED.guard();
    let s = cur_slot();
    let me = procs()[procs()[s].leader].pid;
    unsafe {
        for e in (*(&raw mut EXITS)).iter_mut() {
            if e.0 != usize::MAX && e.1 == me && (pid == -1 || e.0 as i64 == pid) {
                let r = Waited::Found(e.0, e.2);
                if !nowait { e.0 = usize::MAX; }
                return r;
            }
        }
    }
    let alive = procs().iter().any(|p| {
        p.state != State::Unused && p.state != State::Zombie && p.leader == slot_index(p) && p.parent == me
            && (pid == -1 || p.pid as i64 == pid)
    });
    if !alive { return Waited::NoChild; }
    if nohang { return Waited::Nothing; }
    let p = &mut procs()[s];
    p.state = State::WaitChild;
    p.wait_pid = pid;
    p.wait_ptr = WAIT_REDO;
    unsafe { block(frame) };
    Waited::Blocked
}

/// wait_ptr de quem espera em waitid (refaz a syscall em vez de colher).
const WAIT_REDO: u64 = u64::MAX;

fn slot_index(p: &Process) -> usize {
    p.pid % MAX_PROCS
}

/// O slot `s` acabou: larga o espaco de enderecamento e libera o slot.
fn release(s: usize) {
    let ps = procs();
    let owner_busy = drop_mm(s);
    if owner_busy {
        ps[s].state = State::Zombie;         // threads/filho de vfork ainda usam o espaco
    } else {
        ps[s].state = State::Unused;
    }
}

/// `s` deixa de usar o seu espaco. Se era o ultimo usuario, o espaco e
/// liberado. Retorna true se `s` e o dono e ainda ha outros usuarios (o slot
/// precisa ficar como Zombie guardando as tabelas).
fn drop_mm(s: usize) -> bool {
    let ps = procs();
    let m = ps[s].mm;
    if ps[m].members > 0 { ps[m].members -= 1; }
    if m == s {
        if ps[s].members > 0 { return true; }
        free_mm(s);
    } else {
        ps[s].root = 0;
        ps[s].mm = s;
        if ps[m].state == State::Zombie && ps[m].members == 0 {
            free_mm(m);
            ps[m].state = State::Unused;
        }
    }
    false
}

fn free_mm(m: usize) {
    let ps = procs();
    if ps[m].root != 0 { arch::mmu::free_user_space(ps[m].root); }
    crate::vm::release(m);
    ps[m].root = 0;
}

/// Acorda o pai suspenso por vfork (o filho fez execve ou terminou).
fn wake_vfork_parent(s: usize) {
    let ps = procs();
    let p = ps[s].vfork_parent;
    ps[s].vfork_parent = IDLE;
    if p != IDLE && ps[p].state == State::VforkWait {
        ps[p].state = State::Ready;
        kick_idle_cpu();
    }
}

/// Uma thread falhou (ou o processo pediu): termina o processo inteiro.
pub unsafe fn exit_group(frame: *mut Context) {
    let _g = SCHED.guard();
    let s = cur_slot();
    if s == IDLE { return; }
    let ps = procs();
    let l = ps[s].leader;
    if l != s {
        match ps[l].state {
            State::Unused | State::Zombie => {}
            State::Running => { ps[l].killed = true; arch::send_ipi(ps[l].cpu); }
            _ => reap(l),
        }
    }
    unsafe { exit_locked(frame) };
}

/// SYS_FUTEX_WAIT: dorme se o u32 em `addr` ainda vale `val` (checado sob
/// a trava, entao um WAKE nao se perde). Retorna 0 (acordado), -1 (valor
/// mudou) ou -2 (tempo esgotado). `timeout_ms` = 0: sem limite.
pub unsafe fn futex_wait(frame: *mut Context, addr: u64, val: u32, timeout_ms: u64) {
    let ticks = if timeout_ms > 0 { time::ms_to_ticks(timeout_ms) } else { 0 };
    unsafe { futex_wait_ticks(frame, addr, val, ticks) };
}

/// FUTEX_WAIT com timeout em ticks do contador (0 = sem limite).
pub unsafe fn futex_wait_ticks(frame: *mut Context, addr: u64, val: u32, timeout_ticks: u64) {
    let _g = SCHED.guard();
    let s = cur_slot();
    let cur = unsafe { core::ptr::read_volatile(addr as *const u32) };
    if cur != val {
        // valor mudou: -1 no Pulsar, -EAGAIN no Linux
        let e = if procs()[s].abi == ABI_LINUX { (-11i64) as u64 } else { u64::MAX };
        unsafe { (*frame).set_ret(e) };
        return;
    }
    let p = &mut procs()[s];
    p.state = State::FutexWait;
    p.futex_addr = addr;
    p.wake_at = if timeout_ticks > 0 { arch::now() + timeout_ticks } else { 0 };
    unsafe {
        (*frame).set_ret(0);
        block(frame);
    }
}

/// SYS_FUTEX_WAKE: acorda ate `n` threads do mesmo processo esperando em `addr`.
pub fn futex_wake(addr: u64, n: u64) -> u64 {
    let _g = SCHED.guard();
    let s = cur_slot();
    if s == IDLE { return 0; }
    futex_wake_locked(procs()[s].root, addr, n)
}

fn futex_wake_locked(root: u64, addr: u64, n: u64) -> u64 {
    let ps = procs();
    let mut woken = 0;
    for p in ps.iter_mut() {
        if woken >= n { break; }
        if p.state == State::FutexWait && p.root == root && p.futex_addr == addr {
            p.state = State::Ready;
            p.wake_at = 0;
            p.ctx.set_ret(0);
            woken += 1;
        }
    }
    if woken > 0 { kick_idle_cpus(woken as usize); }
    woken
}

/// Termina o processo corrente e troca para o proximo.
pub unsafe fn exit_current(frame: *mut Context) {
    let _g = SCHED.guard();
    unsafe { exit_locked(frame) };
}

unsafe fn exit_locked(frame: *mut Context) {
    let c = cpu();
    let s = cur_slot();
    if s == IDLE { return; }
    // CLONE_CHILD_CLEARTID / set_tid_address: zera o tid e acorda quem
    // espera (e assim que pthread_join sabe que a thread terminou)
    let ct = procs()[s].clear_tid;
    if ct != 0 {
        let root = procs()[s].root;
        if let Some(pa) = arch::mmu::translate_user(root, ct, true) {
            unsafe { core::ptr::write_volatile(pa as *mut u32, 0) };
            futex_wake_locked(root, ct, 1);
        }
    }
    unsafe {
        procs()[s].ctx = *frame;
        CURRENT[c] = IDLE;
        // contabiliza o tempo final e sai do espaco do processo antes de
        // liberar suas tabelas
        account_and_leave(c, s);
        reap(s);
        dispatch(frame);
    }
}

fn account_and_leave(c: usize, s: usize) {
    unsafe {
        let now = arch::now();
        let dt = now.saturating_sub(LAST_SWITCH[c]);
        LAST_SWITCH[c] = now;
        BUSY[c] += dt;
        procs()[s].cpu_time += dt;
    }
    arch::mmu::switch_space(arch::mmu::kernel_root(), 0);
}

/// SYS_KILL: termina `pid`. Se ele estiver rodando em outra CPU, marca e
/// interrompe aquela CPU (ela o termina ao entrar no kernel).
pub unsafe fn kill(frame: *mut Context, pid: usize) -> bool {
    unsafe { kill_sig(frame, pid, SIGKILL) }
}

/// Termina `pid` registrando a morte pelo sinal `sig` (sem tratadores de
/// sinal: todo sinal fatal encerra o processo).
pub unsafe fn kill_sig(frame: *mut Context, pid: usize, sig: u32) -> bool {
    let _g = SCHED.guard();
    let Some(s) = slot_of(pid) else { return false };
    set_status_of(s, sig & 0x7F);
    if s == cur_slot() {
        unsafe { exit_locked(frame) };
        return true;
    }
    let p = &mut procs()[s];
    if p.state == State::Running {
        p.killed = true;
        arch::send_ipi(p.cpu);
    } else {
        reap(s);
    }
    true
}

// ------------------------------------------------------------------ IPC

/// SEND sincrono: entrega `msg` a `dst` e bloqueia ate o reply. A resposta
/// e escrita em `msg_ptr` (e x0/a0 = 0) quando o reply chegar.
pub unsafe fn ipc_send(frame: *mut Context, dst_pid: usize, msg: Message, msg_ptr: u64) -> bool {
    let _g = SCHED.guard();
    let me = cur_slot();
    let Some(dst) = slot_of(dst_pid) else { return false };
    if dst == me { return false; }
    let ps = procs();
    ps[me].msg_ptr = msg_ptr;
    if ps[dst].state == State::RecvBlocked {
        ps[dst].wake_at = 0;
        ps[dst].inbox = msg;
        ps[dst].peer = me;
        deliver_recv(dst);
        ps[dst].state = State::Ready;
        ps[me].state = State::ReplyBlocked;
        ps[me].peer = dst;
        kick_idle_cpu();
    } else {
        ps[me].state = State::SendBlocked;
        ps[me].send_to = dst;
        ps[me].inbox = msg;
    }
    unsafe { block(frame) };
    true
}

/// Procura um emissor esperando por `me`, a partir do slot seguinte.
fn find_sender(me: usize) -> Option<usize> {
    let ps = procs();
    for off in 1..=MAX_PROCS {
        let s = (me + off) % MAX_PROCS;
        if ps[s].state == State::SendBlocked && ps[s].send_to == me {
            return Some(s);
        }
    }
    None
}

/// Casa o receptor corrente com o emissor `s`: copia a mensagem para o
/// buffer do receptor (espaco ativo) e devolve o pid do emissor.
unsafe fn accept_from(frame: *mut Context, me: usize, s: usize, msg_ptr: u64) {
    let ps = procs();
    ps[me].inbox = ps[s].inbox;
    ps[me].peer = s;
    ps[s].state = State::ReplyBlocked;
    ps[s].peer = me;
    if msg_ptr != 0 {
        write_msg(ps[me].root, msg_ptr, &ps[me].inbox);
    }
    unsafe { (*frame).set_ret(ps[s].pid as u64) };
}

/// RECV bloqueante. Retorna o pid do remetente. Com `timeout_ms` > 0,
/// desiste depois do prazo e retorna -1 (o WM espera mensagens OU o
/// proximo quadro, sem girar em falso).
pub unsafe fn ipc_recv(frame: *mut Context, msg_ptr: u64, timeout_ms: u64) {
    let _g = SCHED.guard();
    let me = cur_slot();
    if let Some(s) = find_sender(me) {
        unsafe { accept_from(frame, me, s, msg_ptr) };
        return;
    }
    let p = &mut procs()[me];
    p.msg_ptr = msg_ptr;
    p.state = State::RecvBlocked;
    p.wake_at = if timeout_ms > 0 { arch::now() + time::ms_to_ticks(timeout_ms) } else { 0 };
    unsafe { block(frame) };
}

/// RECV nao-bloqueante: pid do remetente ou -1.
pub unsafe fn ipc_try_recv(frame: *mut Context, msg_ptr: u64) {
    let _g = SCHED.guard();
    let me = cur_slot();
    match find_sender(me) {
        Some(s) => unsafe { accept_from(frame, me, s, msg_ptr) },
        None => unsafe { (*frame).set_ret(u64::MAX) },
    }
}

/// REPLY: responde e desbloqueia o remetente. O chamador continua rodando
/// (o remetente acorda em outra CPU, se houver uma livre).
pub unsafe fn ipc_reply(frame: *mut Context, to_pid: usize, msg: Message) {
    let _g = SCHED.guard();
    let me = cur_slot();
    unsafe { (*frame).set_ret(0) };
    let Some(to) = slot_of(to_pid) else { return };
    let ps = procs();
    if ps[to].state == State::ReplyBlocked && ps[to].peer == me {
        ps[to].reply = msg;
        let ptr = ps[to].msg_ptr;
        if ptr != 0 {
            write_msg(ps[to].root, ptr, &ps[to].reply);
        }
        ps[to].ctx.set_ret(0);
        ps[to].state = State::Ready;
        kick_idle_cpu();
    }
}

/// Entrega a mensagem do inbox no contexto salvo do receptor `p`.
fn deliver_recv(p: usize) {
    let ps = procs();
    let ptr = ps[p].msg_ptr;
    if ptr != 0 {
        write_msg(ps[p].root, ptr, &ps[p].inbox);
    }
    let sender_pid = ps[ps[p].peer].pid;
    ps[p].ctx.set_ret(sender_pid as u64);
}

/// Escreve uma Message no espaco de `root`, palavra a palavra (o buffer
/// pode cruzar paginas fisicamente nao contiguas).
fn write_msg(root: u64, va: u64, m: &Message) {
    let mut words = [0u64; 7];
    words[0] = m.tag;
    words[1..].copy_from_slice(&m.data);
    for (i, w) in words.iter().enumerate() {
        let a = va + (i as u64) * 8;
        if let Some(pa) = arch::mmu::translate_user(root, a, true) {
            unsafe { core::ptr::write_unaligned(pa as *mut u64, *w) };
        }
    }
}

/// SYS_SBRK: aumenta o heap do processo corrente em `inc` bytes (paginas
/// novas zeradas, RW e nao executaveis). Retorna o fim anterior, ou None
/// se faltar memoria ou passar do limite. Diminuir nao devolve memoria.
pub fn sbrk(inc: u64) -> Option<u64> {
    let s = cur_slot();
    if s == IDLE { return None; }
    // o heap e do lider; a faixa e reservada sob a trava, entao threads
    // podem chamar sbrk ao mesmo tempo
    let (old, root, l) = {
        let _g = SCHED.guard();
        let l = procs()[s].mm;
        let old = procs()[l].brk;
        let new = old.checked_add(inc)?;
        if new > HEAP_LIMIT { return None; }
        procs()[l].brk = new;
        (old, procs()[l].root, l)
    };
    let new = old + inc;
    let mut page = (old + 4095) & !4095;
    while page < new {
        let mapped = crate::frame_allocator::alloc_frame().filter(|&frame| unsafe {
            core::ptr::write_bytes(frame as *mut u8, 0, 4096);
            let ok = arch::mmu::map_page_in(root, page, frame, arch::mmu::PageFlags::user_data());
            if !ok { crate::frame_allocator::free_frame(frame); }
            ok
        });
        if mapped.is_none() {
            // guarda o que ja foi mapeado, para nao remapear (e vazar) depois
            let _g = SCHED.guard();
            if procs()[l].brk == new { procs()[l].brk = page.max(old); }
            return None;
        }
        page += 4096;
    }
    arch::mmu::sync_new_pages();
    Some(old)
}

/// brk() do Linux: `addr` = 0 (ou abaixo do inicio) consulta; maior que o
/// atual cresce o heap. Retorna o fim do heap depois da operacao.
pub fn brk(addr: u64) -> u64 {
    let cur = {
        let _g = SCHED.guard();
        let s = cur_slot();
        if s == IDLE { return 0; }
        procs()[procs()[s].mm].brk
    };
    if addr <= cur { return cur; }
    match sbrk(addr - cur) {
        Some(_) => addr,
        None => {
            let _g = SCHED.guard();
            let s = cur_slot();
            procs()[procs()[s].mm].brk
        }
    }
}

// ------------------------------------------------------------- fork / exec

/// fork() (`vfork` = false) ou vfork() / clone(CLONE_VM|CLONE_VFORK).
/// O filho volta da mesma syscall com 0 e herda descritores e diretorio.
/// No fork o espaco e copiado (`new_root`, ja pronto); no vfork o filho usa
/// o espaco do pai, que fica suspenso ate o filho fazer execve ou sair.
/// `stack` != 0 troca a pilha do filho. Retorna o pid do filho.
pub unsafe fn fork(frame: *mut Context, new_root: Option<(u64, u16)>, stack: u64,
                   tls: Option<u64>, clear_tid: u64) -> Option<usize> {
    let _g = SCHED.guard();
    let s = cur_slot();
    if s == IDLE { return None; }
    let ps = procs();
    let (l, m) = (ps[s].leader, ps[s].mm);
    let i = (0..MAX_PROCS).find(|&i| ps[i].state == State::Unused)?;
    let generation = unsafe { GENERATION[i] };
    unsafe { GENERATION[i] += 1; }
    let mut c = Process::empty();
    c.pid = i + MAX_PROCS * generation;
    c.ctx = unsafe { *frame };
    c.ctx.set_ret(0);
    if stack != 0 { c.ctx.set_sp(stack); }
    if let Some(v) = tls { c.ctx.set_tls(v); }
    unsafe { arch::fp_save(&mut c.fp) };
    c.leader = i;
    c.abi = ps[s].abi;
    c.parent = ps[l].pid;
    c.name = ps[l].name;
    c.brk = ps[m].brk;
    c.clear_tid = clear_tid;
    match new_root {
        Some((root, asid)) => {
            c.root = root;
            c.asid = asid;
            c.mm = i;
            c.members = 1;
        }
        None => {
            c.root = ps[m].root;
            c.asid = ps[m].asid;
            c.mm = m;
            ps[m].members += 1;
            c.vfork_parent = s;
        }
    }
    c.sig = crate::signal::SigThread { pending: 0, ..ps[s].sig };
    c.state = State::Ready;
    ps[i] = c;
    crate::signal::copy_actions(l, i);
    crate::file::clone_table(l, i);
    crate::file::set_cwd_of(i, crate::file::cwd_of(l));
    unsafe {
        let (src, n) = ((*(&raw const EXE))[l], EXE_LEN[l]);
        (*(&raw mut EXE))[i] = src;
        EXE_LEN[i] = n;
        let (cs, cn) = ((*(&raw const CMDLINE))[l], CMD_LEN[l]);
        (*(&raw mut CMDLINE))[i] = cs;
        CMD_LEN[i] = cn;
    }
    if new_root.is_some() { crate::vm::clone_space(m, i); }
    kick_idle_cpu();
    let pid = ps[i].pid;
    if new_root.is_none() {
        // vfork: o pai so continua quando o filho largar o espaco
        ps[s].state = State::VforkWait;
        unsafe {
            (*frame).set_ret(pid as u64);
            block(frame);
        }
    }
    Some(pid)
}

/// execve(): troca a imagem do processo corrente pelo espaco `root` ja
/// carregado. Outras threads do processo morrem; o espaco antigo e largado
/// (num filho de vfork, devolvido ao pai, que acorda). Descritores com
/// close-on-exec sao fechados. Falha (false) se outra thread ainda roda.
pub unsafe fn exec_replace(frame: *mut Context, root: u64, asid: u16, entry: u64, sp: u64,
                           abi: u8, name: &str, setup: &mut dyn FnMut(usize)) -> bool {
    let _g = SCHED.guard();
    let s = cur_slot();
    let ps = procs();
    if s == IDLE || ps[s].leader != s { return false; }
    // derruba as outras threads (as que rodam em outra CPU saem depois)
    for i in 0..MAX_PROCS {
        if i == s || ps[i].leader != s { continue; }
        match ps[i].state {
            State::Unused | State::Zombie => {}
            State::Running => { ps[i].killed = true; arch::send_ipi(ps[i].cpu); }
            _ => reap(i),
        }
    }
    if ps[s].mm == s && ps[s].members > 1 { return false; }
    // sai do espaco antigo antes de liberar as tabelas dele
    arch::mmu::switch_space(root, asid);
    wake_vfork_parent(s);
    drop_mm(s);
    let p = &mut ps[s];
    p.root = root;
    p.asid = asid;
    p.mm = s;
    p.members = 1;
    p.abi = abi;
    p.brk = HEAP_BASE;
    p.clear_tid = 0;
    p.name = [0; NAME_LEN];
    let n = name.len().min(NAME_LEN - 1);
    p.name[..n].copy_from_slice(&name.as_bytes()[..n]);
    p.fp = FpState::zeroed();
    unsafe {
        arch::fp_restore(&p.fp);
        *frame = Context::new_user(entry, sp);
    }
    unsafe { CUR_ABI[cpu()] = abi; }
    p.sig.alt_sp = 0;
    p.sig.alt_size = 0;
    crate::signal::exec_actions(s);
    setup(s);
    crate::file::exec_cleanup(s);
    crate::file::flush_wakes_sched_held(&mut io_wake_locked);
    true
}

// ------------------------------------------------------------- esperas de E/S

/// Contadores de eventos por objeto (hash): quem decide esperar le o
/// contador com a trava do objeto e so dorme se ninguem sinalizou depois.
static IO_SEQ: [core::sync::atomic::AtomicU64; 64] = [const { core::sync::atomic::AtomicU64::new(0) }; 64];

#[inline]
pub fn key_bucket(key: u64) -> usize {
    ((key.wrapping_mul(0x9E37_79B9_7F4A_7C15) >> 58) as usize) & 63
}

#[inline]
pub fn key_bit(key: u64) -> u64 {
    1u64 << key_bucket(key)
}

#[inline]
pub fn bump_io_seq(key: u64) {
    IO_SEQ[key_bucket(key)].fetch_add(1, Ordering::AcqRel);
    IO_SEQ[key_bucket(POLL_KEY)].fetch_add(1, Ordering::AcqRel);
}

#[inline]
pub fn io_seq(key: u64) -> u64 {
    IO_SEQ[key_bucket(key)].load(Ordering::Acquire)
}

pub fn io_seq_mask(mask: u64) -> u64 {
    if mask == 0 { return io_seq(POLL_KEY); }
    let mut sum = 0u64;
    let mut m = mask;
    while m != 0 {
        let b = m.trailing_zeros() as usize;
        sum = sum.wrapping_add(IO_SEQ[b].load(Ordering::Acquire));
        m &= m - 1;
    }
    sum
}

/// Chave especial: qualquer evento nos buckets de `io_mask` acorda quem espera nela (poll,
/// select, epoll refazem a varredura).
pub const POLL_KEY: u64 = 0x504F_4C4C;

/// Dorme ate `io_wake(key)` ou ate `deadline` (contador; 0 = sem prazo); a
/// syscall e reexecutada ao acordar. Se houve um evento desde `seen` (lido
/// com io_seq) ou ha sinal pendente, reexecuta ja.
pub unsafe fn io_wait_until(frame: *mut Context, key: u64, seen: u64, deadline: u64) {
    let _g = SCHED.guard();
    let s = cur_slot();
    unsafe { (*frame).rewind_syscall() };
    if key == POLL_KEY { procs()[s].in_poll = true; }
    if io_seq(key) != seen { return; }
    if procs()[s].sig.pending & !procs()[s].sig.mask != 0 { return; } // sinal pendente: refaz ja
    let p = &mut procs()[s];
    p.state = State::IoWait;
    p.io_key = key;
    p.io_mask = if key == POLL_KEY { u64::MAX } else { key_bit(key) };
    p.wake_at = deadline;
    unsafe { block(frame) };
}

/// Dorme em poll/select/epoll filtrando acordares apenas para os buckets de `mask`.
pub unsafe fn io_wait_mask(frame: *mut Context, mask: u64, seen: u64, deadline: u64) {
    let _g = SCHED.guard();
    let s = cur_slot();
    unsafe { (*frame).rewind_syscall() };
    procs()[s].in_poll = true;
    if mask != 0 && io_seq_mask(mask) != seen { return; }
    if procs()[s].sig.pending & !procs()[s].sig.mask != 0 { return; }
    let p = &mut procs()[s];
    p.state = State::IoWait;
    p.io_key = POLL_KEY;
    p.io_mask = mask;
    p.wake_at = deadline;
    unsafe { block(frame) };
}

/// Sinaliza um evento em `key` e acorda quem espera nele. Nao pode ser
/// chamada com a trava FS presa (ordem SCHED -> FS).
pub fn io_wake(key: u64) {
    let _g = SCHED.guard();
    io_wake_locked(key);
}

/// io_wake com SCHED ja presa.
fn io_wake_locked(key: u64) {
    let bit = key_bit(key);
    let mut woken = 0;
    for p in procs().iter_mut() {
        if p.state == State::IoWait && (p.io_key == key || (p.io_key == POLL_KEY && (p.io_mask & bit) != 0)) {
            p.state = State::Ready;
            woken += 1;
        }
    }
    if woken > 0 { kick_idle_cpus(woken); }
}

// ------------------------------------------------------------- caminho do executavel

const EXE_MAX: usize = 256;
static mut EXE: [[u8; EXE_MAX]; MAX_PROCS] = [[0; EXE_MAX]; MAX_PROCS];
static mut EXE_LEN: [usize; MAX_PROCS] = [0; MAX_PROCS];

/// Caminho do executavel do slot (para /proc/self/exe). Chamar com SCHED
/// presa (setup de create_ex / exec_replace) ou antes do processo existir.
pub fn set_exe_path(slot: usize, path: &[u8]) {
    let n = path.len().min(EXE_MAX);
    unsafe {
        let exe = &mut *(&raw mut EXE);
        exe[slot][..n].copy_from_slice(&path[..n]);
        EXE_LEN[slot] = n;
    }
}

/// Copia o caminho do executavel do processo corrente; retorna o tamanho.
pub fn exe_path(out: &mut [u8]) -> usize {
    let _g = SCHED.guard();
    let s = cur_slot();
    if s == IDLE { return 0; }
    let l = procs()[s].leader;
    unsafe {
        let n = EXE_LEN[l].min(out.len());
        let exe = &*(&raw const EXE);
        out[..n].copy_from_slice(&exe[l][..n]);
        n
    }
}

// ------------------------------------------------------------- sinais (apoio a signal.rs)

/// Roda `f` com SCHED presa sobre a thread corrente (e o slot dela).
pub fn with_current<R>(f: impl FnOnce(&mut Process, usize) -> R) -> Option<R> {
    let _g = SCHED.guard();
    let s = cur_slot();
    if s == IDLE { return None; }
    Some(f(&mut procs()[s], s))
}

/// Dica sem trava: a thread corrente desta CPU tem sinal pendente?
#[inline]
pub fn signal_hint() -> bool {
    let s = cur_slot();
    s != IDLE && procs()[s].sig.pending != 0
}

/// Entrega `sig` a uma thread (`tid`) ou ao processo (`pid`, escolhe uma
/// thread que nao bloqueie). Com SCHED presa: acorda a thread se ela
/// dorme numa espera interrompivel, ou manda um IPI se ela roda em outra
/// CPU. Retorna false se o alvo nao existe.
pub fn post_signal(target: usize, sig: u32, thread: bool, info_pid: usize) -> bool {
    let _g = SCHED.guard();
    post_signal_locked(target, sig, thread, info_pid)
}

fn post_signal_locked(target: usize, sig: u32, thread: bool, info_pid: usize) -> bool {
    let Some(t) = slot_of(target) else { return false };
    // ignorado (explicita ou implicitamente): descarta sem acordar ninguem
    if !crate::signal::would_act(procs()[t].leader, sig) { return true; }
    let ps = procs();
    let l = ps[t].leader;
    let bit = 1u64 << (sig - 1);
    let mut dest = t;
    if !thread {
        // prefere o lider; senao a primeira thread que nao bloqueia
        if ps[l].sig.mask & bit != 0 {
            if let Some(i) = (0..MAX_PROCS).find(|&i| ps[i].leader == l && ps[i].state != State::Unused
                && ps[i].state != State::Zombie && ps[i].sig.mask & bit == 0) { dest = i; } else { dest = l; }
        } else { dest = l; }
    }
    let p = &mut ps[dest];
    p.sig.pending |= bit;
    p.sig.from_pid = info_pid as u32;
    if p.sig.mask & bit != 0 && p.state != State::SigWait { return true; }
    let linux = p.abi == ABI_LINUX;
    let restart = crate::signal::restarts(l, sig);
    match p.state {
        State::WaitChild if linux && restart => {
            // SA_RESTART: o wait4 roda de novo depois do tratador
            p.ctx.rewind_syscall();
            p.state = State::Ready;
            kick_idle_cpu();
        }
        State::Sleeping | State::FutexWait | State::WaitChild | State::SigWait => {
            if linux { p.ctx.set_ret((-4i64) as u64); }      // -EINTR
            p.wake_at = 0;
            p.state = State::Ready;
            kick_idle_cpu();
        }
        State::IoWait if p.io_key == POLL_KEY || !restart => {
            // poll/select/pause sempre retornam EINTR (como no Linux)
            p.ctx.skip_syscall();
            p.ctx.set_ret((-4i64) as u64);
            p.wake_at = 0;
            p.io_key = 0;
            p.io_deadline = 0;
            p.in_poll = false;
            p.state = State::Ready;
            kick_idle_cpu();
        }
        State::IoWait => { p.wake_at = 0; p.state = State::Ready; kick_idle_cpu(); } // syscall refeita depois
        State::Running => { if p.cpu != cpu() { arch::send_ipi(p.cpu); } }
        _ => {}
    }
    true
}

/// rt_sigsuspend: troca a mascara e dorme ate chegar um sinal. A mascara
/// antiga volta pelo quadro do tratador (ou na hora, se nenhum rodar).
pub unsafe fn sig_suspend(frame: *mut Context, mask: u64) {
    let _g = SCHED.guard();
    let s = cur_slot();
    let p = &mut procs()[s];
    p.sig.saved = p.sig.mask;
    p.sig.suspended = true;
    p.sig.mask = mask;
    unsafe { (*frame).set_ret((-4i64) as u64) };
    if p.sig.pending & !mask != 0 { return; }
    p.state = State::SigWait;
    unsafe { block(frame) };
}

/// pid -> slot (para signal.rs)
pub fn slot_of_pid(pid: usize) -> Option<usize> {
    let _g = SCHED.guard();
    slot_of(pid)
}

pub fn leader_of_slot(t: usize) -> usize {
    let _g = SCHED.guard();
    procs()[t].leader
}

pub fn leader_pid_of(t: usize) -> usize {
    let _g = SCHED.guard();
    procs()[procs()[t].leader].pid
}

pub fn abi_of_slot(t: usize) -> u8 {
    let _g = SCHED.guard();
    procs()[t].abi
}

/// A thread do slot bloqueia `sig`?
pub fn blocks(t: usize, sig: u32) -> bool {
    let _g = SCHED.guard();
    procs()[t].sig.mask & (1u64 << (sig - 1)) != 0
}

/// setitimer(ITIMER_REAL): arma (`value` > 0, em ns) ou desarma o SIGALRM do
/// processo; retorna (restante, intervalo) anteriores em ns.
pub fn set_alarm(value_ns: u64, every_ns: u64) -> (u64, u64) {
    let _g = SCHED.guard();
    let s = cur_slot();
    if s == IDLE { return (0, 0); }
    let l = procs()[s].leader;
    let now = arch::now();
    let f = arch::timer_freq() as u128;
    let to_ticks = |ns: u64| ((ns as u128 * f).div_ceil(1_000_000_000)) as u64;
    let to_ns = |t: u64| ((t as u128 * 1_000_000_000) / f) as u64;
    let p = &mut procs()[l];
    let old = (if p.alarm_at > now { to_ns(p.alarm_at - now) } else { 0 }, to_ns(p.alarm_every));
    p.alarm_at = if value_ns > 0 { now + to_ticks(value_ns).max(1) } else { 0 };
    p.alarm_every = to_ticks(every_ns);
    if value_ns > 0 { program_timer(now); }
    old
}

/// Prazo de uma espera que sobrevive a reexecucao da syscall: na primeira
/// chamada (`io_deadline` = 0) grava `fresh` e o devolve; nas seguintes
/// devolve o gravado. `fresh` = u64::MAX: sem prazo.
pub fn io_deadline(fresh: u64) -> u64 {
    let _g = SCHED.guard();
    let s = cur_slot();
    if s == IDLE { return fresh; }
    let p = &mut procs()[s];
    if p.io_deadline == 0 { p.io_deadline = fresh; }
    p.io_deadline
}

/// A operacao terminou: a proxima chamada comeca um prazo novo.
pub fn io_deadline_clear() {
    let _g = SCHED.guard();
    let s = cur_slot();
    if s != IDLE { procs()[s].io_deadline = 0; procs()[s].in_poll = false; }
}

// ------------------------------------------------------------- argumentos (/proc/self/cmdline)

const CMD_MAX: usize = 1024;
static mut CMDLINE: [[u8; CMD_MAX]; MAX_PROCS] = [[0; CMD_MAX]; MAX_PROCS];
static mut CMD_LEN: [usize; MAX_PROCS] = [0; MAX_PROCS];

/// Guarda argv (strings separadas por NUL). Mesmas regras de set_exe_path.
pub fn set_cmdline(slot: usize, argv: &[u8]) {
    let n = argv.len().min(CMD_MAX);
    unsafe {
        let c = &mut *(&raw mut CMDLINE);
        c[slot][..n].copy_from_slice(&argv[..n]);
        CMD_LEN[slot] = n;
    }
}

pub fn cmdline(out: &mut [u8]) -> usize {
    let _g = SCHED.guard();
    let s = cur_slot();
    if s == IDLE { return 0; }
    let l = procs()[s].leader;
    unsafe {
        let c = &*(&raw const CMDLINE);
        let n = CMD_LEN[l].min(out.len());
        out[..n].copy_from_slice(&c[l][..n]);
        n
    }
}

/// (threads vivas do processo corrente, tempo de CPU somado em ticks)
pub fn thread_stats() -> (usize, u64) {
    let _g = SCHED.guard();
    let s = cur_slot();
    if s == IDLE { return (0, 0); }
    let l = procs()[s].leader;
    let mut n = 0;
    let mut t = 0;
    for p in procs().iter() {
        if p.leader == l && p.state != State::Unused && p.state != State::Zombie { n += 1; t += p.cpu_time; }
    }
    (n, t)
}

// ------------------------------------------------------------- recuperacao de falhas do kernel

/// Falha do kernel (panic ou excecao em EL1/S) atendendo esta CPU: mata o
/// processo corrente inteiro (como um SIGSEGV) e escolhe o proximo. As
/// travas ja foram soltas. Retorna o contexto a carregar.
pub unsafe fn oops_recover() -> (Context, Option<usize>) {
    let _g = SCHED.guard();
    let c = cpu();
    let mut ctx = Context::new_idle(c);
    let s = cur_slot();
    if s == IDLE {
        unsafe { dispatch(&mut ctx) };
        return (ctx, None);
    }
    let ps = procs();
    let pid = ps[ps[s].leader].pid;
    set_status_of(s, 11);
    unsafe { LAST_CRASH = pid; }
    let l = ps[s].leader;
    if l != s {
        match ps[l].state {
            State::Unused | State::Zombie => {}
            State::Running => { ps[l].killed = true; arch::send_ipi(ps[l].cpu); }
            _ => reap(l),
        }
    }
    unsafe { exit_locked(&mut ctx) };
    (ctx, Some(pid))
}

/// Esta CPU ja esta no escalonador (ha para onde voltar depois de uma falha)?
pub fn cpu_online() -> bool {
    ONLINE[cpu()].load(Ordering::Relaxed)
}
