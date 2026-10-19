#!/usr/bin/env python3
"""png2icn.py <png> <saida.icn>

Converte o icone PNG de um aplicativo (RGBA ou RGB de 8 bits, sem
entrelacamento: o formato dos icones de aplicativos Linux) para o formato que
o shell do Pulsar desenha no dock e nos menus:

    "PICN"  u32 largura  u32 altura  (largura*altura) u32 ARGB (alfa direto)

tudo little-endian. So a biblioteca padrao do Python (zlib).
"""
import struct
import sys
import zlib


def decode_png(data):
    if data[:8] != b"\x89PNG\r\n\x1a\n":
        sys.exit("nao e PNG")
    pos, idat, hdr = 8, b"", None
    while pos < len(data):
        n, kind = struct.unpack(">I4s", data[pos:pos + 8])
        body = data[pos + 8:pos + 8 + n]
        pos += 12 + n
        if kind == b"IHDR":
            hdr = struct.unpack(">IIBBBBB", body)
        elif kind == b"IDAT":
            idat += body
        elif kind == b"IEND":
            break
    w, h, depth, ctype, _, _, interlace = hdr
    if depth != 8 or ctype not in (2, 6) or interlace:
        sys.exit("so PNG RGB/RGBA de 8 bits sem entrelacamento")
    bpp = 4 if ctype == 6 else 3
    raw = zlib.decompress(idat)
    stride = w * bpp
    out, prev, p = [], bytearray(stride), 0
    for _ in range(h):
        f, line = raw[p], bytearray(raw[p + 1:p + 1 + stride])
        p += 1 + stride
        for i in range(stride):
            a = line[i - bpp] if i >= bpp else 0
            b = prev[i]
            c = prev[i - bpp] if i >= bpp else 0
            if f == 1:
                line[i] = (line[i] + a) & 255
            elif f == 2:
                line[i] = (line[i] + b) & 255
            elif f == 3:
                line[i] = (line[i] + (a + b) // 2) & 255
            elif f == 4:
                pa, pb, pc = abs(b - c), abs(a - c), abs(a + b - 2 * c)
                pred = a if pa <= pb and pa <= pc else (b if pb <= pc else c)
                line[i] = (line[i] + pred) & 255
        for x in range(w):
            r, g, bl = line[x * bpp:x * bpp + 3]
            al = line[x * bpp + 3] if bpp == 4 else 255
            out.append(al << 24 | r << 16 | g << 8 | bl)
        prev = line
    return w, h, out


def main():
    if len(sys.argv) != 3:
        sys.exit(__doc__)
    w, h, px = decode_png(open(sys.argv[1], "rb").read())
    with open(sys.argv[2], "wb") as f:
        f.write(b"PICN" + struct.pack("<II", w, h) + struct.pack(f"<{len(px)}I", *px))


if __name__ == "__main__":
    main()
