use crate::context::Context;

#[derive(Clone, Copy, PartialEq)]
pub enum State {
    Unused,
    Ready,
    Running,
    SendBlocked, // esperando o receptor aceitar a mensagem
    RecvBlocked, // esperando alguem enviar
    ReplyBlocked, // enviou, esperando a resposta
}

/// Uma mensagem de IPC: 6 palavras de payload + tag. Suficiente para
/// requisicoes de driver (opcode + args) sem alocacao dinamica.
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

#[derive(Clone, Copy)]
pub struct Process {
    pub pid: usize,
    pub state: State,
    pub ctx: Context,
    pub l0: u64,
    pub asid: u16,
    // IPC
    pub inbox: Message,      // mensagem recebida (visivel ao receptor)
    pub reply: Message,      // resposta (visivel ao emissor apos reply)
    pub peer: usize,         // com quem esta em rendezvous
    pub send_to: usize,      // destino pretendido de um send bloqueado
    pub msg_ptr: u64,        // VA (no espaco do processo) do buffer de msg da syscall
}

impl Process {
    pub const fn empty() -> Self {
        Process {
            pid: 0, state: State::Unused, ctx: Context::zeroed(), l0: 0, asid: 0,
            inbox: Message::zero(), reply: Message::zero(), peer: 0, send_to: 0,
            msg_ptr: 0,
        }
    }
}

const MAX_PROCS: usize = 8;
pub const INVALID: usize = usize::MAX;
static mut PROCS: [Process; MAX_PROCS] = [Process::empty(); MAX_PROCS];
static mut CURRENT: usize = 0;

pub fn create(entry: u64, stack_top: u64, l0: u64, asid: u16) -> usize {
    unsafe {
        for i in 0..MAX_PROCS {
            if PROCS[i].state == State::Unused {
                PROCS[i] = Process::empty();
                PROCS[i].pid = i;
                PROCS[i].state = State::Ready;
                PROCS[i].ctx = Context::zeroed();
                PROCS[i].ctx.elr = entry;
                PROCS[i].ctx.sp_el0 = stack_top;
                PROCS[i].ctx.spsr = 0x0;
                PROCS[i].l0 = l0;
                PROCS[i].asid = asid;
                return i;
            }
        }
        panic!("sem slots de processo");
    }
}

pub fn current() -> usize {
    unsafe { CURRENT }
}

pub fn current_l0() -> u64 {
    unsafe { PROCS[CURRENT].l0 }
}

/// Existe um processo com este pid ativo?
pub fn is_alive(pid: usize) -> bool {
    unsafe { pid < MAX_PROCS && PROCS[pid].state != State::Unused }
}

fn runnable(idx: usize) -> bool {
    unsafe { matches!(PROCS[idx].state, State::Ready | State::Running) }
}

/// Proximo processo executavel (round-robin), ou None.
fn pick_next() -> Option<usize> {
    unsafe {
        for offset in 1..=MAX_PROCS {
            let idx = (CURRENT + offset) % MAX_PROCS;
            if PROCS[idx].state == State::Ready {
                return Some(idx);
            }
        }
        if PROCS[CURRENT].state == State::Running {
            Some(CURRENT)
        } else {
            None
        }
    }
}

/// Troca para `next`, aplicando o espaco de enderecos dele.
unsafe fn switch_to(frame: *mut Context, next: usize) {
    unsafe {
        CURRENT = next;
        PROCS[next].state = State::Running;
        *frame = PROCS[next].ctx;
        crate::mmu::switch_space(PROCS[next].l0, PROCS[next].asid);
    }
}

/// Reescalona para o proximo executavel. Se ninguem estiver pronto (todos
/// bloqueados em IPC), roda o idle (WFE ate um IRQ/troca) — mas aqui
/// simplesmente mantemos o frame e deixamos o timer reescalonar.
unsafe fn reschedule(frame: *mut Context) {
    unsafe {
        if let Some(next) = pick_next() {
            switch_to(frame, next);
        }
        // Se None: nenhum processo pronto. O frame atual (bloqueado) nao
        // deve rodar; sinalizamos via spsr para o retorno cair num loop
        // seguro. Na pratica, o shell nunca bloqueia, entao sempre ha um
        // processo pronto neste sistema.
    }
}

