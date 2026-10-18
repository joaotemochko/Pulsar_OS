//! Sockets locais: AF_UNIX (socketpair, bind com caminho ou nome abstrato,
//! listen/accept/connect) e o loopback TCP (127.0.0.1), que usa as mesmas
//! conexoes.
//!
//! Uma conexao tem duas pontas e uma fila por sentido. A fila e um anel de
//! bytes; no modo pacote (SOCK_SEQPACKET/SOCK_DGRAM) guarda tambem o tamanho
//! de cada mensagem, e cada recv devolve uma mensagem inteira. Descritores
//! enviados com SCM_RIGHTS viajam presos a posicao do fluxo onde a mensagem
//! comecou e sao instalados no processo que recebe.
//!
//! Tudo aqui supoe a trava FS presa (e chamado pelo file.rs).
use crate::file::{self, OpenFile};

const EAGAIN: i64 = 11;
const ENOMEM: i64 = 12;
const EPIPE: i64 = 32;
const EMSGSIZE: i64 = 90;
const EADDRINUSE: i64 = 98;
const ECONNREFUSED: i64 = 111;
const ENOBUFS: i64 = 105;

const CAP: u64 = 64 * 1024;
const PAGES: usize = 16;
const MSGQ: usize = 128;
const ATTQ: usize = 8;
/// Maximo de descritores numa mensagem.
pub const MAX_FDS: usize = 16;
const MAX_CONNS: usize = 64;
const MAX_LISTEN: usize = 16;
const BACKLOG: usize = 16;

#[derive(Clone, Copy)]
struct Att { pos: u64, n: usize, files: [OpenFile; MAX_FDS] }

#[derive(Clone, Copy)]
struct Queue {
    buf: u64,
    head: u64,
    len: u64,
    wpos: u64,           // bytes ja escritos (posicao absoluta)
    rpos: u64,           // bytes ja lidos
    msgs: [u32; MSGQ],   // modo pacote: tamanhos das mensagens na fila
    mhead: usize,
    mcount: usize,
    atts: [Att; ATTQ],
    natt: usize,
    eof: bool,           // shutdown(SHUT_WR) do outro lado
}

const NO_FILE: OpenFile = file::FREE_FILE;
const QEMPTY: Queue = Queue { buf: 0, head: 0, len: 0, wpos: 0, rpos: 0, msgs: [0; MSGQ], mhead: 0, mcount: 0,
                              atts: [Att { pos: 0, n: 0, files: [NO_FILE; MAX_FDS] }; ATTQ], natt: 0, eof: false };

#[derive(Clone, Copy)]
struct Conn {
    used: bool,
    packet: bool,
    q: [Queue; 2],       // q[e] = o que a ponta e le
    refs: [u32; 2],
    port: [u16; 2],      // loopback TCP: porta de cada ponta (0 = AF_UNIX)
}

const CEMPTY: Conn = Conn { used: false, packet: false, q: [QEMPTY; 2], refs: [0; 2], port: [0; 2] };
static mut CONNS: [Conn; MAX_CONNS] = [CEMPTY; MAX_CONNS];
fn conns() -> &'static mut [Conn; MAX_CONNS] { unsafe { &mut *(&raw mut CONNS) } }

/// Endereco ao qual um socket local se liga.
#[derive(Clone, Copy, PartialEq)]
pub enum Key { None, Ino(u32), Abstract([u8; 108], usize), Port(u16) }

#[derive(Clone, Copy)]
struct Listener { used: bool, refs: u32, packet: bool, key: Key, backlog: [usize; BACKLOG], n: usize }
const LEMPTY: Listener = Listener { used: false, refs: 0, packet: false, key: Key::None, backlog: [0; BACKLOG], n: 0 };
static mut LISTENERS: [Listener; MAX_LISTEN] = [LEMPTY; MAX_LISTEN];
fn listeners() -> &'static mut [Listener; MAX_LISTEN] { unsafe { &mut *(&raw mut LISTENERS) } }

fn key_r(c: usize, e: usize) -> u64 { 0x554E_0000_0000 | (c as u64) << 2 | e as u64 }
fn key_l(i: usize) -> u64 { 0x554C_0000_0000 | i as u64 }

// ------------------------------------------------------------ conexoes

