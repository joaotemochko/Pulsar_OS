//! trespass.pulse — processo HOSTIL de proposito.
//!
//! Tenta ler a memoria de codigo do shell (VA 0x0100_0000). Com espacos
//! de enderecos por processo, esse VA NAO EXISTE no espaco do trespass:
//! o hardware gera Data Abort e o kernel mata o processo. Se o print de
//! "CONSEGUI LER" aparecer na serial, o isolamento falhou.
#![no_std]
#![no_main]

use plib::*;

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    exit()
}

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    write("[trespass] rodando. Tentando ler 0x01000000 (codigo do shell)...\n");

    let leaked = unsafe { core::ptr::read_volatile(0x0100_0000 as *const u32) };

    // Se chegamos aqui, o isolamento FALHOU.
    write("[trespass] !!! CONSEGUI LER A MEMORIA DO SHELL — ISOLAMENTO QUEBRADO !!!\n");
    let _ = leaked;
    exit()
}
