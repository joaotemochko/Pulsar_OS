/* text.c — renderizador de texto anti-aliased (fonte Poppins). */
#include "pulsar.h"
#include "font_ui.h"

static i32 draw_glyphs(Fb* fb, i32 x, i32 y, const char* s, u32 color, u32 alpha){
    i32 pen=x;
    for(const u8* p=(const u8*)s; *p; p++){
        u8 c=*p;
        if(c<32||c>126){ pen+=6; continue; }
        const Glyph* g=&FONTUI_GLYPHS[c-32];
        i32 gx=pen+g->left;
        i32 gy=y+FONTUI_ASCENT+g->top;
        const u8* data=&FONTUI_PIX[g->off];
        for(i32 row=0; row<g->h; row++){
            for(i32 col=0; col<g->w; col++){
                u32 a=data[row*g->w+col];
                if(a){
                    i32 pxx=gx+col, pyy=gy+row;
                    if(pxx>=0&&pyy>=0) fb_blend(fb,pxx,pyy,color,a*alpha/255);
                }
            }
        }
        pen+=g->adv;
    }
    return pen-x;
}
i32 text_ui(Fb* fb, i32 x, i32 y, const char* s, u32 color){ return draw_glyphs(fb,x,y,s,color,255); }
i32 text_ui_a(Fb* fb, i32 x, i32 y, const char* s, u32 color, u32 alpha){ return draw_glyphs(fb,x,y,s,color,alpha); }
i32 text_ui_width(const char* s){
    i32 w=0;
    for(const u8* p=(const u8*)s; *p; p++){
        u8 c=*p;
        if(c<32||c>126){ w+=6; continue; }
        w+=FONTUI_GLYPHS[c-32].adv;
    }
    return w;
}
