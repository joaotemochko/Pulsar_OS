//! Driver virtio-net + pilha IPv4 com sockets TCP e UDP.
//!
//! Com `-netdev user` (SLIRP) o QEMU faz NAT para a rede do host: gateway
//! 10.0.2.2, DNS 10.0.2.3, nosso IP 10.0.2.15. Tudo sai pelo MAC do gateway.
//!
//! O kernel oferece so transporte; protocolos de aplicacao (DNS, HTTP, TLS)
//! vivem no userspace, como num microkernel deve ser.
//!
//! TCP: ate MAX_TCP conexoes, janela de recepcao de 64 KiB (anunciada
//! conforme o espaco livre, com atualizacao de janela quando o app le),
//! controle de fluxo pela janela do par, retransmissao por tempo do que nao
//! foi confirmado e descarte (com ACK duplicado) de segmentos fora de ordem.
//! UDP: fila global de datagramas recebidos, consumida por porta destino.
//!
//! A placa e lida por polling: cada syscall de rede drena a fila RX antes
//! de agir. Toda a pilha fica sob NET_LOCK (trava folha).

use crate::uart::Uart;
use crate::virtio::{self, QueueMem, VirtQueue};
use core::fmt::Write;

const VIRTIO_ID_NET: u32 = 1;

pub const OUR_IP: [u8; 4] = [10, 0, 2, 15];
pub const GW_IP: [u8; 4] = [10, 0, 2, 2];

static mut MAC: [u8; 6] = [0x52, 0x54, 0x00, 0x12, 0x34, 0x56];

/// virtio_net_hdr (12 bytes com VERSION_1) antes de cada frame
const NET_HDR_LEN: usize = 12;
const NRX: usize = 64;   // = QSIZE: rajadas de varias conexoes em paralelo

#[repr(C, align(4096))]
struct PktBuf([u8; 2048]);
static mut TXBUF: PktBuf = PktBuf([0; 2048]);
static mut RXBUFS: [PktBuf; NRX] = [const { PktBuf([0; 2048]) }; NRX];

static mut RXQ_MEM: QueueMem = QueueMem::zeroed();
static mut TXQ_MEM: QueueMem = QueueMem::zeroed();

struct Net {
    rxq: VirtQueue,
    txq: VirtQueue,
}
static mut NET: Option<Net> = None;

/// Trava da pilha de rede (folha: nunca pega outra trava).
pub static NET_LOCK: crate::sync::SpinLock = crate::sync::SpinLock::new();

static mut GW_MAC: Option<[u8; 6]> = None;

/// Inicializa o virtio-net. Retorna true se achou o dispositivo.
pub fn init() -> bool {
    let mut serial = Uart;
    let Some(base) = virtio::probe(VIRTIO_ID_NET) else {
        let _ = write!(serial, "[net] nenhum virtio-net encontrado\n");
        return false;
    };
    if !virtio::init_device(base) {
        let _ = write!(serial, "[net] init_device falhou\n");
        return false;
    }
    let mut mac = [0u8; 6];
    for i in 0..6 {
        mac[i] = virtio::config_read8(base, i as u64);
    }
    unsafe { MAC = mac; }

    let rxq = unsafe { VirtQueue::setup(base, 0, &raw mut RXQ_MEM) };
    let txq = unsafe { VirtQueue::setup(base, 1, &raw mut TXQ_MEM) };
    let (Some(mut rxq), Some(txq)) = (rxq, txq) else {
        let _ = write!(serial, "[net] setup de filas falhou\n");
        return false;
    };
    virtio::driver_ok(base);
    for i in 0..NRX {
        let addr = unsafe { core::ptr::addr_of!(RXBUFS[i].0) } as u64;
        rxq.post_recv(i as u16, addr, 2048);
    }
    unsafe { NET = Some(Net { rxq, txq }); }

    let _ = write!(serial, "[net] virtio-net em {:#x}, MAC {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}\n",
        base, mac[0], mac[1], mac[2], mac[3], mac[4], mac[5]);
    let _ = write!(serial, "[net] IP {}.{}.{}.{}  gateway {}.{}.{}.{}\n",
        OUR_IP[0], OUR_IP[1], OUR_IP[2], OUR_IP[3], GW_IP[0], GW_IP[1], GW_IP[2], GW_IP[3]);
    true
}

