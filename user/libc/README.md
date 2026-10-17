# libc do Pulsar (C)

Biblioteca estática (`libpulsar.a`) para apps em C, espelhando a `plib` (Rust),
com um subconjunto da libc padrão suficiente para compilar bibliotecas como
QuickJS, BearSSL, stb e NanoSVG:

- `pulsar.h`: syscalls (AArch64 `svc` / RISC-V `ecall`), IPC, `SysInfo`/`ProcInfo`,
  `sys_getrandom` (bytes do virtio-rng).
- `malloc.c`: `malloc`, `free`, `calloc`, `realloc`, `malloc_usable_size` sobre
  a syscall `SBRK`; seguro entre threads (trava global após a primeira thread). Blocos de até 1 KiB usam classes de 16 em 16 bytes com
  listas livres próprias (O(1)); os maiores ficam numa lista ordenada com
  fusão de vizinhos.
- `string.c`: `mem*`, `str*`, `strdup`, `strcasecmp`... e formatação
  (`fmt_u`, `fmt_size`, `civil_time`, `utf8_encode`).
- `stdio.c`: `printf`/`snprintf` e afins (stb_sprintf), `sscanf`; a saída vai
  para a serial. Não há arquivos: `fopen` sempre falha.
- `stdlib.c`: `strtol`/`strtod`, `qsort`, `bsearch`, `rand`, `abort`, `exit`,
  `errno`, `assert`.
- `math.c`: o que o `compiler_builtins` do Rust não exporta (`frexp`, `modf`,
  `lrint`...). O resto da libm e o ponto flutuante por software vêm dele —
  o kernel não salva os registradores de FPU, então os apps C são compilados
  sem eles (`-mgeneral-regs-only` / `rv64imac`).
- `time.c`: `time`, `gettimeofday`, `clock_gettime`, `gmtime_r`, `mktime`
  (RTC + contador em ms; fuso sempre UTC).
- `graphics.c`: preenchimentos, retângulos arredondados e sombras com
  anti-aliasing, discos, linhas, tema claro/escuro e botões.
- `text.c` + `font_*.c`: texto UTF-8 (ASCII + Latin-1) com as fontes Noto
  geradas por `tools/genfonts.py`.
- `thread.c`: `thread_spawn` (threads do processo, em qualquer núcleo),
  `Mutex` sobre futex (`mutex_lock`/`mutex_unlock`) e `cpu_count`.
- `wm.c`: cliente do WM (`app_init`, `app_poll`, `app_commit`, `app_title`;
  `app_peek` consulta eventos sem esperar).
- `crt0_<arch>.s`: ponto de entrada (`_start` chama `main` e sai).
- `include/`: `pulsar.h` e os cabeçalhos padrão (`stdio.h`, `stdlib.h`,
  `string.h`, `math.h`, `time.h`, `ctype.h`, `errno.h`, `assert.h`,
  `inttypes.h`...). `stdint.h`, `stddef.h`, `stdarg.h` e `limits.h` vêm do clang.

O exemplo mínimo de app com janela é `user/hello_c/main.c`. O `Makefile` da
raiz compila a libc e os apps para a arquitetura escolhida (`ARCH=`).
