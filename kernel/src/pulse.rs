/// Magic: "PULS" em little-endian.
pub const PULSE_MAGIC: u32 = 0x534C5550; // 'P','U','L','S'

#[repr(C)]
#[derive(Clone, Copy)]
pub struct PulseHeader {
    pub magic: u32,        // PULSE_MAGIC
    pub version: u16,      // versao do formato
    pub seg_count: u16,    // numero de segmentos
    pub entry: u64,        // VA do ponto de entrada
    pub seg_table_off: u32, // offset (no arquivo) da tabela de segmentos
    pub _reserved: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct PulseSegment {
    pub file_off: u32,   // offset dos bytes no arquivo .pulse
    pub file_size: u32,  // quantos bytes copiar do arquivo
    pub vaddr: u64,      // VA de destino
    pub mem_size: u32,   // tamanho em memoria (>= file_size; resto e zerado = .bss)
    pub flags: u32,      // bit0=R, bit1=W, bit2=X
}

pub const SEG_R: u32 = 1 << 0;
pub const SEG_W: u32 = 1 << 1;
pub const SEG_X: u32 = 1 << 2;