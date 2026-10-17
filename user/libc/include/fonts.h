/* fonts.h — gerado por tools/genfonts.py. NAO EDITAR. */
/* incluido por pulsar.h (depende dos tipos u8/u16/i16/u32/i32 de la). */
#ifndef _PULSAR_FONTS_H
#define _PULSAR_FONTS_H
typedef struct { u16 w,h; i16 adv,left,top; u32 off; } Glyph;
typedef struct { const Glyph* glyphs; const u8* pix; i32 ascent, height; } Font;
extern const Font FONT_UI;
extern const Font FONT_UI_BOLD;
extern const Font FONT_SMALL;
extern const Font FONT_MONO;
extern const Font FONT_TITLE;
extern const Font FONT_HUGE;
#endif