/// Salva o contexto atual e troca para o proximo (yield cooperativo).
pub unsafe fn schedule(frame: *mut Context) {
    unsafe {
        if PROCS[CURRENT].state == State::Running {
            PROCS[CURRENT].ctx = *frame;
            PROCS[CURRENT].state = State::Ready;
        } else {
            // processo bloqueou-se (IPC): salva o contexto mas nao volta a Ready
            PROCS[CURRENT].ctx = *frame;
        }
        reschedule(frame);
    }
}

// ------------------------------------------------------------------ IPC

/// SEND sincrono: entrega `msg` a `dst` e bloqueia ate o reply. `msg_ptr`
/// e o VA do buffer da syscall (a resposta sera escrita la, e x0=0, quando
/// este processo voltar a rodar — feito por `deliver_reply`). Retorna
/// false se `dst` invalido (falha imediata, sem bloquear).
pub unsafe fn ipc_send(frame: *mut Context, dst: usize, msg: Message, msg_ptr: u64) -> bool {
    unsafe {
        if !is_alive(dst) || dst == CURRENT {
            return false;
        }
        PROCS[CURRENT].ctx = *frame;
        PROCS[CURRENT].msg_ptr = msg_ptr;

        if PROCS[dst].state == State::RecvBlocked {
            PROCS[dst].inbox = msg;
            PROCS[dst].peer = CURRENT;
            deliver_recv(dst);       // escreve a msg no ctx salvo do receptor
            PROCS[dst].state = State::Ready;
            PROCS[CURRENT].state = State::ReplyBlocked;
            PROCS[CURRENT].peer = dst;
        } else {
            PROCS[CURRENT].state = State::SendBlocked;
            PROCS[CURRENT].send_to = dst;
            PROCS[CURRENT].inbox = msg;
        }
        reschedule(frame);
        true
    }
}

/// Escreve, no contexto SALVO do processo `p`, a mensagem recebida e o
/// remetente (x0). Chamado quando `p` sai de RecvBlocked.
unsafe fn deliver_recv(p: usize) {
    unsafe {
        let ptr = PROCS[p].msg_ptr;
        if ptr != 0 {
            write_msg_to(p, ptr, &PROCS[p].inbox);
        }
        PROCS[p].ctx.x[0] = PROCS[p].peer as u64;
    }
}

/// Escreve a resposta no contexto salvo do emissor `p` (que estava em
/// ReplyBlocked) e zera x0. Chamado no reply.
unsafe fn deliver_reply(p: usize) {
    unsafe {
        let ptr = PROCS[p].msg_ptr;
        if ptr != 0 {
            write_msg_to(p, ptr, &PROCS[p].reply);
        }
        PROCS[p].ctx.x[0] = 0;
    }
}

/// Escreve uma Message no espaco de enderecos do processo `p`. Como as
/// paginas de usuario de `p` estao mapeadas via seu proprio L0, e o buffer
/// esta na RAM (identity no kernel), traduzimos pelo walk das tabelas de p.
unsafe fn write_msg_to(p: usize, va: u64, m: &Message) {
    unsafe {
        // Traduz o VA do processo p para PA usando as tabelas dele.
        if let Some(pa) = crate::mmu::translate(PROCS[p].l0, va) {
            core::ptr::write_unaligned(pa as *mut u64, m.tag);
            for i in 0..6 {
                core::ptr::write_unaligned((pa + 8 + (i as u64) * 8) as *mut u64, m.data[i]);
            }
        }
    }
}

/// RECV sincrono: bloqueia ate receber. `msg_ptr` = VA do buffer da
/// syscall (recebe a mensagem). O remetente sai em x0 quando o processo
/// retorna (feito aqui inline se ha emissor pronto, ou por deliver_recv).
pub unsafe fn ipc_recv(frame: *mut Context, msg_ptr: u64) {
    unsafe {
        PROCS[CURRENT].ctx = *frame;
        PROCS[CURRENT].msg_ptr = msg_ptr;

        for offset in 1..=MAX_PROCS {
            let s = (CURRENT + offset) % MAX_PROCS;
            if PROCS[s].state == State::SendBlocked && PROCS[s].send_to == CURRENT {
                // Casamento imediato: escreve msg no frame ATUAL (estamos
                // rodando como o receptor) e retorna sem bloquear.
                PROCS[CURRENT].inbox = PROCS[s].inbox;
                PROCS[CURRENT].peer = s;
                PROCS[s].state = State::ReplyBlocked;
                PROCS[s].peer = CURRENT;
                if msg_ptr != 0 {
                    write_msg_inline(frame, msg_ptr, &PROCS[CURRENT].inbox);
                }
                (*frame).x[0] = s as u64;
                return;
            }
        }

        // Ninguem esperando: bloqueia. deliver_recv preenchera o ctx salvo.
        PROCS[CURRENT].state = State::RecvBlocked;
        reschedule(frame);
    }
}

