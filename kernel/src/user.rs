use core::arch::global_asm;

// Um arquivo .pulse COMPLETO montado a mao em assembly:
// [PulseHeader][PulseSegment][codigo+dados do programa]
// O programa: SYS_WRITE de uma mensagem, depois loop.
global_asm!(
    ".section .rodata",
    ".global pulse_file_start",
    ".global pulse_file_end",
    ".balign 8",
    "pulse_file_start:",
    // --- PulseHeader (24 bytes) ---
    "    .word 0x534C5550",        // magic "PULS"
    "    .hword 1",                // version
    "    .hword 1",                // seg_count = 1
    "    .quad  0x40100000",       // entry (VA do entry point)
    "    .word  (seg_table - pulse_file_start)", // seg_table_off
    "    .word  0",                // reserved
    // --- PulseSegment (24 bytes) ---
    "seg_table:",
    "    .word  (prog_code - pulse_file_start)",  // file_off
    "    .word  (prog_end - prog_code)",          // file_size
    "    .quad  0x40100000",       // vaddr
    "    .word  (prog_end - prog_code)",          // mem_size
    "    .word  0b101",            // flags: R+X (sem W -> W^X!)
    // --- codigo do programa ---
    ".balign 4",
    "prog_code:",
    "    mov   x8, #1",
    "    adr   x0, prog_msg",
    "    mov   x1, #(prog_msg_end - prog_msg)",
    "    svc   #0",                  // imprime a mensagem (prova que rodou)
    // --- TESTE W^X: tenta escrever na propria pagina de codigo ---
    "    adr   x2, prog_code",       // x2 = endereco do proprio codigo (R+X, sem W)
    "    mov   x3, #0xDEAD",
    "    str   x3, [x2]",            // ESCRITA em pagina nao-gravavel -> deve faultar
    // se chegar aqui, o W^X FALHOU (nao deveria executar):
    "    mov   x8, #1",
    "    adr   x0, fail_msg",
    "    mov   x1, #(fail_msg_end - fail_msg)",
    "    svc   #0",
    "1:  wfe",
    "    b     1b",
    ".balign 4",
    "prog_msg:",
    "    .ascii \"  [.pulse/EL0] Programa carregado pelo loader!\\n\"",
    "prog_msg_end:",
    "fail_msg:",
    "    .ascii \"  [.pulse/EL0] ERRO: escrita em codigo funcionou (W^X FALHOU)!\\n\"",
    "fail_msg_end:",
    "prog_end:",
);

unsafe extern "C" {
    pub static pulse_file_start: u8;
    pub static pulse_file_end: u8;
}