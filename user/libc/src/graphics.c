/* graphics.c — primitivas de desenho com anti-aliasing (espelho da plib). */
#include "pulsar.h"

Rect rect_and(Rect a, Rect b){
    i32 x0=a.x>b.x?a.x:b.x, y0=a.y>b.y?a.y:b.y;
    i32 x1=(a.x+a.w)<(b.x+b.w)?(a.x+a.w):(b.x+b.w);
    i32 y1=(a.y+a.h)<(b.y+b.h)?(a.y+a.h):(b.y+b.h);
    if(x1<=x0||y1<=y0) return rect(0,0,0,0);
    return rect(x0,y0,x1-x0,y1-y0);
}

u32 mix(u32 bg, u32 fg, u32 a){
    if(a>=255) return fg;
    a+=a>>7;
    u32 ia=256-a;
    u32 rb=((fg&0xFF00FF)*a+(bg&0xFF00FF)*ia)>>8;
    u32 g=((fg&0x00FF00)*a+(bg&0x00FF00)*ia)>>8;
    return (rb&0xFF00FF)|(g&0x00FF00);
}
u32 shade(u32 c, i32 t){ return t>=0?mix(c,0xFFFFFF,(u32)t):mix(c,0,(u32)(-t)); }

u32 isqrt_u(u32 n){
    u32 x=n, r=0, bit=1u<<30;
    while(bit>x) bit>>=2;
    while(bit){ if(x>=r+bit){ x-=r+bit; r=(r>>1)+bit; } else r>>=1; bit>>=2; }
    return r;
}

void fb_set_clip(Fb* fb, Rect r){ fb->clip=rect_and(r,rect(0,0,fb->w,fb->h)); }

static inline u32* row(Fb* fb, i32 y){ return fb->base+(i64)y*fb->stride; }

void fb_blend(Fb* fb, i32 x, i32 y, u32 c, u32 a){
    if(a==0||!rect_has(fb->clip,x,y)) return;
    u32* p=row(fb,y)+x; *p=mix(*p,c,a);
}
static void span(Fb* fb, i32 y, i32 x0, i32 x1, u32 c, u32 a){
    Rect cl=fb->clip;
    if(y<cl.y||y>=cl.y+cl.h) return;
    if(x0<cl.x) x0=cl.x;
    if(x1>cl.x+cl.w) x1=cl.x+cl.w;
    if(x0>=x1||a==0) return;
    u32* p=row(fb,y);
    if(a>=255){ for(i32 x=x0;x<x1;x++) p[x]=c; }
    else { for(i32 x=x0;x<x1;x++) p[x]=mix(p[x],c,a); }
}
void fb_fill_a(Fb* fb, Rect r, u32 c, u32 a){
    r=rect_and(r,fb->clip);
    for(i32 y=r.y;y<r.y+r.h;y++) span(fb,y,r.x,r.x+r.w,c,a);
}
void fb_fill(Fb* fb, Rect r, u32 c){ fb_fill_a(fb,r,c,255); }
void fb_hline(Fb* fb, i32 x, i32 y, i32 w, u32 c, u32 a){ span(fb,y,x,x+w,c,a); }
void fb_vgrad(Fb* fb, Rect r, u32 top, u32 bot){
    Rect cr=rect_and(r,fb->clip); i32 h=r.h>0?r.h:1;
    for(i32 y=cr.y;y<cr.y+cr.h;y++) span(fb,y,cr.x,cr.x+cr.w,mix(top,bot,(u32)((y-r.y)*255/h)),255);
}