/// RECV nao-bloqueante: se ha um emissor esperando, entrega e retorna o
/// pid; senao retorna -1 imediatamente (x0). O WM usa isto para atender
/// apps sem parar seu loop de composicao.
pub unsafe fn ipc_try_recv(frame: *mut Context, msg_ptr: u64) {
    unsafe {
        for offset in 1..=MAX_PROCS {
            let s = (CURRENT + offset) % MAX_PROCS;
            if PROCS[s].state == State::SendBlocked && PROCS[s].send_to == CURRENT {
                PROCS[CURRENT].inbox = PROCS[s].inbox;
                PROCS[CURRENT].peer = s;
                PROCS[s].state = State::ReplyBlocked;
                PROCS[s].peer = CURRENT;
                if msg_ptr != 0 {
                    write_msg_inline(frame, msg_ptr, &PROCS[CURRENT].inbox);
                }
                (*frame).x[0] = s as u64;
                return;
            }
        }
        // vazio: retorna -1 sem bloquear
        (*frame).x[0] = u64::MAX;
    }
}

/// Escreve uma Message diretamente no frame ATIVO (o processo corrente),
/// cujo espaco de enderecos ja esta em TTBR0 — pode usar o VA direto.
unsafe fn write_msg_inline(_frame: *mut Context, va: u64, m: &Message) {
    unsafe {
        core::ptr::write_unaligned(va as *mut u64, m.tag);
        for i in 0..6 {
            core::ptr::write_unaligned((va + 8 + (i as u64) * 8) as *mut u64, m.data[i]);
        }
    }
}

/// REPLY: responde ao ultimo remetente e o desbloqueia.
pub unsafe fn ipc_reply(frame: *mut Context, to: usize, msg: Message) {
    unsafe {
        PROCS[CURRENT].ctx = *frame;
        if is_alive(to) && PROCS[to].state == State::ReplyBlocked && PROCS[to].peer == CURRENT {
            PROCS[to].reply = msg;
            deliver_reply(to);       // escreve a resposta no ctx salvo do emissor
            PROCS[to].state = State::Ready;
        }
        PROCS[CURRENT].state = State::Ready;
        reschedule(frame);
    }
}

/// Mata o processo atual e troca para o proximo.
pub unsafe fn exit_current(frame: *mut Context) -> (usize, bool) {
    unsafe {
        let dead = CURRENT;
        PROCS[CURRENT].state = State::Unused;

        // Desbloqueia quem estava em rendezvous com o morto (evita deadlock).
        for i in 0..MAX_PROCS {
            if PROCS[i].state == State::ReplyBlocked && PROCS[i].peer == dead {
                PROCS[i].reply = Message::zero();
                PROCS[i].state = State::Ready;
            }
            if PROCS[i].state == State::SendBlocked && PROCS[i].send_to == dead {
                PROCS[i].reply = Message::zero();
                PROCS[i].state = State::Ready;
            }
        }

        if let Some(next) = pick_next() {
            switch_to(frame, next);
            (dead, true)
        } else {
            (dead, false)
        }
    }
}

pub unsafe fn first_context() -> Context {
    unsafe {
        CURRENT = 0;
        PROCS[0].state = State::Running;
        crate::mmu::switch_space(PROCS[0].l0, PROCS[0].asid);
        PROCS[0].ctx
    }
}

/// Preempcao pelo timer: so troca se o atual ainda estiver rodando
/// (processos bloqueados em IPC nao voltam a Ready por tick).
pub unsafe fn preempt(frame: *mut Context) {
    unsafe {
        let _ = runnable(CURRENT);
        schedule(frame);
    }
}
