# Firefox no Pulsar OS

Porte do Firefox ESR (hoje **153.5.0esr**) para a camada de compatibilidade
Linux: um programa Linux comum (musl, libc++, ligação dinâmica com o GTK 3 e as
demais bibliotecas de `/usr/lib`), com o backend Wayland do GTK — o serviço
`wlsrv` do Pulsar faz a ponte com o gerenciador de janelas.

## Compilar

```sh
make ARCH=aarch64 musl              # sysroot com GTK, alsa-lib etc.
sh tools/build_firefox.sh aarch64   # baixa o fonte, aplica os patches, configure + build
```

O fonte oficial é baixado (e conferido por SHA-256) em `~/firefox-src`; o
objdir fica em `~/firefox-src/obj-<arch>`. `tools/build_firefox.sh <arch> <cmd>`
roda outro comando do `mach` (por exemplo `configure` ou `package`).

Ferramentas do host: LLVM ≥ 19 (`~/llvm22`), Rust estável com o alvo
`<arch>-unknown-linux-musl`, cbindgen e Node.js (`~/node-local`).

## `mozconfig`

- `--target=<arch>-unknown-linux-musl --with-sysroot=build/<arch>/sysroot`
- `--enable-default-toolkit=cairo-gtk3-wayland-only` (sem X11)
- áudio só por ALSA (`--enable-audio-backends=alsa`)
- desligados neste primeiro porte: sandbox (seccomp), D-Bus, WebRTC, crash
  reporter, updater, bibliotecas em wasm (RLBox)

## Patches (`patches/`)

Cada um é uma adaptação pequena, válida para qualquer Linux com musl/libc++
(não só para o Pulsar), aplicada uma vez sobre o fonte original:

| Patch | Por quê |
|---|---|
| `0001-musl-malloc-sem-noexcept` | o mozjemalloc declara `free`, `memalign`... como `noexcept`, como a glibc; a musl não (igual ao Android/macOS) |
| `0002-musl-mutex-sem-adaptive` | `PTHREAD_MUTEX_ADAPTIVE_NP` é extensão da glibc: com a musl usa o mutex normal |
| `0003-musl-libcxx-visibilidade` | com libc++ o `#pragma GCC visibility push(hidden)` do `gcc_hidden.h` conflita com os atributos de visibilidade por classe; usa `-fvisibility=hidden`, como no FreeBSD |
