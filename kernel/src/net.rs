//! Driver virtio-net + pilha de rede minima (Ethernet/ARP/IPv4/UDP).
//!
//! Objetivo: provar rede REAL no QEMU. Com "-netdev user" (SLIRP), o QEMU
//! roteia nossos pacotes para a internet do host via NAT. Endereco padrao
//! do SLIRP: gateway 10.0.2.2, DNS 10.0.2.3, nosso IP 10.0.2.15.
//!
//! Implementado: virtio-net (filas RX/TX), Ethernet, ARP (resolve o gateway),
//! IPv4 + checksum, UDP (envio e recepcao). Suficiente para um "ping UDP"
//! e para consultas simples. TCP fica para depois.

use crate::virtio::{self, QueueMem, VirtQueue};
use crate::uart::Uart;
use core::fmt::Write;

const VIRTIO_ID_NET: u32 = 1;

// Nosso endereco (padrao do SLIRP do QEMU)
pub const OUR_IP: [u8; 4] = [10, 0, 2, 15];
pub const GW_IP:  [u8; 4] = [10, 0, 2, 2];

static mut MAC: [u8; 6] = [0x52, 0x54, 0x00, 0x12, 0x34, 0x56];

// virtio_net_hdr: 12 bytes antes de cada frame
const NET_HDR_LEN: usize = 12;

#[repr(C, align(4096))]
struct PktBuf([u8; 2048]);
static mut TXBUF: PktBuf = PktBuf([0; 2048]);
static mut RXBUFS: [PktBuf; 8] = [const { PktBuf([0; 2048]) }; 8];

static mut RXQ_MEM: QueueMem = QueueMem::zeroed();
static mut TXQ_MEM: QueueMem = QueueMem::zeroed();

struct Net {
    base: u64,
    rxq: VirtQueue,
    txq: VirtQueue,
}
static mut NET: Option<Net> = None;

// Cache ARP do gateway (preenchido apos resolver)
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
    // le o MAC da config do dispositivo (offset 0..6)
    let mut mac = [0u8; 6];
    for i in 0..6 {
        mac[i] = virtio::config_read8(base, i as u64);
    }
    unsafe { MAC = mac; }

    let rxq = unsafe { VirtQueue::setup(base, 0, &raw mut RXQ_MEM) };
    let txq = unsafe { VirtQueue::setup(base, 1, &raw mut TXQ_MEM) };
    let (Some(rxq), Some(txq)) = (rxq, txq) else {
        let _ = write!(serial, "[net] setup de filas falhou\n");
        return false;
    };
    virtio::driver_ok(base);

    unsafe { NET = Some(Net { base, rxq, txq }); }

    // posta buffers de recepcao
    unsafe {
        if let Some(n) = (&mut *core::ptr::addr_of_mut!(NET)).as_mut() {
            for i in 0..8u16 {
                let addr = core::ptr::addr_of!(RXBUFS[i as usize].0) as u64;
                n.rxq.post_recv(i, addr, 2048);
            }
        }
    }

    let _ = write!(serial, "[net] virtio-net em {:#x}, MAC {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}\n",
                   base, mac[0],mac[1],mac[2],mac[3],mac[4],mac[5]);
    let _ = write!(serial, "[net] IP {}.{}.{}.{}  gateway {}.{}.{}.{}\n",
                   OUR_IP[0],OUR_IP[1],OUR_IP[2],OUR_IP[3], GW_IP[0],GW_IP[1],GW_IP[2],GW_IP[3]);
    true
}

pub fn mac() -> [u8; 6] { unsafe { MAC } }

/// A rede esta inicializada?
pub fn is_up() -> bool { unsafe { (&*core::ptr::addr_of!(NET)).is_some() } }

