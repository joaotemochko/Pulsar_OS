#!/usr/bin/env python3
"""mknblr.py — cria uma imagem do Nebular FileSystem (NBFS v1).

Layout no disco (setor = 512 bytes):
  setor 0        : superbloco
  setores 1..=16 : tabela de inodes (128 entradas de 64 bytes = 16 setores)
  setor 17+      : dados (cada arquivo com extensao FIXA pre-alocada)

Superbloco (setor 0):
  magic       u32  "NBFS" = 0x5346424E (little-endian)
  version     u32  = 1
  file_count  u32
  table_sec   u32  = 16
  total_sec   u32  (tamanho total da imagem em setores)
  label[16]   nome do volume
  reserved    ate 512

Inode (64 bytes):
  name[28]    ASCII, zero-terminado
  start_sec   u32
  size        u32
  kind        u32  (1=exec .pulse, 2=texto, 3=dados, 4=dir)
  cap_sec     u32  (setores reservados)
  mtime       u32  (timestamp fake incremental)
  flags       u32  (bit0=oculto, bit1=somente-leitura)
  reserved    u32

Uso: mknblr.py <saida.img> <arquivo1> [arquivo2 ...]
"""
import struct, sys, os

NBFS_MAGIC = 0x5346424E
SECTOR = 512
TABLE_SEC = 16
MAX_FILES = 128
DATA_START = 1 + TABLE_SEC   # setor 17
IMG_SECTORS = 16384          # 8 MB

def kind_of(name):
    if name.endswith('.pulse'): return 1
    if name.endswith('.txt') or name.endswith('.md'): return 2
    return 3

def main():
    if len(sys.argv) < 3:
        sys.exit(__doc__)
    out = sys.argv[1]
    files = sys.argv[2:]

    img = bytearray(IMG_SECTORS * SECTOR)
    inodes = []
    cursor = DATA_START
    mtime = 1000

    for path in files:
        data = open(path, 'rb').read()
        name = os.path.basename(path)
        size = len(data)
        need = (size + SECTOR - 1) // SECTOR
        cap = need + 4                       # folga p/ crescer (editor)
        img[cursor*SECTOR : cursor*SECTOR + size] = data
        inodes.append({
            'name': name.encode()[:27],
            'start': cursor, 'size': size, 'kind': kind_of(name),
            'cap': cap, 'mtime': mtime, 'flags': 0,
        })
        cursor += cap
        mtime += 37
        if cursor > IMG_SECTORS:
            sys.exit(f"imagem cheia ao adicionar {name}")

    # superbloco
    struct.pack_into('<IIIII', img, 0, NBFS_MAGIC, 1, len(inodes), TABLE_SEC, IMG_SECTORS)
    label = b"Nebular"
    img[20:20+len(label)] = label

    # tabela de inodes
    for i, n in enumerate(inodes):
        off = SECTOR + i*64
        img[off:off+28] = n['name'].ljust(28, b'\0')
        struct.pack_into('<IIIIIII', img, off+28,
                         n['start'], n['size'], n['kind'], n['cap'],
                         n['mtime'], n['flags'], 0)

    open(out, 'wb').write(img)
    print(f"[mknblr] {out}: {len(inodes)} arquivo(s), {IMG_SECTORS*SECTOR//1024} KB, "
          f"{cursor} setores usados  [Nebular FileSystem v1]")
    for n in inodes:
        kn = {1:'exec',2:'texto',3:'dados',4:'dir'}[n['kind']]
        print(f"[mknblr]   {n['name'].decode():24} setor {n['start']:5}  {n['size']:8} B  cap={n['cap']} sec  {kn}")

if __name__ == '__main__':
    main()
