use crate::context::Context;

#[derive(Clone, Copy, PartialEq)]
pub enum State { Unused, Ready, Running }

#[derive(Clone, Copy)]
pub struct Process {
    pub pid: usize,
    pub state: State,
    pub ctx: Context,
}

impl Process {
    pub const fn empty() -> Self {
        Process { pid: 0, state: State::Unused, ctx: Context::zeroed() }
    }
}

const MAX_PROCS: usize = 4;
static mut PROCS: [Process; MAX_PROCS] = [Process::empty(); MAX_PROCS];
static mut CURRENT: usize = 0;

pub fn create(entry: u64, stack_top: u64) -> usize {
    unsafe {
        for i in 0..MAX_PROCS {
            if PROCS[i].state == State::Unused {
                PROCS[i].pid = i;
                PROCS[i].state = State::Ready;
                PROCS[i].ctx = Context::zeroed();
                PROCS[i].ctx.elr = entry;
                PROCS[i].ctx.sp_el0 = stack_top;
                PROCS[i].ctx.spsr = 0x0;
                return i;
            }
        }
        panic!("sem slots de processo");
    }
}

/// Proximo processo Ready (round-robin), ou None se nao houver outro.
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

/// Salva o contexto atual e troca para o proximo (yield cooperativo).
pub unsafe fn schedule(frame: *mut Context) {
    unsafe {
        PROCS[CURRENT].ctx = *frame;
        PROCS[CURRENT].state = State::Ready;

        if let Some(next) = pick_next() {
            CURRENT = next;
            PROCS[next].state = State::Running;
            *frame = PROCS[next].ctx;
        }
        // se None, era o unico processo: mantem o frame (continua nele)
    }
}

/// Mata o processo atual e troca para o proximo. Retorna o pid morto,
/// e false em `bool` se nao restou nenhum processo.
pub unsafe fn exit_current(frame: *mut Context) -> (usize, bool) {
    unsafe {
        let dead = CURRENT;
        PROCS[CURRENT].state = State::Unused;

        if let Some(next) = pick_next() {
            CURRENT = next;
            PROCS[next].state = State::Running;
            *frame = PROCS[next].ctx;
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
        PROCS[0].ctx
    }
}