/// Envia um frame Ethernet cru (dst_mac + ethertype + payload).
fn send_frame(dst_mac: &[u8;6], ethertype: u16, payload: &[u8]) {
    unsafe {
        let Some(n) = (&mut *core::ptr::addr_of_mut!(NET)).as_mut() else { return; };
        let src_mac = MAC;
        let buf = &mut *core::ptr::addr_of_mut!(TXBUF.0);
        for b in buf[..NET_HDR_LEN].iter_mut() { *b = 0; }
        let mut p = NET_HDR_LEN;
        buf[p..p+6].copy_from_slice(dst_mac); p+=6;
        buf[p..p+6].copy_from_slice(&src_mac); p+=6;
        buf[p]=(ethertype>>8) as u8; buf[p+1]=ethertype as u8; p+=2;
        buf[p..p+payload.len()].copy_from_slice(payload); p+=payload.len();
        let total = p;
        let addr = core::ptr::addr_of!(TXBUF.0) as u64;
        n.txq.request_sync(&[(addr, total as u32, false)]);
    }
}

/// checksum da internet (16-bit one's complement)
fn checksum(data: &[u8]) -> u16 {
    let mut sum: u32 = 0;
    let mut i = 0;
    while i + 1 < data.len() {
        sum += ((data[i] as u32) << 8) | (data[i+1] as u32);
        i += 2;
    }
    if i < data.len() { sum += (data[i] as u32) << 8; }
    while sum >> 16 != 0 { sum = (sum & 0xFFFF) + (sum >> 16); }
    !(sum as u16)
}

/// Monta e envia um pacote ARP request perguntando "quem tem GW_IP?"
fn arp_request(target: &[u8;4]) {
    let mut a = [0u8; 28];
    a[0]=0;a[1]=1;            // hw type ethernet
    a[2]=0x08;a[3]=0x00;      // proto ipv4
    a[4]=6;a[5]=4;            // tamanhos
    a[6]=0;a[7]=1;            // opcode request
    let src_mac = mac(); a[8..14].copy_from_slice(&src_mac);   // sender mac
    a[14..18].copy_from_slice(&OUR_IP);          // sender ip
    // target mac = 0
    a[24..28].copy_from_slice(target);           // target ip
    send_frame(&[0xFF;6], 0x0806, &a);
}

/// Processa um frame recebido. Atualiza cache ARP e devolve UDP se houver.
/// Retorna Some((src_ip, src_port, dst_port, payload_len)) para UDP.
fn handle_frame(frame: &[u8]) {
    if frame.len() < 14 { return; }
    let ethertype = ((frame[12] as u16) << 8) | frame[13] as u16;
    let payload = &frame[14..];
    match ethertype {
        0x0806 => { // ARP
            if payload.len() >= 28 {
                let opcode = ((payload[6] as u16)<<8)|payload[7] as u16;
                if opcode == 2 { // reply
                    let mut smac = [0u8;6]; smac.copy_from_slice(&payload[8..14]);
                    let mut sip = [0u8;4]; sip.copy_from_slice(&payload[14..18]);
                    if sip == GW_IP { unsafe { GW_MAC = Some(smac); } }
                }
            }
        }
        0x0800 => { // IPv4
            if payload.len() >= 20 {
                let ihl = ((payload[0] & 0x0F) as usize) * 4;
                let proto = payload[9];
                if proto == 6 && payload.len() > ihl {  // TCP
                    tcp_recv(payload, &payload[ihl..]);
                }
            }
        }
        _ => {}
    }
}

/// Drena a fila RX processando frames recebidos (nao bloqueia).
pub fn poll() {
    unsafe {
        let Some(n) = (&mut *core::ptr::addr_of_mut!(NET)).as_mut() else { return; };
        while let Some((idx, len)) = n.rxq.poll_used() {
            let buf = &*core::ptr::addr_of!(RXBUFS[idx as usize].0);
            if len as usize > NET_HDR_LEN {
                let frame = &buf[NET_HDR_LEN..len as usize];
                handle_frame(frame);
            }
            // reposta o buffer
            let addr = core::ptr::addr_of!(RXBUFS[idx as usize].0) as u64;
            n.rxq.post_recv(idx, addr, 2048);
        }
    }
}