pub fn is_up() -> bool {
    unsafe { (*core::ptr::addr_of!(NET)).is_some() }
}

fn net() -> Option<&'static mut Net> {
    unsafe { (*core::ptr::addr_of_mut!(NET)).as_mut() }
}

// ------------------------------------------------------------ Ethernet/ARP

fn send_frame(dst_mac: &[u8; 6], ethertype: u16, payload: &[u8]) {
    let Some(n) = net() else { return };
    unsafe {
        let buf = &mut *core::ptr::addr_of_mut!(TXBUF.0);
        buf[..NET_HDR_LEN].fill(0);
        let mut p = NET_HDR_LEN;
        buf[p..p + 6].copy_from_slice(dst_mac);
        buf[p + 6..p + 12].copy_from_slice(&*core::ptr::addr_of!(MAC));
        buf[p + 12] = (ethertype >> 8) as u8;
        buf[p + 13] = ethertype as u8;
        p += 14;
        buf[p..p + payload.len()].copy_from_slice(payload);
        p += payload.len();
        n.txq.request_sync(&[(core::ptr::addr_of!(TXBUF.0) as u64, p as u32, false)]);
    }
}

fn checksum_acc(mut sum: u32, data: &[u8]) -> u32 {
    let mut i = 0;
    while i + 1 < data.len() {
        sum += ((data[i] as u32) << 8) | data[i + 1] as u32;
        i += 2;
    }
    if i < data.len() { sum += (data[i] as u32) << 8; }
    sum
}

fn fold(mut sum: u32) -> u16 {
    while sum >> 16 != 0 { sum = (sum & 0xFFFF) + (sum >> 16); }
    !(sum as u16)
}

fn arp(op: u16, target_mac: &[u8; 6], target_ip: &[u8; 4], dst: &[u8; 6]) {
    let mut a = [0u8; 28];
    a[1] = 1;
    a[2] = 0x08;
    a[4] = 6;
    a[5] = 4;
    a[7] = op as u8;
    a[8..14].copy_from_slice(unsafe { &*core::ptr::addr_of!(MAC) });
    a[14..18].copy_from_slice(&OUR_IP);
    a[18..24].copy_from_slice(target_mac);
    a[24..28].copy_from_slice(target_ip);
    send_frame(dst, 0x0806, &a);
}

static mut LAST_ARP_FAIL: u64 = 0;

/// Resolve o MAC do gateway (rapido, com cache de falha para nao travar o sistema).
pub fn resolve_gateway() -> Option<[u8; 6]> {
    if let Some(m) = unsafe { GW_MAC } { return Some(m); }
    let now = crate::time::now_ms();
    if unsafe { now < LAST_ARP_FAIL + 5000 } { return None; }
    arp(1, &[0; 6], &GW_IP, &[0xFF; 6]);
    let t0 = crate::time::now_ms();
    while crate::time::now_ms() - t0 < 50 {
        poll();
        if let Some(m) = unsafe { GW_MAC } {
            let _ = write!(Uart, "[net] gateway {}.{}.{}.{} -> {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}\n",
                GW_IP[0], GW_IP[1], GW_IP[2], GW_IP[3], m[0], m[1], m[2], m[3], m[4], m[5]);
            return Some(m);
        }
        core::hint::spin_loop();
    }
    unsafe { LAST_ARP_FAIL = crate::time::now_ms(); }
    let _ = write!(Uart, "[net] ARP do gateway sem resposta (em cache por 5s)\n");
    None
}

