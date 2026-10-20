#!/usr/bin/env python3
"""elf2pulse.py — transforma um binario Linux (ELF64 estatico, AArch64 ou
RISC-V RV64) num executavel .pulse v3 da personalidade Linux.

Uso: elf2pulse.py <entrada> <saida.pulse>

O .pulse v3 guarda os segmentos PT_LOAD do ELF (alinhados a pagina) e um
cabecalho estendido dizendo que o programa usa a ABI Linux: o kernel o
carrega como carregaria o ELF (pilha com argv/envp/auxv, numeros de syscall
do Linux). A tabela de program headers continua dentro do primeiro segmento,
e o endereco dela vai no cabecalho (AT_PHDR), porque a libc a usa para achar
o TLS.

Formato do cabecalho v3 (48 bytes):
  magic u32 "PULS" | version u16 = 3 | seg_count u16 | entry u64
  seg_table_off u32 | arch u32 | stack_top u64 = 0 (o kernel escolhe)
  phdr_va u64 | phnum u16 | abi u16 (1 = Linux) | reservado u32
Segmentos: file_off u32, file_size u32, vaddr u64, mem_size u32, flags u32
(bit0 R, bit1 W, bit2 X), como no v2.

Binarios dinamicos (com PT_INTERP) nao sao convertidos: dependem do ligador
dinamico e das bibliotecas em tempo de execucao. Rode-os direto como ELF.
"""
import struct, sys

PULSE_MAGIC = 0x534C5550
PT_LOAD, PT_INTERP, PT_PHDR = 1, 3, 6
PF_X, PF_W, PF_R = 1, 2, 4
SEG_R, SEG_W, SEG_X = 1, 2, 4
ARCH_IDS = {183: (1, "AArch64"), 243: (2, "RISC-V RV64"), 62: (3, "x86_64")}
ABI_LINUX = 1



def convert(data):
    if data[:4] != b'\x7fELF' or data[4] != 2 or data[5] != 1:
        raise ValueError("esperado ELF64 little-endian")
    e_type, e_machine = struct.unpack_from('<HH', data, 16)
    if e_machine not in ARCH_IDS:
        raise ValueError(f"arquitetura nao suportada: {e_machine}")
    if e_type != 2:
        raise ValueError("so executaveis de endereco fixo (ET_EXEC); "
                         "compile com -static (sem -static-pie)")
    arch_id, arch_name = ARCH_IDS[e_machine]
    e_entry, e_phoff = struct.unpack_from('<QQ', data, 24)
    e_phentsize, e_phnum = struct.unpack_from('<HH', data, 54)

    segs, phdr_va, first = [], 0, None
    for i in range(e_phnum):
        p_type, p_flags, p_offset, p_vaddr, _, p_filesz, p_memsz, _ = \
            struct.unpack_from('<IIQQQQQQ', data, e_phoff + i * e_phentsize)
        if p_type == PT_INTERP:
            raise ValueError("binario dinamico (PT_INTERP): rode-o como ELF")
        if p_type == PT_PHDR:
            phdr_va = p_vaddr
        if p_type != PT_LOAD or p_memsz == 0:
            continue
        if first is None:
            first = (p_vaddr, p_offset)
        # alinha a pagina: inclui os bytes antes do inicio no mesmo frame
        delta = p_vaddr & 0xFFF
        flags = (SEG_R if p_flags & PF_R else 0) | (SEG_W if p_flags & PF_W else 0) \
              | (SEG_X if p_flags & PF_X else 0)
        segs.append((p_offset - delta, p_filesz + delta, p_vaddr - delta, p_memsz + delta, flags))
    if not segs:
        raise ValueError("nenhum PT_LOAD")
    if not phdr_va and first and e_phoff >= first[1]:
        phdr_va = first[0] + (e_phoff - first[1])

    header_size = 48
    table_off = header_size
    data_off = table_off + 24 * len(segs)
    out = bytearray(struct.pack('<IHHQIIQ', PULSE_MAGIC, 3, len(segs), e_entry,
                                table_off, arch_id, 0))
    out += struct.pack('<QHHI', phdr_va, e_phnum, ABI_LINUX, 0)
    blobs = bytearray()
    for off, fsz, va, msz, fl in segs:
        out += struct.pack('<IIQII', data_off + len(blobs), fsz, va, msz, fl)
        blobs += data[off:off + fsz]
    out += blobs
    return bytes(out), arch_name, segs, e_entry


def main():
    if len(sys.argv) != 3:
        sys.exit(__doc__)
    try:
        out, arch, segs, entry = convert(open(sys.argv[1], 'rb').read())
    except ValueError as e:
        sys.exit(f"[elf2pulse] {sys.argv[1]}: {e}")
    open(sys.argv[2], 'wb').write(out)
    print(f"[elf2pulse] {sys.argv[2]} ({arch}, ABI Linux): entry={entry:#x}, "
          f"{len(segs)} seg(s), {len(out)} bytes")


if __name__ == '__main__':
    main()