/// Resolve o MAC do gateway via ARP (bloqueia ate ~1M iteracoes).
pub fn resolve_gateway() -> Option<[u8;6]> {
    unsafe { if let Some(m) = GW_MAC { return Some(m); } }
    let mut serial = Uart;
    for _try in 0..4 {
        arp_request(&GW_IP);
        for _ in 0..500_000 {
            poll();
            unsafe { if let Some(m) = GW_MAC { 
                let _ = write!(serial, "[net] gateway {}.{}.{}.{} -> {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}\n",
                    GW_IP[0],GW_IP[1],GW_IP[2],GW_IP[3], m[0],m[1],m[2],m[3],m[4],m[5]);
                return Some(m);
            } }
            core::hint::spin_loop();
        }
    }
    let _ = write!(serial, "[net] ARP do gateway sem resposta\n");
    None
}

/// Envia um datagrama UDP para dst_ip:dst_port a partir de src_port.
pub fn udp_send(dst_ip: &[u8;4], dst_port: u16, src_port: u16, data: &[u8]) -> bool {
    let Some(gw) = resolve_gateway() else { return false; };
    // Como so falamos com o mundo via gateway (NAT), enviamos ao MAC do gw.
    let udp_len = 8 + data.len();
    let ip_len = 20 + udp_len;
    let mut pkt = [0u8; 1500];
    // ---- IPv4 header ----
    pkt[0]=0x45;                 // versao 4, IHL 5
    pkt[1]=0;                    // DSCP
    pkt[2]=(ip_len>>8) as u8; pkt[3]=ip_len as u8;
    pkt[4]=0;pkt[5]=0;           // id
    pkt[6]=0x40;pkt[7]=0;        // flags: don't fragment
    pkt[8]=64;                   // TTL
    pkt[9]=17;                   // protocolo UDP
    // checksum (10,11) depois
    pkt[12..16].copy_from_slice(&OUR_IP);
    pkt[16..20].copy_from_slice(dst_ip);
    let ipcsum = checksum(&pkt[0..20]);
    pkt[10]=(ipcsum>>8) as u8; pkt[11]=ipcsum as u8;
    // ---- UDP header ----
    let u = 20;
    pkt[u]=(src_port>>8) as u8; pkt[u+1]=src_port as u8;
    pkt[u+2]=(dst_port>>8) as u8; pkt[u+3]=dst_port as u8;
    pkt[u+4]=(udp_len>>8) as u8; pkt[u+5]=udp_len as u8;
    // checksum UDP opcional em IPv4 -> deixamos 0
    pkt[u+8..u+8+data.len()].copy_from_slice(data);
    send_frame(&gw, 0x0800, &pkt[0..ip_len]);
    true
}

// ============================================================
// TCP mínimo — cliente para um único HTTP GET.
// Sem retransmissão nem controle de congestionamento: o SLIRP do
// QEMU é local e confiável. Suficiente para provar HTTP real.
// ============================================================

#[derive(Clone, Copy, PartialEq)]
enum TcpState { Closed, SynSent, Established, FinWait, Done }

struct Tcp {
    state: TcpState,
    local_port: u16,
    remote_ip: [u8; 4],
    remote_port: u16,
    snd_next: u32,   // proximo seq nosso
    rcv_next: u32,   // proximo ack esperado (seq deles + dados)
    // buffer de resposta acumulada
    resp_len: usize,
}

#[repr(C, align(4096))]
struct RespBuf([u8; 16384]);
static mut RESP: RespBuf = RespBuf([0; 16384]);

static mut TCP: Tcp = Tcp {
    state: TcpState::Closed, local_port: 0, remote_ip: [0;4], remote_port: 0,
    snd_next: 0, rcv_next: 0, resp_len: 0,
};