/// Envia um pacote IPv4 (cabecalho montado aqui) pelo gateway.
fn send_ip(proto: u8, dst: &[u8; 4], l4: &[u8]) -> bool {
    let Some(gw) = resolve_gateway() else { return false };
    let mut pkt = [0u8; 1500];
    let total = 20 + l4.len();
    if total > pkt.len() { return false; }
    pkt[0] = 0x45;
    pkt[2] = (total >> 8) as u8;
    pkt[3] = total as u8;
    pkt[6] = 0x40; // nao fragmentar
    pkt[8] = 64;
    pkt[9] = proto;
    pkt[12..16].copy_from_slice(&OUR_IP);
    pkt[16..20].copy_from_slice(dst);
    let c = fold(checksum_acc(0, &pkt[..20]));
    pkt[10] = (c >> 8) as u8;
    pkt[11] = c as u8;
    pkt[20..total].copy_from_slice(l4);
    send_frame(&gw, 0x0800, &pkt[..total]);
    true
}

fn l4_checksum(dst: &[u8; 4], proto: u8, l4: &[u8]) -> u16 {
    let mut sum = checksum_acc(0, &OUR_IP);
    sum = checksum_acc(sum, dst);
    sum += proto as u32 + l4.len() as u32;
    fold(checksum_acc(sum, l4))
}

/// Drena a fila RX processando os frames recebidos (nao bloqueia).
pub fn poll() {
    let Some(n) = net() else { return };
    while let Some((idx, len)) = n.rxq.poll_used() {
        let buf = unsafe { &*core::ptr::addr_of!(RXBUFS[idx as usize].0) };
        if len as usize > NET_HDR_LEN + 14 {
            handle_frame(&buf[NET_HDR_LEN..len as usize]);
        }
        let addr = unsafe { core::ptr::addr_of!(RXBUFS[idx as usize].0) } as u64;
        n.rxq.post_recv(idx, addr, 2048);
    }
    tcp_timers();
}

fn handle_frame(f: &[u8]) {
    let et = ((f[12] as u16) << 8) | f[13] as u16;
    let p = &f[14..];
    match et {
        0x0806 if p.len() >= 28 => {
            let op = ((p[6] as u16) << 8) | p[7] as u16;
            let mut smac = [0u8; 6];
            smac.copy_from_slice(&p[8..14]);
            let sip = [p[14], p[15], p[16], p[17]];
            if sip == GW_IP { unsafe { GW_MAC = Some(smac); } }
            // alguem perguntando por nos: responde
            if op == 1 && p[24..28] == OUR_IP {
                arp(2, &smac, &sip, &smac);
            }
        }
        0x0800 if p.len() >= 20 => {
            let ihl = ((p[0] & 0x0F) as usize) * 4;
            let total = (((p[2] as usize) << 8) | p[3] as usize).min(p.len());
            if total <= ihl { return; }
            let src = [p[12], p[13], p[14], p[15]];
            let l4 = &p[ihl..total];
            match p[9] {
                6 => tcp_input(&src, l4),
                17 => udp_input(&src, l4),
                _ => {}
            }
        }
        _ => {}
    }
}

// --------------------------------------------------------------------- UDP

const UDP_Q: usize = 8;
const UDP_MAX: usize = 1472;

#[derive(Clone, Copy)]
struct Datagram {
    used: bool,
    src_ip: [u8; 4],
    src_port: u16,
    dst_port: u16,
    len: usize,
    data: [u8; UDP_MAX],
}

static mut UDPQ: [Datagram; UDP_Q] =
    [Datagram { used: false, src_ip: [0; 4], src_port: 0, dst_port: 0, len: 0, data: [0; UDP_MAX] }; UDP_Q];
static mut UDP_NEXT: usize = 0;

fn udp_input(src: &[u8; 4], u: &[u8]) {
    if u.len() < 8 { return; }
    let sport = ((u[0] as u16) << 8) | u[1] as u16;
    let dport = ((u[2] as u16) << 8) | u[3] as u16;
    let data = &u[8..];
    let q = unsafe { &mut *core::ptr::addr_of_mut!(UDPQ) };
    // fila circular: se cheia, sobrescreve o mais antigo
    let k = unsafe { UDP_NEXT };
    unsafe { UDP_NEXT = (UDP_NEXT + 1) % UDP_Q; }
    let n = data.len().min(UDP_MAX);
    q[k].used = true;
    q[k].src_ip = *src;
    q[k].src_port = sport;
    q[k].dst_port = dport;
    q[k].len = n;
    q[k].data[..n].copy_from_slice(&data[..n]);
}

