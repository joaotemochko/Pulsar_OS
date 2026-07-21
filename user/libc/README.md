# libc mínima do Pulsar OS

libc freestanding para escrever apps do Pulsar em C, compilados com clang
(cross para `aarch64-none-elf`) e linkados com `ld.lld` — sem glibc.

## Por que não glibc
glibc espera o ABI Linux (centenas de syscalls, TLS, dynamic loader,
crt0/crti/crtn). O Pulsar tem ~25 syscalls próprias e formato `.pulse`
custom. Uma libc mínima dá o C que precisamos sem essa camada de
compatibilidade Linux inteira.

## O que tem
- `pulsar.h`: tipos, wrappers de syscall (`svc #0`, num em x8, args x0-x2),
  IPC, e primitivas de framebuffer.
- `string.c`: memcpy/memset/strlen.
- `graphics.c`: fb_open/fill/pixel/blend (renderização por software).
- `crt0.s`: startup (`_start` → `main` → `sys_exit`).

## Compilar um app
```
cd user/<app> && ./build.sh          # gera <app>.elf
python3 tools/mkpulse.py <app>.elf <app>.pulse <stack_top_hex>
```
O `mkpulse.py` é agnóstico de linguagem: lê PT_LOAD de qualquer ELF64 e
aplica W^X por segmento. Apps C e Rust convivem no mesmo sistema.

## Regras de link
- Base fora do range MMIO (`0x08000000..0x0a010000`).
- Segmentos R/W/X alinhados a 4KB (o loader impõe W^X).
- `ENTRY(_start)`.