// checksum TCP/UDP com pseudo-header IPv4
fn l4_checksum(src: &[u8;4], dst: &[u8;4], proto: u8, l4: &[u8]) -> u16 {
    let mut sum: u32 = 0;
    // pseudo-header
    sum += ((src[0] as u32)<<8)|src[1] as u32;
    sum += ((src[2] as u32)<<8)|src[3] as u32;
    sum += ((dst[0] as u32)<<8)|dst[1] as u32;
    sum += ((dst[2] as u32)<<8)|dst[3] as u32;
    sum += proto as u32;
    sum += l4.len() as u32;
    // corpo L4
    let mut i = 0;
    while i + 1 < l4.len() { sum += ((l4[i] as u32)<<8)|l4[i+1] as u32; i += 2; }
    if i < l4.len() { sum += (l4[i] as u32)<<8; }
    while sum>>16 != 0 { sum = (sum & 0xFFFF) + (sum>>16); }
    !(sum as u16)
}

// envia um segmento TCP (flags, dados). Monta IP+TCP e despacha via gateway.
fn tcp_send(flags: u8, data: &[u8]) {
    let gw = unsafe { match GW_MAC { Some(m)=>m, None=>return } };
    let (rip, rport, lport, seq, ack) = unsafe {
        (TCP.remote_ip, TCP.remote_port, TCP.local_port, TCP.snd_next, TCP.rcv_next)
    };
    let tcp_len = 20 + data.len();
    let ip_len = 20 + tcp_len;
    let mut pkt = [0u8; 1500];
    // IPv4
    pkt[0]=0x45; pkt[2]=(ip_len>>8) as u8; pkt[3]=ip_len as u8;
    pkt[6]=0x40; pkt[8]=64; pkt[9]=6; // proto TCP
    pkt[12..16].copy_from_slice(&OUR_IP);
    pkt[16..20].copy_from_slice(&rip);
    let ipc = checksum(&pkt[0..20]);
    pkt[10]=(ipc>>8) as u8; pkt[11]=ipc as u8;
    // TCP
    let t = 20;
    pkt[t]=(lport>>8) as u8; pkt[t+1]=lport as u8;
    pkt[t+2]=(rport>>8) as u8; pkt[t+3]=rport as u8;
    pkt[t+4..t+8].copy_from_slice(&seq.to_be_bytes());
    pkt[t+8..t+12].copy_from_slice(&ack.to_be_bytes());
    pkt[t+12]=0x50; // data offset 5 (20 bytes)
    pkt[t+13]=flags;
    pkt[t+14]=0xFF; pkt[t+15]=0xFF; // window
    pkt[t+20..t+20+data.len()].copy_from_slice(data);
    let csum = l4_checksum(&OUR_IP, &rip, 6, &pkt[t..t+tcp_len]);
    pkt[t+16]=(csum>>8) as u8; pkt[t+17]=csum as u8;
    send_frame(&gw, 0x0800, &pkt[0..ip_len]);
}