/// Envia um datagrama UDP.
pub fn udp_send(dst_ip: &[u8; 4], dst_port: u16, src_port: u16, data: &[u8]) -> bool {
    let mut u = [0u8; 8 + UDP_MAX];
    if data.len() > UDP_MAX { return false; }
    let len = 8 + data.len();
    u[0] = (src_port >> 8) as u8;
    u[1] = src_port as u8;
    u[2] = (dst_port >> 8) as u8;
    u[3] = dst_port as u8;
    u[4] = (len >> 8) as u8;
    u[5] = len as u8;
    u[8..len].copy_from_slice(data);
    let c = l4_checksum(dst_ip, 17, &u[..len]);
    u[6] = (c >> 8) as u8;
    u[7] = c as u8;
    send_ip(17, dst_ip, &u[..len])
}

/// Retira o datagrama mais antigo destinado a `port`: (ip, porta, bytes).
pub fn udp_recv(port: u16, out: &mut [u8]) -> Option<([u8; 4], u16, usize)> {
    poll();
    let q = unsafe { &mut *core::ptr::addr_of_mut!(UDPQ) };
    let start = unsafe { UDP_NEXT };
    for k in 0..UDP_Q {
        let d = &mut q[(start + k) % UDP_Q];
        if d.used && d.dst_port == port {
            d.used = false;
            let n = d.len.min(out.len());
            out[..n].copy_from_slice(&d.data[..n]);
            return Some((d.src_ip, d.src_port, n));
        }
    }
    None
}

// --------------------------------------------------------------------- TCP

pub const MAX_TCP: usize = 24;   // o Vela abre varias conexoes e guarda outras ociosas (keep-alive)
const RXCAP: usize = 64 * 1024;
const TXCAP: usize = 16 * 1024;
const RTO_MS: u64 = 400;
const MAX_RETRIES: u32 = 12;

const FIN: u8 = 0x01;
const SYN: u8 = 0x02;
const RST: u8 = 0x04;
const PSH: u8 = 0x08;
const ACK: u8 = 0x10;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TcpState { Free, SynSent, Established, PeerClosed, Error }

struct Tcp {
    state: TcpState,
    lport: u16,
    rip: [u8; 4],
    rport: u16,
    /// primeiro byte nao confirmado / proximo a enviar / proximo esperado
    snd_una: u32,
    snd_nxt: u32,
    rcv_nxt: u32,
    peer_wnd: u32,
    mss: usize,
    /// bytes a partir de snd_una (enviados e nao confirmados + nao enviados)
    tx: [u8; TXCAP],
    tx_len: usize,
    /// anel de recepcao
    rx: [u8; RXCAP],
    rx_head: usize,
    rx_len: usize,
    /// janela anunciada no ultimo segmento
    adv_wnd: usize,
    last_tx: u64,
    retries: u32,
    syn_pending: bool,
}

const TCP_INIT: Tcp = Tcp {
    state: TcpState::Free, lport: 0, rip: [0; 4], rport: 0, snd_una: 0, snd_nxt: 0, rcv_nxt: 0,
    peer_wnd: 0, mss: 0, tx: [0; TXCAP], tx_len: 0, rx: [0; RXCAP], rx_head: 0, rx_len: 0,
    adv_wnd: 0, last_tx: 0, retries: 0, syn_pending: false,
};

static mut TCPS: [Tcp; MAX_TCP] = [TCP_INIT; MAX_TCP];
static mut NEXT_PORT: u16 = 49152;

fn tcps() -> &'static mut [Tcp; MAX_TCP] {
    unsafe { &mut *core::ptr::addr_of_mut!(TCPS) }
}

