# Ambiente glibc (camada Linux)

Programas Linux prontos (o Firefox baixado de mozilla.org, por exemplo) são
ligados à glibc e esperam as bibliotecas de uma distribuição. Este ambiente
traz essas bibliotecas dos pacotes binários do **Debian 13 (trixie)**, sem
modificação, ao lado do mundo musl do Pulsar.

## Arquivos

| Arquivo | O que é |
|---|---|
| `pacotes.txt` | pacotes pedidos (as dependências são resolvidas sozinhas) |
| `base-<arch>.lock` | versão e SHA-256 de cada pacote instalado (build reproduzível) |
| `pacotes-teste.txt` | pacotes só para testes (`gtk-3-examples`) |
| `glibctest.sh` | teste do bash e do coreutils do Debian rodando no Pulsar |

## Montar

```sh
make ARCH=aarch64 glibc     # build/<arch>/glibcroot (o "make disk" já inclui)
make ARCH=aarch64 disk GLIBC=0   # disco sem o ambiente glibc
```

`tools/glibc_env.py` confere o `InRelease` do Debian com o `gpgv` e o chaveiro
oficial, confere por SHA-256 o índice e cada `.deb`, resolve as dependências e
extrai os pacotes. Os `.deb` ficam em `~/debian-cache` (`GLIBC_CACHE`). Para
atualizar as versões, apague o `.lock` e rode de novo.

Os scripts de instalação do Debian não rodam. O que eles gerariam é feito no
build: o `loaders.cache` do gdk-pixbuf, o banco MIME (`update-mime-database`),
o `icon-theme.cache` e os esquemas do GSettings (compilados junto com os da
musl, no mesmo `/usr/share/glib-2.0/schemas`). Documentação, manuais e
traduções fora de `GLIBC_LINGUAS` (pt, pt_BR, en) ficam de fora.

## Layout no disco

- bibliotecas em `/usr/lib/<arch>-linux-gnu` (multiarch do Debian), separadas de `/usr/lib` (musl);
- carregador em `/lib/ld-linux-aarch64.so.1` ou `/lib/ld-linux-riscv64-lp64d.so.1`;
- programas do Debian em `/usr/bin`.
