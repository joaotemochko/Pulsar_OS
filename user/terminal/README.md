# Terminal do Pulsar OS (em C)

Primeiro app funcional escrito em C sobre a libc mínima do Pulsar. Roda
como cliente do Window Manager: pede uma superfície, desenha nela, e recebe
teclado/mouse via o protocolo IPC do WM (SVC_WM).

## Recursos
- Renderização na própria superfície (isolada), composta pelo WM.
- Fonte Poppins anti-aliased (mesma da GUI Rust, portada para C).
- Entrada de teclado roteada pelo WM (foco).
- Comandos: help, echo <txt>, clear, ver, files (conta arquivos via IPC
  do filesystem daemon).
- Cursor de texto piscando, histórico de linhas.

## Compilar
```
cd user/terminal && ./build.sh
python3 tools/mkpulse.py terminal.elf terminal.pulse 0x11400000
```