/// Monta e envia um segmento com `flags`, numero de sequencia `seq` e dados.
fn tcp_segment(t: &mut Tcp, flags: u8, seq: u32, data: &[u8]) {
    let opt_len = if flags & SYN != 0 { 4 } else { 0 };
    let hdr = 20 + opt_len;
    let mut s = [0u8; 1480];
    let len = hdr + data.len();
    s[0] = (t.lport >> 8) as u8;
    s[1] = t.lport as u8;
    s[2] = (t.rport >> 8) as u8;
    s[3] = t.rport as u8;
    s[4..8].copy_from_slice(&seq.to_be_bytes());
    let ack = if flags & ACK != 0 { t.rcv_nxt } else { 0 };
    s[8..12].copy_from_slice(&ack.to_be_bytes());
    s[12] = ((hdr / 4) as u8) << 4;
    s[13] = flags;
    let wnd = (RXCAP - t.rx_len).min(65535);
    t.adv_wnd = wnd;
    s[14] = (wnd >> 8) as u8;
    s[15] = wnd as u8;
    if opt_len > 0 {
        // MSS = 1460
        s[20] = 2;
        s[21] = 4;
        s[22] = 0x05;
        s[23] = 0xB4;
    }
    s[hdr..len].copy_from_slice(data);
    let c = l4_checksum(&t.rip, 6, &s[..len]);
    s[16] = (c >> 8) as u8;
    s[17] = c as u8;
    send_ip(6, &t.rip, &s[..len]);
}

/// Envia o que couber na janela do par (dados ainda nao enviados).
fn tcp_output(t: &mut Tcp) {
    if t.state != TcpState::Established && t.state != TcpState::PeerClosed { return; }
    loop {
        let in_flight = t.snd_nxt.wrapping_sub(t.snd_una) as usize;
        if in_flight >= t.tx_len { break; }
        let wnd = (t.peer_wnd as usize).max(1);
        if in_flight >= wnd { break; }
        let n = (t.tx_len - in_flight).min(t.mss).min(wnd - in_flight);
        let mut chunk = [0u8; 1460];
        chunk[..n].copy_from_slice(&t.tx[in_flight..in_flight + n]);
        let seq = t.snd_nxt;
        tcp_segment(t, ACK | PSH, seq, &chunk[..n]);
        t.snd_nxt = t.snd_nxt.wrapping_add(n as u32);
        t.last_tx = crate::time::now_ms();
    }
}

fn tcp_input(src: &[u8; 4], s: &[u8]) {
    if s.len() < 20 { return; }
    let sport = ((s[0] as u16) << 8) | s[1] as u16;
    let dport = ((s[2] as u16) << 8) | s[3] as u16;
    let Some(t) = tcps().iter_mut().find(|t| t.state != TcpState::Free && t.lport == dport
        && t.rport == sport && t.rip == *src) else { return };
    let seq = u32::from_be_bytes([s[4], s[5], s[6], s[7]]);
    let ackn = u32::from_be_bytes([s[8], s[9], s[10], s[11]]);
    let off = ((s[12] >> 4) as usize) * 4;
    let flags = s[13];
    let wnd = ((s[14] as u32) << 8) | s[15] as u32;
    if off > s.len() || off < 20 { return; }
    let data = &s[off..];

    if flags & RST != 0 {
        t.state = TcpState::Error;
        return;
    }
    if t.state == TcpState::SynSent {
        if flags & (SYN | ACK) == SYN | ACK && ackn == t.snd_una.wrapping_add(1) {
            // opcao MSS do par
            let mut i = 20;
            while i + 1 < off {
                match s[i] {
                    0 => break,
                    1 => i += 1,
                    2 if i + 3 < off => {
                        t.mss = (((s[i + 2] as usize) << 8) | s[i + 3] as usize).clamp(536, 1460);
                        i += 4;
                    }
                    _ => i += (s[i + 1] as usize).max(2),
                }
            }
            t.snd_una = ackn;
            t.snd_nxt = ackn;
            t.rcv_nxt = seq.wrapping_add(1);
            t.peer_wnd = wnd;
            t.state = TcpState::Established;
            t.syn_pending = false;
            t.retries = 0;
            let n = t.snd_nxt;
            tcp_segment(t, ACK, n, &[]);
            tcp_output(t);
        }
        return;
    }
    // ACK do que enviamos
    if flags & ACK != 0 {
        let acked = ackn.wrapping_sub(t.snd_una) as usize;
        let in_flight = t.snd_nxt.wrapping_sub(t.snd_una) as usize;
        if acked > 0 && acked <= in_flight {
            t.tx.copy_within(acked..t.tx_len, 0);
            t.tx_len -= acked;
            t.snd_una = ackn;
            t.retries = 0;
            t.last_tx = crate::time::now_ms();
        }
        t.peer_wnd = wnd;
    }
    // dados: so em ordem (o par retransmite o resto)
    let mut need_ack = false;
    let mut in_order = data.is_empty();
    if !data.is_empty() {
        let mut d = data;
        let mut sq = seq;
        let behind = t.rcv_nxt.wrapping_sub(sq) as usize;
        if behind < d.len() && behind < 0x8000_0000 {
            d = &d[behind..];
            sq = t.rcv_nxt;
        }
        if sq == t.rcv_nxt {
            let room = RXCAP - t.rx_len;
            let n = d.len().min(room);
            for (k, &b) in d[..n].iter().enumerate() {
                t.rx[(t.rx_head + t.rx_len + k) % RXCAP] = b;
            }
            t.rx_len += n;
            t.rcv_nxt = t.rcv_nxt.wrapping_add(n as u32);
            in_order = n == d.len();
        }
        need_ack = true;
    }
    if flags & FIN != 0 && in_order && seq.wrapping_add(data.len() as u32) == t.rcv_nxt
        && t.state == TcpState::Established
    {
        t.rcv_nxt = t.rcv_nxt.wrapping_add(1);
        t.state = TcpState::PeerClosed;
        need_ack = true;
    }
    if need_ack {
        let n = t.snd_nxt;
        tcp_segment(t, ACK, n, &[]);
    }
    tcp_output(t);
}