/// Conexao nova com as duas pontas referenciadas uma vez. Retorna o indice.
pub fn conn_new(packet: bool) -> Result<usize, i64> {
    let c = conns().iter().position(|c| !c.used).ok_or(ENOMEM)?;
    let a = crate::frame_allocator::alloc_contig(PAGES).ok_or(ENOMEM)?;
    let Some(b) = crate::frame_allocator::alloc_contig(PAGES) else {
        crate::frame_allocator::free_contig(a, PAGES);
        return Err(ENOMEM);
    };
    let mut n = CEMPTY;
    n.used = true;
    n.packet = packet;
    n.q[0].buf = a;
    n.q[1].buf = b;
    n.refs = [1, 1];
    conns()[c] = n;
    Ok(c)
}

pub fn add_ref(c: usize, e: usize) { conns()[c].refs[e] += 1; }

/// Uma referencia a ponta `e` acabou. Sem referencias nas duas pontas, a
/// conexao some (e os descritores ainda em transito sao fechados).
pub fn drop_ref(c: usize, e: usize) {
    let k = &mut conns()[c];
    k.refs[e] -= 1;
    if k.refs[e] == 0 {
        file::push_wake(key_r(c, 1 - e));   // o outro lado ve fim/EPIPE
        file::push_wake(key_r(c, e));
    }
    if k.refs[0] == 0 && k.refs[1] == 0 {
        let pending: [Queue; 2] = k.q;
        for q in pending.iter() {
            crate::frame_allocator::free_contig(q.buf, PAGES);
        }
        *k = CEMPTY;
        for q in pending.iter() {
            for a in &q.atts[..q.natt] {
                for f in &a.files[..a.n] { file::drop_ref(*f); }
            }
        }
    }
}

pub fn set_ports(c: usize, a: u16, b: u16) { conns()[c].port = [a, b]; }
pub fn ports(c: usize, e: usize) -> (u16, u16) { let k = conns()[c]; (k.port[e], k.port[1 - e]) }

/// Le da ponta `e`. `fds` recebe os descritores anexados ao inicio do que
/// foi lido. Retorna (bytes, mensagem truncada?).
pub fn read(c: usize, e: usize, dst: u64, len: u64, nonblock: bool,
            fds: &mut ([OpenFile; MAX_FDS], usize)) -> Result<(u64, bool), i64> {
    let k = &mut conns()[c];
    let peer_gone = k.refs[1 - e] == 0;
    let packet = k.packet;
    let q = &mut k.q[e];
    if q.len == 0 {
        if peer_gone || q.eof { return Ok((0, false)); }
        if nonblock { return Err(EAGAIN); }
        return Err(file::would_block(key_r(c, e)));
    }
    // descritores presos a esta posicao
    if q.natt > 0 && q.atts[0].pos == q.rpos {
        let a = q.atts[0];
        fds.0[..a.n].copy_from_slice(&a.files[..a.n]);
        fds.1 = a.n;
        q.atts.copy_within(1..q.natt, 0);
        q.natt -= 1;
    }
    let (take, consume, trunc) = if packet {
        let m = q.msgs[q.mhead] as u64;
        q.mhead = (q.mhead + 1) % MSGQ;
        q.mcount -= 1;
        (len.min(m), m, len < m)
    } else {
        let mut n = len.min(q.len);
        // nao atravessa o inicio de outra mensagem com descritores
        if q.natt > 0 && q.atts[0].pos > q.rpos { n = n.min(q.atts[0].pos - q.rpos); }
        (n, n, false)
    };
    let mut done = 0;
    while done < take {
        let at = (q.head + done) % CAP;
        let chunk = (take - done).min(CAP - at);
        unsafe { core::ptr::copy_nonoverlapping((q.buf + at) as *const u8, (dst + done) as *mut u8, chunk as usize) };
        done += chunk;
    }
    q.head = (q.head + consume) % CAP;
    q.len -= consume;
    q.rpos += consume;
    file::push_wake(key_r(c, 1 - e));       // quem escreve para ca tem espaco
    Ok((take, trunc))
}

