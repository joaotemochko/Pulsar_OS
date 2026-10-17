#!/usr/bin/env python3
"""genfonts.py — gera os atlas de fonte anti-aliased do Pulsar (Rust e C).

Cada fonte cobre ASCII (32..126) + Latin-1 (160..255), o suficiente para
portugues ("Configurações", "Área de Trabalho"). Os glifos sao bitmaps de
alpha (8 bits) em canvas justo, com metricas para posicionar:
    gx = pen + left
    gy = y + ascent + top        (y = topo da caixa de texto)

Uso: genfonts.py <saida_rust.rs> <dir_c_src> <dir_c_include>
Requer Pillow e as fontes Noto (fonts-noto-core) instaladas.
"""
import sys, os
from PIL import Image, ImageFont, ImageDraw

NOTO = "/usr/share/fonts/truetype/noto"
FONTS = [
    # nome,   arquivo,                   px
    ("UI",      "NotoSans-Regular.ttf",     15),
    ("UI_BOLD", "NotoSans-Bold.ttf",        15),
    ("SMALL",   "NotoSans-Regular.ttf",     12),
    ("MONO",    "NotoSansMono-Regular.ttf", 14),
    ("TITLE",   "NotoSans-Bold.ttf",        22),
    ("HUGE",    "NotoSans-Regular.ttf",     40),
]
CODES = list(range(32, 127)) + list(range(160, 256))

def render(path, px):
    font = ImageFont.truetype(path, px)
    asc, desc = font.getmetrics()
    glyphs, pixels = [], bytearray()
    for code in CODES:
        ch = chr(code)
        x0, y0, x1, y1 = font.getbbox(ch)
        gw, gh = max(1, x1 - x0), max(1, y1 - y0)
        adv = int(round(font.getlength(ch)))
        if code in (32, 160):
            gw = gh = 0
            data = b""
        else:
            img = Image.new("L", (gw, gh), 0)
            ImageDraw.Draw(img).text((-x0, -y0), ch, font=font, fill=255)
            data = bytes(img.getdata())
        glyphs.append((gw, gh, adv, x0, y0 - asc, len(pixels), len(data)))
        pixels += data
    return asc, asc + desc, glyphs, pixels

def rust_bytes(b):
    out = []
    for i in range(0, len(b), 32):
        out.append("    " + ",".join(str(v) for v in b[i:i+32]) + ",")
    return "\n".join(out)

def main():
    out_rs, c_src, c_inc = sys.argv[1], sys.argv[2], sys.argv[3]
    rs = ["//! Fontes geradas por tools/genfonts.py — NAO EDITAR.",
          "//! Noto Sans / Noto Sans Mono (SIL Open Font License 1.1).",
          "use crate::text::{Font, Glyph};", ""]
    h = ["/* fonts.h — gerado por tools/genfonts.py. NAO EDITAR. */",
         "/* incluido por pulsar.h (depende dos tipos u8/u16/i16/u32/i32 de la). */",
         "#ifndef _PULSAR_FONTS_H", "#define _PULSAR_FONTS_H",
         "typedef struct { u16 w,h; i16 adv,left,top; u32 off; } Glyph;",
         "typedef struct { const Glyph* glyphs; const u8* pix; i32 ascent, height; } Font;"]
    total = 0
    for name, file, px in FONTS:
        asc, height, glyphs, pixels = render(os.path.join(NOTO, file), px)
        total += len(pixels)
        lname = name.lower()
        rs.append(f"static {name}_PIX: [u8; {len(pixels)}] = [\n{rust_bytes(pixels)}\n];")
        rs.append(f"static {name}_GLYPHS: [Glyph; {len(glyphs)}] = [")
        for (w, gh, adv, left, top, off, _ln) in glyphs:
            rs.append(f"    Glyph {{ w: {w}, h: {gh}, adv: {adv}, left: {left}, top: {top}, off: {off} }},")
        rs.append("];")
        rs.append(f"pub static FONT_{name}: Font = Font {{ glyphs: &{name}_GLYPHS, pix: &{name}_PIX, ascent: {asc}, height: {height} }};\n")
        # C: um arquivo por fonte (o linker so puxa as usadas)
        c = [f"/* {file} {px}px — gerado por tools/genfonts.py */", '#include "pulsar.h"',
             f"static const u8 PIX[{len(pixels)}] = {{"]
        for i in range(0, len(pixels), 32):
            c.append("  " + ",".join(str(v) for v in pixels[i:i+32]) + ",")
        c.append("};")
        c.append(f"static const Glyph GLY[{len(glyphs)}] = {{")
        for (w, gh, adv, left, top, off, _ln) in glyphs:
            c.append(f"  {{{w},{gh},{adv},{left},{top},{off}}},")
        c.append("};")
        c.append(f"const Font FONT_{name} = {{ GLY, PIX, {asc}, {height} }};")
        open(os.path.join(c_src, f"font_{lname}.c"), "w").write("\n".join(c) + "\n")
        h.append(f"extern const Font FONT_{name};")
    h.append("#endif")
    open(out_rs, "w").write("\n".join(rs))
    open(os.path.join(c_inc, "fonts.h"), "w").write("\n".join(h) + "\n")
    print(f"[genfonts] {len(FONTS)} fontes, {total} bytes de pixels")

main()
