/* text.c — texto anti-aliased UTF-8 (ASCII + Latin-1) com as fontes Noto. */
#include "pulsar.h"

u32 utf8_next(const char* s, int* i){
    u8 b=(u8)s[*i]; (*i)++;
    if(b<0x80) return b;
    int len; u32 cp;
    if((b&0xE0)==0xC0){ len=1; cp=b&0x1F; }
    else if((b&0xF0)==0xE0){ len=2; cp=b&0x0F; }
    else if((b&0xF8)==0xF0){ len=3; cp=b&0x07; }
    else return '?';
    for(int k=0;k<len;k++){
        u8 c=(u8)s[*i];
        if((c&0xC0)!=0x80) return '?';
        cp=(cp<<6)|(c&0x3F); (*i)++;
    }
    return cp;
}

static const Glyph* glyph(const Font* f, u32 cp){
    int idx;
    if(cp>=32&&cp<=126) idx=(int)cp-32;
    else if(cp>=160&&cp<=255) idx=(int)cp-160+95;
    else idx='?'-32;
    return &f->glyphs[idx];
}

i32 text_draw_n(Fb* fb, const Font* f, i32 x, i32 y, const char* s, int n, u32 c){
    i32 pen=x; int i=0;
    Rect cl=fb->clip;
    while(i<n && s[i]){
        u32 cp=utf8_next(s,&i);
        const Glyph* g=glyph(f,cp);
        i32 gx=pen+g->left, gy=y+f->ascent+g->top;
        if(g->w && gx<cl.x+cl.w && gx+g->w>cl.x && gy<cl.y+cl.h && gy+g->h>cl.y){
            const u8* d=f->pix+g->off;
            for(i32 r=0;r<g->h;r++){
                i32 py=gy+r; if(py<cl.y||py>=cl.y+cl.h) continue;
                u32* row=fb->base+(i64)py*fb->stride;
                for(i32 k=0;k<g->w;k++){
                    u32 a=d[r*g->w+k]; if(!a) continue;
                    i32 px=gx+k; if(px<cl.x||px>=cl.x+cl.w) continue;
                    row[px]=mix(row[px],c,a);
                }
            }
        }
        pen+=g->adv;
    }
    return pen-x;
}
i32 text_draw(Fb* fb, const Font* f, i32 x, i32 y, const char* s, u32 c){
    return text_draw_n(fb,f,x,y,s,0x7FFFFFFF,c);
}
i32 text_width_n(const Font* f, const char* s, int n){
    i32 w=0; int i=0;
    while(i<n && s[i]) w+=glyph(f,utf8_next(s,&i))->adv;
    return w;
}
i32 text_width(const Font* f, const char* s){ return text_width_n(f,s,0x7FFFFFFF); }
void text_center(Fb* fb, const Font* f, Rect r, const char* s, u32 c){
    text_draw(fb,f,r.x+(r.w-text_width(f,s))/2,r.y+(r.h-f->height)/2,s,c);
}
i32 text_right(Fb* fb, const Font* f, i32 right, i32 y, const char* s, u32 c){
    i32 w=text_width(f,s); text_draw(fb,f,right-w,y,s,c); return w;
}