/* cobertura AA (0..255) do pixel (dx,dy) num canto de raio r */
static u32 corner_cov(i32 r, i32 dx, i32 dy){
    i32 px=dx*16+8-r*16, py=dy*16+8-r*16;
    i32 d=(i32)isqrt_u((u32)(px*px+py*py));
    i32 c=r*16+8-d; if(c<0)c=0; if(c>16)c=16;
    c*=16; return (u32)(c>255?255:c);
}
static void round_impl(Fb* fb, Rect r, i32 rad, u32 top, u32 bot, u32 a){
    if(r.w<=0||r.h<=0) return;
    if(rad>r.w/2) rad=r.w/2;
    if(rad>r.h/2) rad=r.h/2;
    if(rad<0) rad=0;
    Rect cr=rect_and(r,fb->clip);
    i32 h=r.h>0?r.h:1;
    for(i32 y=cr.y;y<cr.y+cr.h;y++){
        u32 c=top==bot?top:mix(top,bot,(u32)((y-r.y)*255/h));
        i32 dy=y<r.y+rad?y-r.y:(y>=r.y+r.h-rad?r.y+r.h-1-y:-1);
        if(dy<0||rad==0){ span(fb,y,r.x,r.x+r.w,c,a); continue; }
        for(i32 dx=0;dx<rad;dx++){
            u32 cov=corner_cov(rad,dx,dy);
            if(!cov) continue;
            fb_blend(fb,r.x+dx,y,c,cov*a/255);
            fb_blend(fb,r.x+r.w-1-dx,y,c,cov*a/255);
        }
        span(fb,y,r.x+rad,r.x+r.w-rad,c,a);
    }
}
void fb_round_rect(Fb* fb, Rect r, i32 rad, u32 c){ round_impl(fb,r,rad,c,c,255); }
void fb_round_rect_a(Fb* fb, Rect r, i32 rad, u32 c, u32 a){ round_impl(fb,r,rad,c,c,a); }
void fb_round_grad(Fb* fb, Rect r, i32 rad, u32 top, u32 bot){ round_impl(fb,r,rad,top,bot,255); }

void fb_round_border(Fb* fb, Rect r, i32 rad, u32 c, u32 a){
    if(rad>r.w/2) rad=r.w/2;
    if(rad>r.h/2) rad=r.h/2;
    if(rad<1) rad=1;
    span(fb,r.y,r.x+rad,r.x+r.w-rad,c,a);
    span(fb,r.y+r.h-1,r.x+rad,r.x+r.w-rad,c,a);
    for(i32 y=r.y+rad;y<r.y+r.h-rad;y++){ fb_blend(fb,r.x,y,c,a); fb_blend(fb,r.x+r.w-1,y,c,a); }
    for(i32 dy=0;dy<rad;dy++) for(i32 dx=0;dx<rad;dx++){
        i32 o=(i32)corner_cov(rad,dx,dy);
        i32 in=(rad>1&&dx>=1&&dy>=1)?(i32)corner_cov(rad-1,dx-1,dy-1):0;
        i32 cv=o-in; if(cv<=0) continue;
        u32 aa=(u32)cv*a/255;
        fb_blend(fb,r.x+dx,r.y+dy,c,aa); fb_blend(fb,r.x+r.w-1-dx,r.y+dy,c,aa);
        fb_blend(fb,r.x+dx,r.y+r.h-1-dy,c,aa); fb_blend(fb,r.x+r.w-1-dx,r.y+r.h-1-dy,c,aa);
    }
}

void fb_shadow(Fb* fb, Rect r, i32 rad, i32 blur, i32 dy, u32 alpha){
    Rect s=rect(r.x,r.y+dy,r.w,r.h);
    Rect o=rect(s.x-blur,s.y-blur,s.w+2*blur,s.h+2*blur);
    Rect cr=rect_and(o,fb->clip);
    i32 b16=blur*16;
    i32 sx0=(s.x+rad)*16, sx1=(s.x+s.w-rad)*16, sy0=(s.y+rad)*16, sy1=(s.y+s.h-rad)*16;
    for(i32 y=cr.y;y<cr.y+cr.h;y++){
        i32 py=y*16+8, qy=py<sy0?sy0-py:(py>sy1?py-sy1:0);
        for(i32 x=cr.x;x<cr.x+cr.w;x++){
            if(rect_has(rect(r.x+rad,r.y+rad,r.w-2*rad,r.h-2*rad),x,y)){ x=r.x+r.w-rad-1; continue; }
            i32 px=x*16+8, qx=px<sx0?sx0-px:(px>sx1?px-sx1:0);
            i32 d=(qx==0?qy:(qy==0?qx:(i32)isqrt_u((u32)(qx*qx+qy*qy))))-rad*16;
            if(d>=b16) continue;
            u32 a=alpha;
            if(d>0){ u32 t=(u32)(256-d*256/b16); a=alpha*((t*t)>>8)>>8; }
            if(a){ u32* p=row(fb,y)+x; *p=mix(*p,0,a); }
        }
    }
}

