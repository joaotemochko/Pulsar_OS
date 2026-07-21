#!/usr/bin/env python3
"""genfont_c.py — gera atlas de fonte AA em C a partir de uma TTF.
Uso: genfont_c.py <fonte.ttf> <px> <saida.c> <saida.h>
"""
import sys
from PIL import Image, ImageFont, ImageDraw
ttf,px,outc,outh = sys.argv[1],int(sys.argv[2]),sys.argv[3],sys.argv[4]
font=ImageFont.truetype(ttf,px)
asc,desc=font.getmetrics(); height=asc+desc
glyphs=[]
for code in range(32,127):
    ch=chr(code); x0,y0,x1,y1=font.getbbox(ch)
    gw=max(1,x1-x0); gh=max(1,y1-y0); adv=int(round(font.getlength(ch)))
    img=Image.new("L",(gw,gh),0); d=ImageDraw.Draw(img); d.text((-x0,-y0),ch,font=font,fill=255)
    glyphs.append({'code':code,'w':gw,'h':gh,'adv':adv,'left':x0,'top':y0-asc,'data':list(img.getdata())})
blob=bytearray(); metas=[]
for g in glyphs:
    off=len(blob); blob.extend(g['data'])
    metas.append((g['w'],g['h'],g['adv'],g['left'],g['top'],off,len(g['data'])))
# header
h=[]
h.append("#ifndef _FONT_UI_H\n#define _FONT_UI_H")
h.append('#include "pulsar.h"')
h.append(f"#define FONTUI_HEIGHT {height}")
h.append(f"#define FONTUI_ASCENT {asc}")
h.append("typedef struct { u16 w,h; i16 adv,left,top; u32 off,len; } Glyph;")
h.append("extern const u8 FONTUI_PIX[];")
h.append("extern const Glyph FONTUI_GLYPHS[95];")
h.append("#endif")
open(outh,"w").write("\n".join(h))
# c
c=[]
c.append(f'#include "{outh.split("/")[-1]}"')
c.append(f"const u8 FONTUI_PIX[{len(blob)}] = {{")
row=[]
for b in blob:
    row.append(str(b))
    if len(row)==32: c.append("  "+",".join(row)+","); row=[]
if row: c.append("  "+",".join(row)+",")
c.append("};")
c.append("const Glyph FONTUI_GLYPHS[95] = {")
for (w,hh,adv,left,top,off,ln) in metas:
    c.append(f"  {{{w},{hh},{adv},{left},{top},{off},{ln}}},")
c.append("};")
open(outc,"w").write("\n".join(c))
print(f"[genfont_c] {len(glyphs)} glifos, height={height} asc={asc}, {len(blob)} bytes")
