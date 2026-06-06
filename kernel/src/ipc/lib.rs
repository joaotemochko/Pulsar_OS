#![no_std]

pub const MAX_PAYLOAD_SIZE: usize = 256;

#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageType {
    Short = 1,  // Caminho Rápido (Dados no payload)
    Shared = 2, // Caminho Pesado (Aviso de memória compartilhada)
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct MessageHeader {
    pub sender_pid: u32,
    pub port_id: u32,
    pub msg_type: MessageType,
    pub payload_size: u32,
}

#[repr(C)]
pub struct Message {
    pub header: MessageHeader,
    pub payload: [u8; MAX_PAYLOAD_SIZE], // Array fixo para alocação rápida
}

impl Message {
    // Uma função auxiliar para criar mensagens vazias facilmente
    pub fn new() -> Self {
        Self {
            header: MessageHeader {
                sender_pid: 0,
                port_id: 0,
                msg_type: MessageType::Short,
                payload_size: 0,
            },
            payload: [0; MAX_PAYLOAD_SIZE],
        }
    }
}