/// Retransmissoes por tempo (SYN ou dados nao confirmados).
fn tcp_timers() {
    let now = crate::time::now_ms();
    for t in tcps().iter_mut() {
        let waiting = t.syn_pending || t.snd_nxt != t.snd_una;
        if t.state == TcpState::Free || t.state == TcpState::Error || !waiting { continue; }
        if now - t.last_tx < RTO_MS << t.retries.min(4) { continue; }
        t.retries += 1;
        if t.retries > MAX_RETRIES {
            t.state = TcpState::Error;
            continue;
        }
        t.last_tx = now;
        if t.syn_pending {
            let s = t.snd_una;
            tcp_segment(t, SYN, s, &[]);
        } else {
            // volta ao primeiro byte nao confirmado (go-back-N)
            t.snd_nxt = t.snd_una;
            tcp_output(t);
        }
    }
}

/// Abre uma conexao (nao bloqueia: o handshake avanca a cada poll).
pub fn tcp_connect(ip: &[u8; 4], port: u16) -> Option<usize> {
    resolve_gateway()?;
    poll();
    let h = tcps().iter().position(|t| t.state == TcpState::Free)?;
    let lport = unsafe {
        let p = NEXT_PORT;
        NEXT_PORT = if NEXT_PORT >= 60999 { 49152 } else { NEXT_PORT + 1 };
        p
    };
    let isn = (crate::arch::now() as u32).wrapping_mul(2654435761);
    let t = &mut tcps()[h];
    t.state = TcpState::SynSent;
    t.lport = lport;
    t.rip = *ip;
    t.rport = port;
    t.snd_una = isn;
    t.snd_nxt = isn;
    t.rcv_nxt = 0;
    t.peer_wnd = 0;
    t.mss = 536;
    t.tx_len = 0;
    t.rx_head = 0;
    t.rx_len = 0;
    t.retries = 0;
    t.syn_pending = true;
    t.last_tx = crate::time::now_ms();
    tcp_segment(t, SYN, isn, &[]);
    Some(h)
}

fn conn(h: usize) -> Option<&'static mut Tcp> {
    tcps().get_mut(h).filter(|t| t.state != TcpState::Free)
}

/// Estado: 0 conectando, 1 estabelecida, 2 fechada pelo par, -1 erro.
pub fn tcp_status(h: usize) -> i64 {
    poll();
    match conn(h).map(|t| t.state) {
        Some(TcpState::SynSent) => 0,
        Some(TcpState::Established) => 1,
        Some(TcpState::PeerClosed) => 2,
        _ => -1,
    }
}

