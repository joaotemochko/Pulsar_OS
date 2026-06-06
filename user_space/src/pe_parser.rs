#![no_std]

// A anotação `packed` é crucial porque os cabeçalhos PE não têm padding (alinhamento) perfeito
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct DosHeader {
    pub e_magic: u16,      // O famoso "MZ" (0x5A4D)
    pub e_cblp: u16,
    pub e_cp: u16,
    pub e_crlc: u16,
    pub e_cparhdr: u16,
    pub e_minalloc: u16,
    pub e_maxalloc: u16,
    pub e_ss: u16,
    pub e_sp: u16,
    pub e_csum: u16,
    pub e_ip: u16,
    pub e_cs: u16,
    pub e_lfarlc: u16,
    pub e_ovno: u16,
    pub e_res: [u16; 4],
    pub e_oemid: u16,
    pub e_oeminfo: u16,
    pub e_res2: [u16; 10],
    pub e_lfanew: u32,     // O offset (posição) onde o verdadeiro cabeçalho PE começa
}

#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct CoffHeader {
    pub machine: u16,              // Para nós, isso DEVE ser 0xAA64 (AArch64)
    pub number_of_sections: u16,   // Quantas seções (.text, .data) existem
    pub time_date_stamp: u32,
    pub pointer_to_symbol_table: u32,
    pub number_of_symbols: u32,
    pub size_of_optional_header: u16,
    pub characteristics: u16,
}

// O cabeçalho opcional do PE32+ (Formato de 64 bits do Windows)
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct OptionalHeader64 {
    pub magic: u16,                // DEVE ser 0x020B (PE32+)
    pub major_linker_version: u8,
    pub minor_linker_version: u8,
    pub size_of_code: u32,
    pub size_of_initialized_data: u32,
    pub size_of_uninitialized_data: u32,
    pub address_of_entry_point: u32, // O "main()" do programa!
    pub base_of_code: u32,
    pub image_base: u64,             // Onde o programa prefere ser carregado na memória
    pub section_alignment: u32,
    pub file_alignment: u32,
    // ... existem mais campos, mas este é o núcleo vital para começarmos
}