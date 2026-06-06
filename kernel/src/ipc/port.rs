#[derive(Debug, PartialEq)]
pub enum PortState {
    Free,
    InUse { owner_pid: u32 }, // O enum guarda o PID apenas se estiver em uso!
}

pub struct Port {
    pub port_id: u32,
    pub name: [u8; 64], // Nome em array de bytes para evitar alocação dinâmica no início
    pub state: PortState,
    
    // No futuro, usaremos VecDeque ou estruturas de sincronização aqui
    // para enfileirar as threads que estão esperando resposta.
}