/// Enfileira dados para envio. Retorna quantos bytes foram aceitos
/// (0 = buffer cheio ou ainda conectando) ou -1 (conexao invalida).
pub fn tcp_send(h: usize, data: &[u8]) -> i64 {
    poll();
    let Some(t) = conn(h) else { return -1 };
    match t.state {
        TcpState::SynSent => return 0,
        TcpState::Established => {}
        _ => return -1,
    }
    let n = data.len().min(TXCAP - t.tx_len);
    t.tx[t.tx_len..t.tx_len + n].copy_from_slice(&data[..n]);
    t.tx_len += n;
    tcp_output(t);
    n as i64
}

/// Le dados recebidos: n > 0 bytes; 0 = fim (par fechou e nada mais a
/// ler); -2 = nada ainda (tente de novo); -1 = erro/conexao resetada.
pub fn tcp_recv(h: usize, out: &mut [u8]) -> i64 {
    poll();
    let Some(t) = conn(h) else { return -1 };
    if t.rx_len == 0 {
        return match t.state {
            TcpState::PeerClosed => 0,
            TcpState::Error => -1,
            _ => -2,
        };
    }
    let n = out.len().min(t.rx_len);
    for (k, b) in out[..n].iter_mut().enumerate() {
        *b = t.rx[(t.rx_head + k) % RXCAP];
    }
    t.rx_head = (t.rx_head + n) % RXCAP;
    t.rx_len -= n;
    // a janela reabriu bastante: avisa o par (senao ele fica parado)
    let free = (RXCAP - t.rx_len).min(65535);
    if t.state == TcpState::Established && t.adv_wnd < RXCAP / 4 && free >= RXCAP / 2 {
        let s = t.snd_nxt;
        tcp_segment(t, ACK, s, &[]);
    }
    n as i64
}

/// Fecha a conexao (FIN, ou RST se ainda nao estabelecida) e libera o slot.
pub fn tcp_close(h: usize) {
    poll();
    let Some(t) = conn(h) else { return };
    let s = t.snd_nxt;
    match t.state {
        TcpState::Established | TcpState::PeerClosed => tcp_segment(t, FIN | ACK, s, &[]),
        TcpState::SynSent => tcp_segment(t, RST, s, &[]),
        _ => {}
    }
    t.state = TcpState::Free;
}

// ------------------------------------------------------ prontidao (poll/epoll)

/// (ha o que ler ou fim, cabe mais para enviar, erro/fechada) — sem consumir.
pub fn tcp_ready(h: usize) -> (bool, bool, bool) {
    poll();
    let Some(t) = conn(h) else { return (true, false, true) };
    match t.state {
        TcpState::SynSent => (false, false, false),
        TcpState::Established => (t.rx_len > 0, t.tx_len < TXCAP, false),
        TcpState::PeerClosed => (true, false, true),
        _ => (true, false, true),
    }
}

/// Bytes recebidos e ainda nao lidos (FIONREAD).
pub fn tcp_avail(h: usize) -> usize {
    poll();
    conn(h).map_or(0, |t| t.rx_len)
}

/// Tamanho do proximo datagrama na porta `port` (FIONREAD), 0 se nenhum.
pub fn udp_next_len(port: u16) -> usize {
    poll();
    let q = unsafe { &*core::ptr::addr_of!(UDPQ) };
    q.iter().find(|d| d.used && d.dst_port == port).map_or(0, |d| d.len)
}

/// Ha datagrama esperando na porta `port`?
pub fn udp_pending(port: u16) -> bool {
    poll();
    let q = unsafe { &*core::ptr::addr_of!(UDPQ) };
    q.iter().any(|d| d.used && d.dst_port == port)
}

/// Porta UDP local livre para um socket novo.
pub fn udp_alloc_port() -> u16 {
    unsafe {
        let p = NEXT_PORT;
        NEXT_PORT = if NEXT_PORT >= 60999 { 49152 } else { NEXT_PORT + 1 };
        p
    }
}

/// Porta local de uma conexao TCP (getsockname).
pub fn tcp_local_port(h: usize) -> u16 {
    conn(h).map_or(0, |t| t.lport)
}