void fb_disc(Fb* fb, i32 cx, i32 cy, i32 r, u32 c, u32 a){
    Rect bb=rect(cx-r-1,cy-r-1,2*r+3,2*r+3);
    Rect cr=rect_and(bb,fb->clip);
    for(i32 y=cr.y;y<cr.y+cr.h;y++){
        i32 py=y*16+8-cy*16;
        for(i32 x=cr.x;x<cr.x+cr.w;x++){
            i32 px=x*16+8-cx*16;
            i32 d=(i32)isqrt_u((u32)(px*px+py*py));
            i32 cov=r*16+8-d; if(cov<=0) continue; if(cov>16) cov=16;
            u32 al=(u32)(cov*16); if(al>255) al=255;
            fb_blend(fb,x,y,c,al*a/255);
        }
    }
}
void fb_ring(Fb* fb, i32 cx, i32 cy, i32 r, i32 t, u32 c, u32 a){
    Rect cr=rect_and(rect(cx-r-1,cy-r-1,2*r+3,2*r+3),fb->clip);
    for(i32 y=cr.y;y<cr.y+cr.h;y++){
        i32 py=y*16+8-cy*16;
        for(i32 x=cr.x;x<cr.x+cr.w;x++){
            i32 px=x*16+8-cx*16;
            i32 d=(i32)isqrt_u((u32)(px*px+py*py));
            i32 o=r*16+8-d, in=d-(r-t)*16+8;
            i32 cov=o<in?o:in; if(cov<=0) continue; if(cov>16) cov=16;
            u32 al=(u32)(cov*16); if(al>255) al=255;
            fb_blend(fb,x,y,c,al*a/255);
        }
    }
}
void fb_line(Fb* fb, i32 x0, i32 y0, i32 x1, i32 y1, i32 width, u32 c, u32 a){
    i32 X0=x0*16,Y0=y0*16,X1=x1*16,Y1=y1*16,hw=width*8;
    i32 minx=(X0<X1?X0:X1)-hw, miny=(Y0<Y1?Y0:Y1)-hw;
    i32 maxx=(X0>X1?X0:X1)+hw, maxy=(Y0>Y1?Y0:Y1)+hw;
    Rect cr=rect_and(rect(minx/16-1,miny/16-1,(maxx-minx)/16+3,(maxy-miny)/16+3),fb->clip);
    i64 dx=X1-X0, dy=Y1-Y0, len2=dx*dx+dy*dy; if(len2<1) len2=1;
    for(i32 y=cr.y;y<cr.y+cr.h;y++){
        i64 py=y*16+8;
        for(i32 x=cr.x;x<cr.x+cr.w;x++){
            i64 px=x*16+8;
            i64 t=(px-X0)*dx+(py-Y0)*dy; if(t<0)t=0; if(t>len2)t=len2;
            i64 qx=X0+dx*t/len2-px, qy=Y0+dy*t/len2-py;
            i64 dd=qx*qx+qy*qy; if(dd>0xFFFFFFFFL) continue;
            i32 d=(i32)isqrt_u((u32)dd);
            i32 cov=hw+8-d; if(cov<=0) continue; if(cov>16) cov=16;
            u32 al=(u32)(cov*16); if(al>255) al=255;
            fb_blend(fb,x,y,c,al*a/255);
        }
    }
}

Theme theme_make(int dark, u32 accent){
    Theme t;
    t.dark=dark; t.accent=accent?accent:0x0A84FF;
    if(dark){
        t.win_bg=0x1E1F24; t.side_bg=0x26272D; t.text=0xF2F2F7; t.text2=0xA1A1A8; t.text3=0x6C6C73;
        t.sep=0x3A3A40; t.ctrl_bg=0x3A3B41; t.ctrl_border=0x4A4B52; t.field_bg=0x2A2B30; t.hover=0x3A3B42;
    } else {
        t.win_bg=0xFFFFFF; t.side_bg=0xF2F2F6; t.text=0x1D1D1F; t.text2=0x6E6E73; t.text3=0xA0A0A6;
        t.sep=0xE2E2E7; t.ctrl_bg=0xFFFFFF; t.ctrl_border=0xD2D2D7; t.field_bg=0xFFFFFF; t.hover=0xE8E8ED;
    }
    return t;
}

void ui_button(Fb* fb, Rect r, const char* label, const Theme* th, int primary, int hover){
    if(primary){
        fb_round_grad(fb,r,7,shade(th->accent,hover?40:20),th->accent);
        text_center(fb,&FONT_UI_BOLD,r,label,0xFFFFFF);
    } else {
        fb_shadow(fb,r,7,3,1,25);
        fb_round_rect(fb,r,7,hover?th->hover:th->ctrl_bg);
        fb_round_border(fb,r,7,th->ctrl_border,255);
        text_center(fb,&FONT_UI,r,label,th->text);
    }
}
