# Vela — navegador do Pulsar OS (C)

O kernel oferece só sockets TCP/UDP e bytes aleatórios (`GETRANDOM`); todo o
resto do navegador roda no userspace:

| Arquivo | O que faz |
|---|---|
| `net.c` | DNS (resolvedor do QEMU, `10.0.2.3`) com cache, HTTP/1.1 (chunked, redirecionamentos, POST), cookies |
| `tls.c`, `certs.c` | HTTPS com o BearSSL: TLS 1.0–1.2, validação X.509 pelas raízes da Mozilla, retomada de sessão por host |
| `url.c` | URLs absolutas/relativas (RFC 3986) e o que se digita na barra (palavras soltas viram busca) |
| `dom.c` | árvore DOM e parser HTML tolerante (tags implícitas, texto bruto, entidades, modo quirks) |
| `css.c` | folhas de estilo, seletores, cascata, `var()`, `calc()`, cores modernas, `@media`, `@supports` |
| `layout.c` | caixas → display list: blocos, linhas, inline-block, flex, grid, tabelas, posicionamento |
| `paint.c`, `font.c`, `image.c` | pintura, texto TrueType (Noto + símbolos DejaVu) e imagens PNG/JPEG/GIF/SVG |
| `js.c`, `prelude.js` | QuickJS: o DOM exposto por funções nativas e a API do navegador escrita em JS |
| `main.c` | interface, laço de eventos, carga da página, formulários, histórico |
| `work.c` | threads: fila de tarefas dos workers e entrega dos resultados à thread principal |

## O que funciona

- **HTTPS** com cadeado na barra; erros de certificado aparecem numa página
  de erro (expirado, autoridade desconhecida, nome errado, TLS 1.3 exigido).
- **CSS**: seletores completos (atributos, combinadores, `:nth-child`, `:not`,
  `:is`, `:where`), especificidade, `!important`, propriedades `--custom`,
  `calc()/min()/max()/clamp()`, `rgb/hsl/oklch/lab/color-mix/light-dark`,
  `@media` (inclusive a sintaxe `width >= 600px`), `@supports`, `@layer`;
  flexbox (wrap, grow/shrink, justify/align, `order`, `gap`), grid
  (`repeat()`, `fr`, `minmax`, `auto-fill`), tabelas (colspan, larguras
  min/max), `position` relative/absolute/fixed com `z-index`, `transform:
  translate`, `overflow: hidden`, listas, bordas arredondadas, gradientes
  (como cor sólida), fundo com imagem.
- **JavaScript** (QuickJS, ES2023): DOM (Node/Element/Document, `innerHTML`,
  `querySelector`, `classList`, `dataset`, `style`), eventos com captura e
  bolha, `setTimeout`/`setInterval`/`requestAnimationFrame`, `fetch`,
  `XMLHttpRequest`, `URL`, `localStorage`, `history.pushState`, módulos ES
  (`<script type=module>`, `import()`, import maps), custom elements,
  `document.write`, scripts inseridos dinamicamente.
- **Formulários**: campos de texto, senha, checkbox, radio, select, textarea,
  envio GET e POST; Enter envia.
- Links, histórico (Voltar / Backspace), âncoras `#id`, `<meta refresh>`.

## Desempenho

O Vela usa vários núcleos:

- **thread principal**: interface, DOM, JavaScript, estilo, layout e pintura;
- **thread de rede**: todas as conexões HTTP/HTTPS ao mesmo tempo, sem
  bloquear — a página nunca espera a rede para responder a cliques e rolagem;
- **workers** (um por núcleo livre, até 4): analisam folhas de estilo,
  decodificam imagens e rasterizam SVG enquanto a página já aparece;
- o **cálculo de estilo** divide o DOM em subárvores que a principal e os
  workers estilizam em paralelo (cada um com sua arena e seu filtro de
  ancestrais).

- Até 8 conexões em paralelo (TCP e TLS não bloqueantes), conexões
  reaproveitadas por host (keep-alive) e sessões TLS retomadas; ChaCha20-Poly1305
  é preferido por ser mais rápido que AES em software.
- Folhas de estilo, scripts e imagens são baixados em lotes paralelos. As
  declarações CSS só são analisadas quando a regra casa com algum elemento.
- A página aparece assim que HTML + CSS chegam; depois, scripts e lotes de
  imagens se alternam, um por volta do laço, sem travar rolagem e cliques.
  Mudanças vindas do JS refazem o layout com intervalo proporcional ao seu custo.
- Nada de mensagens de progresso: só uma linha fina sob a barra enquanto a
  página carrega, e o endereço de um link num balão quando o mouse passa.

## Limites conhecidos

- TLS 1.3 não existe no BearSSL: servidores que só aceitam 1.3 falham.
- WebP, AVIF, vídeo, áudio, canvas (o contexto 2D existe, mas não desenha).
- Floats viram blocos inline (colunas lado a lado funcionam; texto não contorna).
- Sem `:hover` e sem animações/transições CSS.
- O JavaScript roda com ponto flutuante por software e sob emulação no QEMU:
  páginas muito pesadas demoram, e cada script tem 15 s de orçamento.

## Testar

- Internet: `wikipedia.org`, `news.ycombinator.com`, `github.com`,
  ou palavras soltas para buscar no DuckDuckGo Lite.
- Local: no host, `python3 -m http.server 8080` e abra `10.0.2.2:8080/`.
- O log serial mostra cada requisição (`[vela] GET ...` com tempo e tamanho),
  o tempo de estilo/layout e erros de JavaScript (`[vela/js] ...`).
