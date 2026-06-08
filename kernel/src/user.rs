use core::arch::global_asm;

// Processo A: imprime "A" em loop infinito, SEM yield (nao coopera).
global_asm!(
    ".section .rodata",
    ".global pulse_a_start",
    ".balign 8",
    "pulse_a_start:",
    "    .word 0x534C5550",
    "    .hword 1",
    "    .hword 1",
    "    .quad  0x40100000",
    "    .word  (a_seg - pulse_a_start)",
    "    .word  0",
    "a_seg:",
    "    .word  (a_code - pulse_a_start)",
    "    .word  (a_end - a_code)",
    "    .quad  0x40100000",
    "    .word  (a_end - a_code)",
    "    .word  0b101",
    ".balign 4",
    "a_code:",
    "a_loop:",
    "    mov   x8, #1",
    "    adr   x0, a_msg",
    "    mov   x1, #1",
    "    svc   #0",
    // espera um pouco (busy loop) so pra nao floodar a tela rapido demais
    "    mov   x9, #0x400000",
    "a_delay:",
    "    sub   x9, x9, #1",
    "    cbnz  x9, a_delay",
    "    b     a_loop",          // <- SEM yield: loop infinito puro
    ".balign 4",
    "a_msg:",
    "    .ascii \"A\"",
    "a_end:",
);

// Processo B: imprime "B" em loop infinito, SEM yield.
global_asm!(
    ".section .rodata",
    ".global pulse_b_start",
    ".balign 8",
    "pulse_b_start:",
    "    .word 0x534C5550",
    "    .hword 1",
    "    .hword 1",
    "    .quad  0x40110000",
    "    .word  (b_seg - pulse_b_start)",
    "    .word  0",
    "b_seg:",
    "    .word  (b_code - pulse_b_start)",
    "    .word  (b_end - b_code)",
    "    .quad  0x40110000",
    "    .word  (b_end - b_code)",
    "    .word  0b101",
    ".balign 4",
    "b_code:",
    "b_loop:",
    "    mov   x8, #1",
    "    adr   x0, b_msg",
    "    mov   x1, #1",
    "    svc   #0",
    "    mov   x9, #0x400000",
    "b_delay:",
    "    sub   x9, x9, #1",
    "    cbnz  x9, b_delay",
    "    b     b_loop",          // <- SEM yield
    ".balign 4",
    "b_msg:",
    "    .ascii \"B\"",
    "b_end:",
);

unsafe extern "C" {
    pub static pulse_a_start: u8;
    pub static pulse_b_start: u8;
}