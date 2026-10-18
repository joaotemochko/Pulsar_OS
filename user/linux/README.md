# Programas Linux (musl)

Programas C POSIX comuns, compilados contra a **musl** original
(`user/third_party/musl`, sem modificações) e executados pela personalidade
Linux do kernel (`kernel/src/linux.rs`): os números de syscall e as
estruturas são os do Linux, então nada aqui conhece o Pulsar.

| Diretório | Vira | Ligação |
|---|---|---|
| `*.c` | `/bin/<nome>` e `/bin/<nome>.pulse` | estática (`libc.a`); o `.pulse` é a conversão feita por `tools/elf2pulse.py` |
| `dynamic/*.c` | `/bin/<nome>` | dinâmica: PIE com interpretador `/lib/ld-musl-<arch>.so.1`, contra `libc.so` e as bibliotecas de `lib/` |
| `lib/<nome>.c` | `/lib/lib<nome>.so` | biblioteca compartilhada (cabeçalho ao lado) |
| `rt/` | — | rotinas do compilador que faltam no `compiler_builtins` do Rust (`__multc3`), necessárias ao `libc.so` |

Para rodar, digite o nome no **Terminal** (ex.: `posixtest primeiro`); a saída
aparece na janela e Ctrl+C encerra o programa.

| Programa | O que verifica |
|---|---|
| `posixtest` | argv/envp, `uname`, `malloc`, `printf` de `double`, libm, stdio com caminhos relativos, diretórios, links simbólicos, `mmap` de arquivo, relógios, threads com mutex, TLS. Espera `primeiro` como argumento |
| `spawntest` | pipes, `fork`, `waitpid`, `posix_spawn` com redirecionamento, `execve`, `vfork`, `O_CLOEXEC`, vários filhos |
| `sigtest` | tratadores, `SA_SIGINFO`, máscaras, `SIGSEGV` com `siglongjmp`, tratador que faz `mprotect` e retorna, `ucontext` alterado, `sigaltstack`, `setitimer` + `pause`, `SIGCHLD`, `EINTR`, `pthread_kill`, `SIGPIPE`, morte por sinal |
| `iotest` | `poll`/`select`/`epoll`, `eventfd`, `socketpair`, `/dev`, `/proc`, DNS e HTTP de verdade (`iotest semrede` pula a rede) |
| `unixtest` | `socketpair` SEQPACKET, socket com caminho e abstrato (`listen`/`accept`), `SCM_RIGHTS` (pipe e arquivo para outro processo), TCP local em 127.0.0.1, `memfd` + `MAP_SHARED` entre processos, `MAP_SHARED` anônimo, `shm_open`/`shm_unlink`, `/proc/self/cmdline` e `status` |
| `oopstest` | falhas do kernel provocadas em filhos (panic, acesso inválido, panic com a trava do sistema de arquivos presa): só o filho morre e o disco segue utilizável |
| `svctest` | mata o gerenciador de janelas e o `fsd`; o `init` os reinicia (rode pelo Terminal: ele sobrevive e reconecta) |
| `wlkeys` | cliente Wayland com `libwayland-client` + `libxkbcommon`: digita usando o layout do sistema (abnt, abnt2, us, us-intl), com teclas mortas; `wlkeys trocar <layout>` testa a troca de layout com o programa aberto |
| `wlhello` | programa Wayland mínimo (protocolo direto, buffers em `memfd`): janela animada; `wlhello loop` fica aberto até Esc/Q; `wlhello parcial [loop]` só declara a área da bolinha em `wl_surface.damage` |
| `cxxtest` | C++ com a libc++ do LLVM: exceções atravessando quadros e dentro de threads, RTTI, contêineres, iostream, `std::thread` + mutex + variável de condição |
| `drawtest` | harfbuzz (acento combinante, kerning GPOS, árabe RTL, devanagari) e cairo (gradientes, curvas, texto, PNG, PDF) numa janela Wayland; `drawtest loop` fica aberto |
| `pangotest` | glib (laço principal, GRegex, GObject com propriedade e sinal, GVariant, GThreadPool, gio, `g_spawn_sync`), pango (markup, quebra de linha, troca de fonte por caractere em latim/✓/árabe/hebraico/devanágari, bidi) e gdk-pixbuf (lê, reduz e grava PNG) numa janela Wayland |
| `fonttest` | fontconfig + FreeType: confere a escolha das fontes (`sans-serif`, `serif`, `monospace`, Arial, Times New Roman e Courier New → Liberation, reserva para ✓ por cobertura) e desenha o texto numa janela Wayland; `fonttest loop` fica aberto |
| `wlpopup` | cena Wayland com `libwayland-client`: janela com geometria (margem de sombra, como no GTK), subsuperfície ARGB e popup com grab, que precisa ser espelhado e deslizado para caber na janela; termina com `fim: ok` quando um clique fora fecha o popup e o seguinte acerta a subsuperfície (teste interativo) |
| `wltest` | isolamento do serviço Wayland: cliente inválido só perde a própria conexão; matar o `wlsrv` não derruba conexões existentes e o `init` o reinicia |
| `dyntest` | ligação dinâmica: `libc.so` + `libdemo.so` (construtor, dado global), `dlopen`/`dlsym` |

Todos imprimem `ok`/`FALHA` por verificação e saem com o número de falhas.

As extensões `0x5000` (provocar um oops, para teste) e `0x5001` (pid de um
serviço do Pulsar) são chamadas do Pulsar disponíveis aos programas da
camada de compatibilidade; o Linux nunca usa esses números.

## Programa novo

Crie o `.c` no diretório certo e rode `make`: o Makefile compila a musl uma
vez (em `build/<arch>/sysroot`, sem tocar no sistema do host), liga o
programa com `clang --target=<arch>-linux-musl` + `ld.lld` e o copia para o
disco. Para testes automáticos, um `/etc/autorun` no disco (um programa com
argumentos por linha) roda tudo no boot com a saída na serial.

Ainda não há: conexões vindas de fora da VM (o `listen` vale só para
127.0.0.1), sockets `AF_UNIX` de datagrama com endereço e `MAP_SHARED` que
grave de volta num arquivo comum do ext2.
