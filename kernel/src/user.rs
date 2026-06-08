use core::arch::global_asm;

// Programa de EL0 em assembly puro, position-independent.
// Faz SYS_WRITE de uma string que vive na MESMA pagina (acesso PC-relative),
// depois entra em loop. Sem nenhuma referencia a endereco absoluto do kernel,
// entao funciona de onde quer que seja copiado.
global_asm!(
    ".section .text",
    ".global user_program_start",
    ".global user_program_end",
    ".balign 4",
    "user_program_start:",
    "    mov   x8, #1",                          // x8 = SYS_WRITE
    "    adr   x0, user_msg",                    // x0 = &mensagem (PC-relative)
    "    mov   x1, #(user_msg_end - user_msg)",  // x1 = tamanho
    "    svc   #0",                              // chama o kernel
    "1:  wfe",
    "    b     1b",
    ".balign 4",
    "user_msg:",
    "    .ascii \"  [EL0] Ola do espaco de usuario via SYS_WRITE!\\n\"",
    "user_msg_end:",
    "user_program_end:",
);

unsafe extern "C" {
    pub static user_program_start: u8;
    pub static user_program_end: u8;
}