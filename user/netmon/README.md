# Monitor de Rede do Pulsar (netmon, em C)

Primeiro app com REDE do Pulsar OS. Usa as syscalls de rede do kernel
(SYS_NET_STATUS, SYS_NET_UDP_SEND) sobre o driver virtio-net.

## Recursos
- Mostra o status da rede (online/offline) e o IP obtido.
- Botao "Enviar Ping UDP": dispara um datagrama UDP ao gateway (10.0.2.2:9)
  e conta quantos foram enviados.

## Rede no Pulsar
O kernel implementa virtio-net + Ethernet + ARP + IPv4 + UDP (kernel/src/net.rs).
No boot resolve o MAC do gateway via ARP e envia um datagrama de teste.
O QEMU precisa das flags: -netdev user,id=n0 -device virtio-net-device,netdev=n0
(ja incluidas no runner do .cargo/config.toml e nos scripts de teste).

Endereco padrao (SLIRP do QEMU): IP 10.0.2.15, gateway 10.0.2.2.
