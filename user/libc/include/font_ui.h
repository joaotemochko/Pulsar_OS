#ifndef _FONT_UI_H
#define _FONT_UI_H
#include "pulsar.h"
#define FONTUI_HEIGHT 23
#define FONTUI_ASCENT 17
typedef struct { u16 w,h; i16 adv,left,top; u32 off,len; } Glyph;
extern const u8 FONTUI_PIX[];
extern const Glyph FONTUI_GLYPHS[95];
#endif