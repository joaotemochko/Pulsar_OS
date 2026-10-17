# Bibliotecas de terceiros

Código e dados de outros projetos usados pelo **Vela** (e, no caso do
stb_sprintf, pela libc). Todos têm licenças permissivas compatíveis com a
licença MIT do Pulsar OS. Estão aqui sem modificações, exceto pelos patches
listados abaixo.

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

`user/vela/certs.c` é gerado a partir da lista de autoridades certificadoras
da Mozilla (via [curl.se/ca/cacert.pem](https://curl.se/ca/cacert.pem), MPL 2.0).

## Patches locais

- **QuickJS** (`quickjs.c`): `CONFIG_ATOMICS` desligado quando `__pulsar__`
  está definido (os `Atomics` exigem pthreads, que o Pulsar não tem).
- **stb_sprintf** (`stb_sprintf.h`): RISC-V de 64 bits reconhecido como
  plataforma de ponteiros de 64 bits (`stbsp__uintptr`).

## Como o build usa

O `Makefile` da raiz compila o BearSSL e o QuickJS em `build/<arch>/libbearssl.a`
e `libquickjs.a` com a libc do Pulsar (`user/libc/include`). Os cabeçalhos
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
for f in NotoSans-Regular NotoSans-Bold NotoSans-Italic NotoSans-BoldItalic NotoSansMono-Regular NotoSansMono-Bold; do
  pyftsubset /usr/share/fonts/truetype/noto/$f.ttf --unicodes="$U" --output-file=$f.ttf \
    --no-hinting --desubroutinize --layout-features='' --drop-tables+=GSUB,GPOS,GDEF,DSIG
done
pyftsubset /usr/share/fonts/truetype/dejavu/DejaVuSans.ttf --unicodes="U+2010-2BFF,U+FFFD,U+00D7,U+00F7" \
  --output-file=DejaVuSans-Symbols.ttf --no-hinting --desubroutinize --layout-features='' --drop-tables+=GSUB,GPOS,GDEF,DSIG
```

**Certificados**: compile a ferramenta `brssl` do BearSSL no host
(`make -C bearssl-0.6 build/brssl`), baixe o `cacert.pem` e rode
`brssl ta cacert.pem`; o resultado substitui o corpo de `user/vela/certs.c`
(mantendo o cabeçalho e as duas variáveis `vela_tas`/`vela_tas_num` do fim).
