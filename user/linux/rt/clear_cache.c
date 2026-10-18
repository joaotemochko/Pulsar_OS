/* __clear_cache(inicio, fim) — torna visivel ao cache de instrucoes o codigo
 * recem-escrito (JITs, trampolins da libffi). Vem do runtime do compilador
 * (libgcc/compiler-rt), ausente no compiler_builtins do Rust.
 *   aarch64: limpa D-cache ate o ponto de unificacao e invalida o I-cache
 *            por linha (o kernel libera DC CVAU/IC IVAU e CTR_EL0 em EL0).
 *   riscv64: a syscall riscv_flush_icache (259) faz fence.i em todas as harts. */


void __clear_cache(void* start, void* end) {
#if defined(__aarch64__)
    unsigned long ctr;
    __asm__ volatile("mrs %0, ctr_el0" : "=r"(ctr));
    unsigned long dline = 4u << ((ctr >> 16) & 0xF), iline = 4u << (ctr & 0xF);
    unsigned long a = (unsigned long)start & ~(dline - 1);
    for (; a < (unsigned long)end; a += dline) __asm__ volatile("dc cvau, %0" :: "r"(a) : "memory");
    __asm__ volatile("dsb ish" ::: "memory");
    a = (unsigned long)start & ~(iline - 1);
    for (; a < (unsigned long)end; a += iline) __asm__ volatile("ic ivau, %0" :: "r"(a) : "memory");
    __asm__ volatile("dsb ish\n isb" ::: "memory");
#elif defined(__riscv)
    register long a0 __asm__("a0") = (long)start;
    register long a1 __asm__("a1") = (long)end;
    register long a2 __asm__("a2") = 0;
    register long a7 __asm__("a7") = 259;
    __asm__ volatile("ecall" : "+r"(a0) : "r"(a1), "r"(a2), "r"(a7) : "memory");
#endif
}
