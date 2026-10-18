# libc do Pulsar (C)

Biblioteca estática (`libpulsar.a`) para apps em C, espelhando a `plib` (Rust),
com um subconjunto da libc padrão suficiente para compilar bibliotecas como
QuickJS, BearSSL, Brotli, stb e NanoSVG (`memory.h` existe só por compatibilidade):

- `pulsar.h`: syscalls (AArch64 `svc` / RISC-V `ecall`), IPC, `SysInfo`/`ProcInfo`,
  `sys_getrandom` (bytes do virtio-rng), memoria virtual (`sys_mmap`,
  `sys_munmap`, `sys_mprotect`), arquivos do ext2 com descritores
  (`sys_open`, `sys_read`, `sys_getdents`, `sys_stat`, `sys_mkdir`,
  `sys_rename`...), pipes (`sys_pipe`, `sys_fcntl`, `sys_dup`) e processos
  por caminho (`sys_spawn_ex` com argv/envp, `sys_wait`). Os retornos sao o
  valor ou `-errno`.
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
  `lrint`...); o resto da libm vem dele. Os apps usam a FPU (o kernel salva
  os registradores de FP/SIMD de cada processo): são compilados hard-float
  (`aarch64` com NEON, `rv64imafdc`/`lp64d`), e o `compiler_builtins` é o do
  alvo de usuário (`aarch64-unknown-none` / `riscv64gc-unknown-none-elf`).
- `time.c`: `time`, `gettimeofday`, `clock_gettime`, `gmtime_r`, `mktime`
  (RTC + contador em ms; fuso sempre UTC).
- `graphics.c`: preenchimentos, retângulos arredondados e sombras com
  anti-aliasing, discos, linhas, tema claro/escuro e botões.
- `text.c` + `font_*.c`: texto UTF-8 (ASCII + Latin-1) com as fontes Noto
  geradas por `tools/genfonts.py`.
- `thread.c`: `thread_spawn` (threads do processo, em qualquer núcleo),
  `Mutex` sobre futex (`mutex_lock`/`mutex_unlock`) e `cpu_count`.
- `wm.c`: cliente do WM (`app_init`, `app_poll`, `app_commit`, `app_commit_rect` — so o retangulo alterado e recomposto —, `app_title`;
  `app_peek` consulta eventos sem esperar).
- `crt0_<arch>.s`: ponto de entrada (`_start` chama `main` e sai).
- `include/`: `pulsar.h` e os cabeçalhos padrão (`stdio.h`, `stdlib.h`,
  `string.h`, `math.h`, `time.h`, `ctype.h`, `errno.h`, `assert.h`,
  `inttypes.h`...). `stdint.h`, `stddef.h`, `stdarg.h` e `limits.h` vêm do clang.

O exemplo mínimo de app com janela é `user/hello_c/main.c`. O `Makefile` da
raiz compila a libc e os apps para a arquitetura escolhida (`ARCH=`).
