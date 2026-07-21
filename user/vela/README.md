# Vela Pulsar — o navegador do Pulsar OS

Primeiro navegador web nativo do Pulsar OS, escrito em C. Faz HTTP GET
usando a pilha TCP nativa do kernel (kernel/src/net.rs) e renderiza o
texto extraido do HTML.

## Como funciona
1. A barra de endereco mostra o alvo. O botao "Ir" dispara sys_http_get().
2. O kernel abre uma conexao TCP (handshake SYN/SYN-ACK/ACK), envia
   "GET / HTTP/1.0", recebe a resposta ate o FIN e devolve o corpo.
3. render_html() remove as tags <...>, pula <script>/<style>, colapsa
   espacos e extrai o texto, que e desenhado com quebra de linha.

## Alcance atual (honesto)
- HTTP/1.0 apenas. Nao ha HTTPS/TLS -> a maioria dos sites modernos
  responde "426 Upgrade Required" (exigem HTTPS). O Vela funciona; a web
  e que virou HTTPS-only.
- Sem DNS: navega por IP (o kernel resolve so o MAC do gateway via ARP).
- render_html e simples: extrai texto, sem CSS/imagens/links clicaveis.

## Testar com um servidor HTTP proprio
No host, suba um servidor que sirva HTML por HTTP puro (porta 8080) e
aponte target_ip para 10.0.2.2 (gateway SLIRP = host). Foi assim que
validamos: o Vela baixou e renderizou uma pagina HTML real.

## Proximos passos
DNS (para digitar dominios), parser de HTML com paragrafos/titulos/links,
e eventualmente HTTPS (grande: exige TLS).
