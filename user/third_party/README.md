# Bibliotecas de terceiros

Código e dados de outros projetos usados pelo **Vela** (que é um submódulo git
em `user/vela`, repositório [VelaBrowser](https://github.com/joaotemochko/VelaBrowser)),
pela libc e pela camada de compatibilidade Linux. A maioria tem licenças
permissivas (MIT, BSD, zlib, Apache); glib, pango, fribidi, atk, gdk-pixbuf e
GTK são LGPL-2.1+ e cairo é LGPL-2.1/MPL-1.1 — ligadas estaticamente nos
programas Linux, com os fontes completos e o build que as religa neste
repositório. Os cabeçalhos UAPI do Linux são GPL-2.0 com a nota de syscall
(só interface). Estão aqui sem modificações, exceto pelos patches listados
abaixo.

| Diretório | Projeto | Versão | Licença | Para quê |
|---|---|---|---|---|
| `bearssl/` | [BearSSL](https://bearssl.org) | 0.6 | MIT (`bearssl/LICENSE.txt`) | TLS 1.0–1.2 e X.509 (HTTPS) |
| `quickjs/` | [QuickJS](https://bellard.org/quickjs/) | 2026-06-04 | MIT (`quickjs/LICENSE`) | motor JavaScript (ES2023, módulos) |
| `stb/stb_truetype.h` | [stb](https://github.com/nothings/stb) | 1.26 | domínio público / MIT | rasterização de fontes TrueType |
| `stb/stb_image.h` | stb | 2.30 | domínio público / MIT | PNG, JPEG, GIF, BMP |
| `stb/stb_sprintf.h` | stb | 1.10 | domínio público / MIT | `printf` da libc |
| `nanosvg/` | [NanoSVG](https://github.com/memononen/nanosvg) | master (2024) | zlib (`nanosvg/LICENSE.txt`) | imagens SVG |
| `fonts/NotoSans*.ttf` | Noto Sans / Noto Sans Mono | Debian 20201225 | OFL 1.1 (`fonts/LICENSE-Noto.txt`) | texto das páginas |
| `fonts/DejaVuSans-Symbols.ttf` | DejaVu Sans | 2.37 | Bitstream Vera (`fonts/LICENSE-DejaVu.txt`) | símbolos ausentes no Noto (−, setas, ✓, formas) |
| `fonts/NotoSerif*.ttf` | Noto Serif | Debian 20201225 | OFL 1.1 (`fonts/LICENSE-Noto.txt`) | família `serif` |
| `fonts/PulsarMetric*.ttf` | Liberation Sans/Serif (renomeadas) | 2.1.5 | OFL 1.1 (`fonts/LICENSE-PulsarMetric.txt`) | Arial/Helvetica e Times com as mesmas métricas |
| `brotli/` | [Brotli](https://github.com/google/brotli) (só o decodificador) | 1.1.0 | MIT (`brotli/LICENSE`) | fontes WOFF2 |
| `libffi/` | [libffi](https://sourceware.org/libffi/) | 3.4.6 (sha256 `b0dea9df…895a4e`) | MIT (`libffi/LICENSE`) | chamadas dinamicas da libwayland |
| `wayland/` | [Wayland](https://wayland.freedesktop.org) | 1.23.1 (sha256 `864fb2a8…1e5ed`) | MIT (`wayland/COPYING`) | `libwayland-client` e `wayland-scanner` (no host) |
| `wayland-protocols/` | wayland-protocols | 1.38 (sha256 `ff17292c…81dbe`) | MIT (`wayland-protocols/COPYING`) | protocolo `xdg-shell` |
| `libxkbcommon/` | [libxkbcommon](https://xkbcommon.org) | 1.7.0 (sha256 `65782f0a…b247`) | MIT (`libxkbcommon/LICENSE`) | mapas de teclado XKB e teclas mortas (compose) |
| `expat/` | [Expat](https://libexpat.github.io) | 2.6.4 (sha256 `a695629d…f55ff7ee`) | MIT (`expat/COPYING`) | XML da configuração do fontconfig |
| `freetype/` | [FreeType](https://freetype.org) | 2.13.3 (sha256 `05503506…6f063289`) | FTL (`freetype/docs/FTL.TXT`; opção BSD-like, com crédito) | rasterização de fontes para os programas Linux |
| `fontconfig/` | [Fontconfig](https://www.freedesktop.org/wiki/Software/fontconfig/) | 2.15.0 (sha256 `63a0658d…3335d7c0e`) | MIT-like (`fontconfig/COPYING`) | descoberta e escolha de fontes; `fc-list`, `fc-match`, `fc-cache` |
| `llvm-runtimes/` | [LLVM](https://llvm.org) libc++, libc++abi, libunwind, compiler-rt, libc (só os cabeçalhos que a libc++ usa) (+ `runtimes/`, `cmake/`, `third-party/`, `llvm/cmake`) | 22.1.8 (do `llvm-project-22.1.8.src.tar.xz`, sha256 `922f1817a0df7b1489272d18134ee0087a8b068828f87ac63b9861b1a9965888`) | Apache 2.0 com exceção LLVM (`llvm/LICENSE.TXT`) | biblioteca C++ dos programas Linux |
| `linux-headers/<arch>/` | cabeçalhos UAPI do Linux (pacote `linux-headers` do Alpine 3.20) | 6.6-r0 (apk sha256 aarch64 `bc25332b…6cac1b`, riscv64 `86884cb4…e687c1d`) | GPL-2.0 com a nota de syscall (só cabeçalhos de interface) | `linux/*.h`, `asm/*.h` no sysroot |
| `zlib/` | [zlib](https://zlib.net) | 1.3.1 (sha256 `38ef96b8…f883e32`) | zlib | compressão |
| `libpng/` | [libpng](http://www.libpng.org) | 1.6.44 (sha256 `60c4da1d5b7f0aa8d158da48e8f8afa9773c1c8baa5d21974df61f1886b8ce8e`) | libpng-2.0 (`libpng/LICENSE`) | PNG |
| `pixman/` | [pixman](https://pixman.org) | 0.44.2 (sha256 `6349061c…71973466`) | MIT (`pixman/COPYING`) | rasterização de pixels (cairo) |
| `harfbuzz/` | [HarfBuzz](https://harfbuzz.github.io) | 10.1.0 (sha256 `6ce3520f2d089a33cef0fc48321334b8e0b72141f6a763719aaaecd2779ecb82`) | MIT "Old" (`harfbuzz/COPYING`) | shaping de texto |
| `cairo/` | [cairo](https://cairographics.org) | 1.18.2 (sha256 `a62b9bb4…85889d45a`) | LGPL-2.1 ou MPL-1.1 (`cairo/COPYING`) | desenho 2D |
| `pcre2/` | [PCRE2](https://pcre2project.github.io/pcre2/) | 10.44 (sha256 `d34f02e113cf7193a1ebf2770d3ac527088d485d4e047ed10e5d217c6ef5de96`) | BSD-3 (`pcre2/LICENCE`) | `GRegex` do glib |
| `glib/` | [GLib](https://gitlab.gnome.org/GNOME/glib) | 2.82.4 (sha256 `37dd0877fe964cd15e9a2710b044a1830fb1bd93652a6d0cb6b8b2dff187c709`) | LGPL-2.1+ (`glib/COPYING`) | glib, gobject, gio, gmodule |
| `fribidi/` | [GNU FriBidi](https://github.com/fribidi/fribidi) | 1.0.16 (sha256 `1b1cde5b235d40479e91be2f0e88a309e3214c8ab470ec8a2744d82a5a9ea05c`) | LGPL-2.1+ (`fribidi/COPYING`) | texto bidirecional |
| `pango/` | [Pango](https://gitlab.gnome.org/GNOME/pango) | 1.54.0 (sha256 `8a9eed75021ee734d7fc0fdf3a65c3bba51dfefe4ae51a9b414a60c70b2d1ed8`) | LGPL-2.1+ (`pango/COPYING`) | layout de texto |
| `gdk-pixbuf/` | [gdk-pixbuf](https://gitlab.gnome.org/GNOME/gdk-pixbuf) | 2.42.12 (sha256 `b9505b3445b9a7e48ced34760c3bcb73e966df3ac94c95a148cb669ab748e3c7`) | LGPL-2.1+ (`gdk-pixbuf/COPYING`) | imagens |
| `atk/` | [ATK](https://gitlab.gnome.org/GNOME/atk) | 2.38.0 (sha256 `ac4de2a4ef4bd5665052952fe169657e65e895c5057dffb3c2a810f6191a0c36`) | LGPL-2.1+ (`atk/COPYING`) | acessibilidade (exigida pelo GTK 3) |
| `libepoxy/` | [libepoxy](https://github.com/anholt/libepoxy) | 1.5.10 (sha256 do .tar.gz do GitHub `a7ced37f4102b745ac86d6a70a9da399cc139ff168ba6b8002b4d8d43c900c15`) | MIT (`libepoxy/COPYING`) | despacho de OpenGL/EGL |
| `khronos/` | [EGL-Registry](https://github.com/KhronosGroup/EGL-Registry) (`EGL/*.h`, `KHR/khrplatform.h`) | commit em `khronos/COMMIT` | Apache-2.0 / MIT (cabeçalhos) | cabeçalhos EGL para a libepoxy |
| `gtk3/` | [GTK](https://gitlab.gnome.org/GNOME/gtk) | 3.24.52 (sha256 `80931fa472a77b9a164f6740e3c0b444fac6770054632d35a7ff9d679e5e7b9f`) | LGPL-2.1+ (`gtk3/COPYING`) | toolkit (backend Wayland) |
| `xkeyboard-config/` | [xkeyboard-config](https://gitlab.freedesktop.org/xkeyboard-config/xkeyboard-config) | 2.43 (sha256 `c810f362c82a834ee89da81e34cd1452c99789339f46f6037f4b9e227dd06c01`) | MIT (`xkeyboard-config/COPYING`) | dados XKB em `/usr/share/X11/xkb` |
| `alsa-lib/` | [alsa-lib](https://www.alsa-project.org) | 1.2.16.1 (sha256 `f740db7f488255944ffd4428416ee3390a96742856916433df468c281436480e`) | LGPL-2.1+ (`alsa-lib/COPYING`) | áudio/MIDI (exigida pelo Firefox) |
| `fonts-linux/` | Noto, Liberation, DejaVu (fontes completas, com tabelas OpenType) | Debian: fonts-noto-core 20201225, fonts-liberation 2.1.5, fonts-dejavu-core 2.37 | OFL 1.1 / Bitstream Vera (`fonts-linux/LICENSE-*.txt`) | `/usr/share/fonts/pulsar` dos programas Linux |
| `musl/` | [musl libc](https://musl.libc.org) | 1.2.5 (sha256 `a9a118bb…c75e4`) | MIT (`musl/COPYRIGHT`) | libc dos programas Linux (`user/linux`); sem modificações |

`user/vela/certs.c` é gerado a partir da lista de autoridades certificadoras
da Mozilla (via [curl.se/ca/cacert.pem](https://curl.se/ca/cacert.pem), MPL 2.0).

Diretórios podados (o resto está sem modificações): `test/` e `docs/` da
libc++/libc++abi/libunwind/compiler-rt/libc, `third-party/benchmark`, `testsuite/` do GTK, `test/` e `perf/` do harfbuzz e do cairo (o
`meson.build` deles só entra nesses diretórios com `tests` ligado), e do
`llvm` só vieram `cmake/`, `utils/llvm-lit` e a licença.

## Patches locais

- **QuickJS** (`quickjs.c`): `CONFIG_ATOMICS` desligado quando `__pulsar__`
  está definido (os `Atomics` exigem pthreads, que o Pulsar não tem).
- **stb_sprintf** (`stb_sprintf.h`): RISC-V de 64 bits reconhecido como
  plataforma de ponteiros de 64 bits (`stbsp__uintptr`).

## Como o build usa

O `Makefile` da raiz compila o BearSSL, o QuickJS e o Brotli em
`build/<arch>/libbearssl.a`, `libquickjs.a` e `libbrotlidec.a` com a libc do
Pulsar (`user/libc/include`); elas são recompiladas se as flags do compilador
mudarem (`build/<arch>/.cc-flags`). Os cabeçalhos
`stb_*` e NanoSVG são incluídos diretamente pelos fontes do Vela, e as fontes
TrueType são embutidas no binário por `tools/bin2c.py`.

Não há FPU salva pelo kernel, então todo ponto flutuante é por software; a
libm e as rotinas de soft-float vêm do `compiler_builtins` do Rust, já
instalado para os alvos (`$(BUILTINS)` no `Makefile`).

## Regenerar dados

**Fontes** (precisa do `fonttools`): os subconjuntos cobrem latim, grego,
cirílico e pontuação/símbolos comuns.

```sh
U="U+0020-007E,U+00A0-024F,U+0300-036F,U+0370-03FF,U+0400-04FF,U+2000-206F,U+20A0-20CF,U+2100-215F,U+2190-21FF,U+2200-22FF,U+2300-23FF,U+25A0-25FF,U+2600-26FF,U+2713-2717,U+FFFD"
for f in NotoSans-Regular NotoSans-Bold NotoSans-Italic NotoSans-BoldItalic NotoSansMono-Regular NotoSansMono-Bold \
         NotoSerif-Regular NotoSerif-Bold NotoSerif-Italic NotoSerif-BoldItalic; do
  pyftsubset /usr/share/fonts/truetype/noto/$f.ttf --unicodes="$U" --output-file=$f.ttf \
    --no-hinting --desubroutinize --layout-features='' --drop-tables+=GSUB,GPOS,GDEF,DSIG
done
pyftsubset /usr/share/fonts/truetype/dejavu/DejaVuSans.ttf --unicodes="U+2010-2BFF,U+FFFD,U+00D7,U+00F7" \
  --output-file=DejaVuSans-Symbols.ttf --no-hinting --desubroutinize --layout-features='' --drop-tables+=GSUB,GPOS,GDEF,DSIG
# Liberation (pasta truetype/liberation) com os mesmos parametros; depois a
# familia e renomeada para "Pulsar Metric Sans/Serif" (clausula de Reserved
# Font Name da OFL) e os arquivos viram PulsarMetric{Sans,Serif}-*.ttf.
```

**Certificados**: compile a ferramenta `brssl` do BearSSL no host
(`make -C bearssl-0.6 build/brssl`), baixe o `cacert.pem` e rode
`brssl ta cacert.pem`; o resultado substitui o corpo de `user/vela/certs.c`
(mantendo o cabeçalho e as duas variáveis `vela_tas`/`vela_tas_num` do fim).