// processa um segmento TCP recebido (chamado de handle_frame)
fn tcp_recv(ip: &[u8], tcp: &[u8]) {
    if tcp.len() < 20 { return; }
    unsafe {
        if TCP.state == TcpState::Closed || TCP.state == TcpState::Done { return; }
        let sport = ((tcp[0] as u16)<<8)|tcp[1] as u16;
        // confere que e da nossa conexao
        if sport != TCP.remote_port { return; }
        let seq = u32::from_be_bytes([tcp[4],tcp[5],tcp[6],tcp[7]]);
        let ackn = u32::from_be_bytes([tcp[8],tcp[9],tcp[10],tcp[11]]);
        let flags = tcp[13];
        let data_off = ((tcp[12]>>4) as usize)*4;
        let payload = if tcp.len() > data_off { &tcp[data_off..] } else { &[][..] };
        let _ = (ip, ackn);

        let syn = flags & 0x02 != 0;
        let ackf = flags & 0x10 != 0;
        let fin = flags & 0x01 != 0;
        let psh = flags & 0x08 != 0;
        let _ = psh;

        match TCP.state {
            TcpState::SynSent => {
                if syn && ackf {
                    // SYN-ACK recebido: ack = seq deles + 1, nosso seq +1
                    TCP.rcv_next = seq.wrapping_add(1);
                    TCP.snd_next = TCP.snd_next.wrapping_add(1);
                    TCP.state = TcpState::Established;
                    tcp_send(0x10, &[]); // ACK do handshake
                }
            }
            TcpState::Established => {
                if !payload.is_empty() {
                    // acumula dados
                    let take = payload.len().min(16384 - TCP.resp_len);
                    let dst = &mut *core::ptr::addr_of_mut!(RESP.0);
                    dst[TCP.resp_len..TCP.resp_len+take].copy_from_slice(&payload[..take]);
                    TCP.resp_len += take;
                    TCP.rcv_next = TCP.rcv_next.wrapping_add(payload.len() as u32);
                    tcp_send(0x10, &[]); // ACK dos dados
                }
                if fin {
                    TCP.rcv_next = TCP.rcv_next.wrapping_add(1);
                    tcp_send(0x11, &[]); // FIN+ACK
                    TCP.snd_next = TCP.snd_next.wrapping_add(1);
                    TCP.state = TcpState::Done;
                }
            }
            TcpState::FinWait => {
                if fin { TCP.rcv_next = TCP.rcv_next.wrapping_add(1); tcp_send(0x10,&[]); }
                TCP.state = TcpState::Done;
            }
            _ => {}
        }
    }
}

/// Faz um HTTP GET simples para ip:port com o caminho dado.
/// Retorna (ptr, len) do corpo bruto recebido (headers+corpo), no buffer RESP.
/// Bloqueia ate a conexao terminar ou timeout.
pub fn http_get(ip: &[u8;4], port: u16, host: &[u8], path: &[u8]) -> (u64, usize) {
    if resolve_gateway().is_none() { return (0,0); }
    unsafe {
        TCP = Tcp {
            state: TcpState::SynSent,
            local_port: 49152,
            remote_ip: *ip, remote_port: port,
            snd_next: 0x1000, rcv_next: 0, resp_len: 0,
        };
    }
    // envia SYN
    tcp_send(0x02, &[]);
    // espera established (timeout)
    let mut ok = false;
    for _ in 0..2_000_000 {
        poll();
        unsafe { if TCP.state == TcpState::Established { ok=true; break; }
                 if TCP.state == TcpState::Done { break; } }
        core::hint::spin_loop();
    }
    if !ok { return (0,0); }

    // monta e envia a requisicao HTTP/1.0
    let mut req = [0u8; 512];
    let mut p = 0;
    let put = |req: &mut [u8], p: &mut usize, s: &[u8]| {
        req[*p..*p+s.len()].copy_from_slice(s); *p += s.len();
    };
    put(&mut req,&mut p,b"GET ");
    put(&mut req,&mut p,path);
    put(&mut req,&mut p,b" HTTP/1.0\r\nHost: ");
    put(&mut req,&mut p,host);
    put(&mut req,&mut p,b"\r\nConnection: close\r\n\r\n");
    tcp_send(0x18, &req[..p]); // PSH+ACK
    unsafe { TCP.snd_next = TCP.snd_next.wrapping_add(p as u32); }

    // recebe ate Done (fin) ou timeout
    for _ in 0..5_000_000 {
        poll();
        unsafe { if TCP.state == TcpState::Done { break; } }
        core::hint::spin_loop();
    }
    unsafe { (core::ptr::addr_of!(RESP.0) as u64, TCP.resp_len) }
}