/// Escreve pela ponta `e` (na fila do outro lado), anexando `files` (as
/// referencias passam para a fila).
pub fn write(c: usize, e: usize, src: u64, len: u64, nonblock: bool, files: &[OpenFile]) -> Result<u64, i64> {
    let k = &mut conns()[c];
    if k.refs[1 - e] == 0 { return Err(EPIPE); }
    let packet = k.packet;
    let q = &mut k.q[1 - e];
    let space = CAP - q.len;
    if packet && len > CAP { return Err(EMSGSIZE); }
    let full = if packet { space < len || q.mcount == MSGQ } else { space == 0 };
    if full {
        if nonblock { return Err(EAGAIN); }
        return Err(file::would_block(key_r(c, 1 - e)));
    }
    if !files.is_empty() {
        if q.natt == ATTQ || files.len() > MAX_FDS { return Err(ENOBUFS); }
        let mut a = Att { pos: q.wpos, n: files.len(), files: [NO_FILE; MAX_FDS] };
        a.files[..files.len()].copy_from_slice(files);
        q.atts[q.natt] = a;
        q.natt += 1;
    }
    let n = if packet { len } else { len.min(space) };
    let mut done = 0;
    while done < n {
        let at = (q.head + q.len + done) % CAP;
        let chunk = (n - done).min(CAP - at);
        unsafe { core::ptr::copy_nonoverlapping((src + done) as *const u8, (q.buf + at) as *mut u8, chunk as usize) };
        done += chunk;
    }
    q.len += n;
    q.wpos += n;
    if packet {
        q.msgs[(q.mhead + q.mcount) % MSGQ] = n as u32;
        q.mcount += 1;
    }
    file::push_wake(key_r(c, 1 - e));
    Ok(n)
}

/// shutdown: SHUT_RD 0, SHUT_WR 1, SHUT_RDWR 2.
pub fn shutdown(c: usize, e: usize, how: u64) {
    let k = &mut conns()[c];
    if how != 0 { k.q[1 - e].eof = true; file::push_wake(key_r(c, 1 - e)); }
    if how != 1 { k.q[e].eof = true; file::push_wake(key_r(c, e)); }
}

/// (ha o que ler ou fim, cabe mais, outro lado fechou)
pub fn ready(c: usize, e: usize) -> (bool, bool, bool) {
    let k = &conns()[c];
    let gone = k.refs[1 - e] == 0;
    let q = &k.q[e];
    let out = &k.q[1 - e];
    let writable = !gone && if k.packet { out.len < CAP && out.mcount < MSGQ } else { out.len < CAP };
    (q.len > 0 || gone || q.eof, writable, gone)
}

// ------------------------------------------------------------ listen / accept

fn find_listener(key: &Key) -> Option<usize> {
    listeners().iter().position(|l| l.used && l.key == *key)
}

/// listen(): registra o endereco. EADDRINUSE se ja ha alguem nele.
pub fn listen(key: Key, packet: bool) -> Result<usize, i64> {
    if find_listener(&key).is_some() { return Err(EADDRINUSE); }
    let i = listeners().iter().position(|l| !l.used).ok_or(ENOMEM)?;
    listeners()[i] = Listener { used: true, refs: 1, packet, key, ..LEMPTY };
    Ok(i)
}

pub fn listener_ref(i: usize, delta: i32) {
    let l = &mut listeners()[i];
    l.refs = (l.refs as i32 + delta) as u32;
    if l.refs == 0 {
        // conexoes nunca aceitas: o lado do servidor fecha
        let pending = *l;
        *l = LEMPTY;
        for &c in &pending.backlog[..pending.n] { drop_ref(c, 1); }
    }
}

pub fn listener_ready(i: usize) -> bool { listeners()[i].n > 0 }

pub fn listener_key(i: usize) -> Key { listeners()[i].key }

/// connect(): cria a conexao e a deixa na fila do listener. Retorna o
/// indice; o cliente fica com a ponta 0, o servidor (accept) com a 1.
pub fn connect(key: &Key, packet: bool) -> Result<usize, i64> {
    let li = find_listener(key).ok_or(ECONNREFUSED)?;
    if listeners()[li].packet != packet { return Err(ECONNREFUSED); }
    if listeners()[li].n == BACKLOG { return Err(EAGAIN); }
    let c = conn_new(packet)?;
    let l = &mut listeners()[li];
    l.backlog[l.n] = c;
    l.n += 1;
    file::push_wake(key_l(li));
    Ok(c)
}

/// accept(): tira a proxima conexao da fila (ponta 1 do servidor).
pub fn accept(i: usize, nonblock: bool) -> Result<usize, i64> {
    let l = &mut listeners()[i];
    if l.n == 0 {
        if nonblock { return Err(EAGAIN); }
        return Err(file::would_block(key_l(i)));
    }
    let c = l.backlog[0];
    l.backlog.copy_within(1..l.n, 0);
    l.n -= 1;
    Ok(c)
}
