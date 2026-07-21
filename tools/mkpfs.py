#!/usr/bin/env python3
"""mkpfs.py — constroi uma imagem de disco PulsarFS v2 (com escrita).

Uso: mkpfs.py <saida.img> [--slack N] <arquivo1> [arquivo2 ...]

Cada arquivo recebe uma extensao FIXA = ceil(tamanho/512) + folga de
`--slack` setores (default 8 = 4KB), permitindo edicao/crescimento
in-place sem realocacao. Arquivos de texto (.txt) ganham folga extra.

Layout (setor = 512B):
  setor 0      superbloco {magic "PFS1", version=2, file_count, table_sectors=8}
  setores 1..8 tabela: 64 entradas de 64B
               {name[32], start u32, size u32, kind u32, capacity_sec u32, pad16}
  setor 9+     dados, cada arquivo em sua extensao reservada
"""
import os, struct, sys

MAGIC = 0x31534650  # "PFS1"
SECTOR = 512
TABLE_SECTORS = 8
DATA_START = 1 + TABLE_SECTORS
IMG_SIZE = 8 * 1024 * 1024
IMG_SECTORS = IMG_SIZE // SECTOR

def main():
    args = sys.argv[1:]
    if not args:
        sys.exit(__doc__)
    slack = 8
    if args[0] == '--slack':
        slack = int(args[1]); args = args[2:]
    out_path, files = args[0], args[1:]
    assert len(files) <= 64, "maximo 64 arquivos"

    table = bytearray(TABLE_SECTORS * SECTOR)
    blobs = bytearray()
    sector = DATA_START

    for i, path in enumerate(files):
        data = open(path, 'rb').read()
        name = os.path.basename(path).encode()[:31]
        is_txt = path.endswith('.txt')
        kind = 1 if path.endswith('.pulse') else 2
        used = (len(data) + SECTOR - 1) // SECTOR
        # capacidade: setores usados + folga (texto ganha mais para editar)
        cap = max(used + slack, 1)
        if is_txt:
            cap = max(cap, 16)  # >= 8KB para textos
        entry = struct.pack('<32sIIII16x', name, sector, len(data), kind, cap)
        table[i * 64:(i + 1) * 64] = entry
        # grava os dados + zeros ate preencher a extensao
        blobs += data + b'\0' * (cap * SECTOR - len(data))
        print(f"[mkpfs] {name.decode():24s} setor {sector:5d}  {len(data):7d} B  "
              f"cap={cap} sec  kind={kind}")
        sector += cap

    assert sector <= IMG_SECTORS, f"imagem excede {IMG_SIZE} bytes"
    sb = struct.pack('<IIII', MAGIC, 2, len(files), TABLE_SECTORS)
    img = bytearray(IMG_SIZE)
    img[0:len(sb)] = sb
    img[SECTOR:SECTOR + len(table)] = table
    img[DATA_START * SECTOR:DATA_START * SECTOR + len(blobs)] = blobs

    open(out_path, 'wb').write(img)
    print(f"[mkpfs] {out_path}: {len(files)} arquivo(s), {IMG_SIZE // 1024} KB, "
          f"{sector} setores usados")

main()
