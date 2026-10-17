# Rede (C)

Monitor da rede virtio-net: estado da conexão, IP, gateway e DNS do modo
`user` (SLIRP) do QEMU, e envio de pings UDP de teste ao gateway pela
syscall `NET_UDP_SEND`. Atalhos: Enter envia um ping; menu **Rede > Atualizar**.

Compilado pelo `Makefile` da raiz.
