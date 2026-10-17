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
use crate::arch::{self, platform::MAX_CPUS, Context};
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
    pub leader: usize,   // slot dono do espaco de enderecamento (= proprio slot num processo)
    pub members: usize,  // no lider: slots vivos que usam o espaco (ele + threads)
    pub futex_addr: u64, // VA esperada em FutexWait
    pub name: [u8; NAME_LEN],
}

impl Process {
    pub const fn empty() -> Self {
        Process {
            pid: 0, state: State::Unused, ctx: Context::zeroed(), root: 0, asid: 0,
            inbox: Message::zero(), reply: Message::zero(), peer: 0, send_to: 0,
            msg_ptr: 0, wake_at: 0, cpu: 0, cpu_time: 0, killed: false, brk: HEAP_BASE,
            leader: 0, members: 0, futex_addr: 0, name: [0; NAME_LEN],
        }
    }
}

pub const MAX_PROCS: usize = 32;   // processos + threads

/// Heap de usuario (SYS_SBRK): cresce de HEAP_BASE ate HEAP_LIMIT. Fica num
/// VA alto (64 GiB), longe do codigo e da regiao grafica, com ate 4 GiB.
pub const HEAP_BASE: u64 = 0x10_0000_0000;
pub const HEAP_LIMIT: u64 = HEAP_BASE + 0x1_0000_0000;
pub const IDLE: usize = usize::MAX;

static mut PROCS: [Process; MAX_PROCS] = [Process::empty(); MAX_PROCS];
static mut GENERATION: [usize; MAX_PROCS] = [0; MAX_PROCS];
static mut CURRENT: [usize; MAX_CPUS] = [IDLE; MAX_CPUS];
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
    let _g = SCHED.guard();
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
            ps[i].members = 1;
            let n = name.len().min(NAME_LEN - 1);
            ps[i].name[..n].copy_from_slice(&name.as_bytes()[..n]);
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
    for i in 0..MAX_PROCS {
        if ps[i].state == State::Unused {
            let generation = unsafe { GENERATION[i] };
            unsafe { GENERATION[i] += 1; }
            let mut t = Process::empty();
            t.pid = i + MAX_PROCS * generation;
            t.ctx = Context::new_user(entry, stack_top);
            t.ctx.set_ret(arg);
            t.root = ps[l].root;
            t.asid = ps[l].asid;
            t.leader = l;
            t.name = ps[l].name;
            t.state = State::Ready;
            ps[i] = t;
            ps[l].members += 1;
            kick_idle_cpu();
            return Some(t.pid);
        }
    }
    None
}

// ------------------------------------------------------------- consultas

pub fn current_pid() -> usize {
    let s = cur_slot();
    if s == IDLE { usize::MAX } else { procs()[s].pid }
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
        State::Sleeping | State::FutexWait => 3,
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

/// Acorda uma CPU ociosa (outra que nao esta) para pegar trabalho novo.
fn kick_idle_cpu() {
    let me = cpu();
    for c in 0..MAX_CPUS {
        if c != me && ONLINE[c].load(Ordering::Relaxed) && unsafe { CURRENT[c] } == IDLE {
            arch::send_ipi(c);
            return;
        }
    }
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
    for p in procs().iter_mut() {
        if p.state == State::Sleeping && p.wake_at <= now {
            p.state = State::Ready;
            p.ctx.set_ret(0);
        } else if p.state == State::FutexWait && p.wake_at != 0 && p.wake_at <= now {
            p.state = State::Ready;
            p.wake_at = 0;
            p.ctx.set_ret((-2i64) as u64); // tempo esgotado
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
        let timed = p.state == State::Sleeping || ((p.state == State::RecvBlocked || p.state == State::FutexWait) && p.wake_at != 0);
        if timed && p.wake_at < deadline {
            deadline = p.wake_at.max(now + 1);
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
        for off in 0..MAX_PROCS {
            let i = (RR_NEXT + off) % MAX_PROCS;
            if ps[i].state == State::Ready {
                chosen = i;
                RR_NEXT = (i + 1) % MAX_PROCS;
                break;
            }
        }
        CURRENT[c] = chosen;
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
    let _g = SCHED.guard();
    let s = cur_slot();
    let p = &mut procs()[s];
    p.wake_at = arch::now() + time::ms_to_ticks(ms);
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
    if l == s {
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

/// O slot `s` acabou: conta no lider e libera o espaco se foi o ultimo.
fn release(s: usize) {
    let ps = procs();
    let l = ps[s].leader;
    if ps[l].members > 0 { ps[l].members -= 1; }
    if l == s {
        if ps[s].members == 0 {
            if ps[s].root != 0 { arch::mmu::free_user_space(ps[s].root); }
            ps[s].root = 0;
            ps[s].state = State::Unused;
        } else {
            ps[s].state = State::Zombie;     // threads ainda rodando em outras CPUs
        }
    } else {
        ps[s].root = 0;                      // o espaco e do lider
        ps[s].state = State::Unused;
        if ps[l].state == State::Zombie && ps[l].members == 0 {
            if ps[l].root != 0 { arch::mmu::free_user_space(ps[l].root); }
            ps[l].root = 0;
            ps[l].state = State::Unused;
        }
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
    let _g = SCHED.guard();
    let s = cur_slot();
    let cur = unsafe { core::ptr::read_volatile(addr as *const u32) };
    if cur != val {
        unsafe { (*frame).set_ret(u64::MAX) };
        return;
    }
    let p = &mut procs()[s];
    p.state = State::FutexWait;
    p.futex_addr = addr;
    p.wake_at = if timeout_ms > 0 { arch::now() + time::ms_to_ticks(timeout_ms) } else { 0 };
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
    let ps = procs();
    let root = ps[s].root;
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
    if woken > 0 { kick_idle_cpu(); }
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
    let _g = SCHED.guard();
    let Some(s) = slot_of(pid) else { return false };
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
        let l = procs()[s].leader;
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
    arch::mmu::flush_tlb();
    Some(old)
}
