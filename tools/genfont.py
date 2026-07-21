#!/usr/bin/env python3
"""genfont.py — atlas de fonte AA a partir de uma TTF, com metricas exatas.

Cada glifo e renderizado num canvas JUSTO (sem padding), e as metricas
sao emitidas de forma que o renderizador posicione:
    gx = pen + left
    gy = y + ascent + top      (top = -(altura acima da baseline))
onde (x,y) e o canto sup-esq da caixa de texto e ascent vem da fonte.

Uso: genfont.py <fonte.ttf> <px> <saida.rs> <NOME>
"""
import sys
from PIL import Image, ImageFont, ImageDraw

ttf, px, out, name = sys.argv[1], int(sys.argv[2]), sys.argv[3], sys.argv[4]
font = ImageFont.truetype(ttf, px)
asc, desc = font.getmetrics()
height = asc + desc

glyphs = []
for code in range(32, 127):
    ch = chr(code)
    bbox = font.getbbox(ch)            # (x0,y0,x1,y1) relativo a origem (baseline em y=asc)
    x0, y0, x1, y1 = bbox
    gw = max(1, x1 - x0)
    gh = max(1, y1 - y0)
    adv = int(round(font.getlength(ch)))
    # canvas JUSTO, sem padding. Desenhamos deslocando pela origem do bbox.
    img = Image.new("L", (gw, gh), 0)
    d = ImageDraw.Draw(img)
    d.text((-x0, -y0), ch, font=font, fill=255)
    data = list(img.getdata())
    # left = x0 (deslocamento horizontal do glifo)
    # top  = y0 - asc  -> distancia (negativa) do topo do glifo a baseline,
    #        de modo que gy = y + asc + top = y + y0 (topo do glifo na caixa)
    glyphs.append({
        'code': code, 'w': gw, 'h': gh, 'adv': adv,
        'left': x0, 'top': y0 - asc,
        'data': data,
    })

lines = []
lines.append(f"// Fonte {name} (AA, {px}px) — metricas exatas, canvas justo")
lines.append(f"pub const {name}_HEIGHT: u32 = {height};")
lines.append(f"pub const {name}_ASCENT: i32 = {asc};")
lines.append("")
if name == "FONTUI":
    lines.append("#[derive(Clone, Copy)]")
    lines.append("pub struct Glyph {")
    lines.append("    pub w: u16, pub h: u16, pub adv: i16, pub left: i16, pub top: i16,")
    lines.append("    pub off: usize, pub len: usize,")
    lines.append("}")
    lines.append("")

blob = bytearray()
metas = []
for g in glyphs:
    off = len(blob)
    blob.extend(g['data'])
    metas.append((g['code'], g['w'], g['h'], g['adv'], g['left'], g['top'], off, len(g['data'])))

lines.append(f"pub static {name}_PIXELS: [u8; {len(blob)}] = [")
row = []
for b in blob:
    row.append(str(b))
    if len(row) == 32:
        lines.append("    " + ",".join(row) + ",")
        row = []
if row:
    lines.append("    " + ",".join(row) + ",")
lines.append("];")
lines.append("")

gtype = "Glyph" if name == "FONTUI" else "super::Glyph"
lines.append(f"pub static {name}_GLYPHS: [{gtype}; {len(metas)}] = [")
for (code, w, h, adv, left, top, off, ln) in metas:
    lines.append(f"    {gtype} {{ w:{w}, h:{h}, adv:{adv}, left:{left}, top:{top}, off:{off}, len:{ln} }}, // {chr(code)!r}")
lines.append("];")
lines.append("")

open(out, "w").write("\n".join(lines))
print(f"[genfont] {name}: {len(glyphs)} glifos, height={height} asc={asc}, {len(blob)} bytes -> {out}")
