#!/usr/bin/env python3
"""mkpulse.py — converte um ELF64 AArch64 em executavel .pulse v2.

Uso: mkpulse.py <entrada.elf> <saida.pulse> <stack_top_hex>

Le os program headers PT_LOAD e gera um segmento .pulse para cada um,
preservando as flags R/W/X (o loader do kernel aplica W^X por segmento).
O entry point vem do e_entry do ELF.
"""
import struct, sys

PULSE_MAGIC = 0x534C5550  # "PULS"
PT_LOAD = 1
PF_X, PF_W, PF_R = 1, 2, 4
SEG_R, SEG_W, SEG_X = 1, 2, 4

def main():
    if len(sys.argv) != 4:
        sys.exit(__doc__)
    elf_path, out_path, stack_top = sys.argv[1], sys.argv[2], int(sys.argv[3], 16)

    data = open(elf_path, 'rb').read()
    assert data[:4] == b'\x7fELF' and data[4] == 2, "esperado ELF64"
    e_entry, e_phoff = struct.unpack_from('<QQ', data, 24)
    e_phentsize, e_phnum = struct.unpack_from('<HH', data, 54)

    segs = []
    for i in range(e_phnum):
        off = e_phoff + i * e_phentsize
        p_type, p_flags, p_offset, p_vaddr, _, p_filesz, p_memsz, _ = \
            struct.unpack_from('<IIQQQQQQ', data, off)
        if p_type != PT_LOAD or p_memsz == 0:
            continue
        assert p_vaddr % 4096 == 0, f"segmento nao alinhado a pagina: {p_vaddr:#x}"
        flags = ((p_flags & PF_R) and SEG_R) | ((p_flags & PF_W) and SEG_W) \
              | ((p_flags & PF_X) and SEG_X)
        assert not (flags & SEG_W and flags & SEG_X), "segmento W+X viola W^X"
        segs.append((p_offset, p_filesz, p_vaddr, p_memsz, flags))

    assert segs, "nenhum PT_LOAD"

    # header (32B) + tabela (24B por segmento) + blobs
    header_size = 32
    table_off = header_size
    data_off = table_off + 24 * len(segs)

    out = bytearray()
    out += struct.pack('<IHHQIIQ', PULSE_MAGIC, 2, len(segs), e_entry,
                       table_off, 0, stack_top)
    blobs = bytearray()
    for p_offset, p_filesz, p_vaddr, p_memsz, flags in segs:
        file_off = data_off + len(blobs)
        out += struct.pack('<IIQII', file_off, p_filesz, p_vaddr, p_memsz, flags)
        blobs += data[p_offset:p_offset + p_filesz]
    out += blobs

    open(out_path, 'wb').write(out)
    print(f"[mkpulse] {out_path}: entry={e_entry:#x} stack={stack_top:#x} "
          f"{len(segs)} seg(s), {len(out)} bytes")
    for i, (_, fsz, va, msz, fl) in enumerate(segs):
        perm = ('R' if fl & SEG_R else '-') + ('W' if fl & SEG_W else '-') + \
               ('X' if fl & SEG_X else '-')
        print(f"[mkpulse]   seg {i}: va={va:#x} file={fsz} mem={msz} {perm}")

main()
