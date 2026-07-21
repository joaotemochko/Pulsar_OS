/* graphics.c — framebuffer e primitivas de desenho por software. */
#include "pulsar.h"

Fb fb_open(void){
    Fb fb;
    fb.base = (u32*)sys_fb_map();
    u64 info = sys_fb_info();
    fb.w = (u32)(info >> 32);
    fb.h = (u32)(info & 0xFFFFFFFF);
    return fb;
}
void fb_pixel(Fb* fb, u32 x, u32 y, u32 color){
    if(x<fb->w && y<fb->h) fb->base[y*fb->w+x]=color;
}
void fb_fill(Fb* fb, u32 x, u32 y, u32 w, u32 h, u32 color){
    u32 x1=x+w, y1=y+h;
    if(x1>fb->w) x1=fb->w;
    if(y1>fb->h) y1=fb->h;
    for(u32 yy=y; yy<y1; yy++){
        u32* row=&fb->base[yy*fb->w];
        for(u32 xx=x; xx<x1; xx++) row[xx]=color;
    }
}
void fb_blend(Fb* fb, u32 x, u32 y, u32 color, u32 a){
    if(x>=fb->w||y>=fb->h) return;
    if(a>=255){ fb->base[y*fb->w+x]=color; return; }
    if(a==0) return;
    u32 bg=fb->base[y*fb->w+x];
    u32 inv=255-a;
    u32 r=(((color>>16)&0xFF)*a+((bg>>16)&0xFF)*inv)/255;
    u32 g=(((color>>8)&0xFF)*a+((bg>>8)&0xFF)*inv)/255;
    u32 b=((color&0xFF)*a+(bg&0xFF)*inv)/255;
    fb->base[y*fb->w+x]=(r<<16)|(g<<8)|b;
}

/* ===== primitivas de UI (portadas do plib Rust, estilo Pulsar Aqua) ===== */

u32 isqrt_u(u32 n){
    if(n==0) return 0;
    u32 x=n, y=(x+1)/2;
    while(y<x){ x=y; y=(x + n/x)/2; }
    return x;
}
u32 lerp(u32 c0, u32 c1, u32 t){
    if(t>255) t=255;
    u32 it=255-t;
    u32 r=(((c0>>16)&0xFF)*it+((c1>>16)&0xFF)*t)/255;
    u32 g=(((c0>>8)&0xFF)*it+((c1>>8)&0xFF)*t)/255;
    u32 b=((c0&0xFF)*it+(c1&0xFF)*t)/255;
    return (r<<16)|(g<<8)|b;
}
void fb_vgrad(Fb* fb, Rect r, u32 top, u32 bot){
    i32 y1=r.y+r.h; if(y1>(i32)fb->h) y1=fb->h;
    i32 x1=r.x+r.w; if(x1>(i32)fb->w) x1=fb->w;
    u32 h=r.h>0?r.h:1;
    for(i32 y=r.y<0?0:r.y; y<y1; y++){
        u32 t=((y-r.y)*255)/h;
        u32 c=lerp(top,bot,t);
        u32* row=&fb->base[y*fb->w];
        for(i32 x=r.x<0?0:r.x; x<x1; x++) row[x]=c;
    }
}
void fb_vgrad_a(Fb* fb, Rect r, u32 top, u32 bot, u32 a){
    i32 y1=r.y+r.h; if(y1>(i32)fb->h) y1=fb->h;
    i32 x1=r.x+r.w; if(x1>(i32)fb->w) x1=fb->w;
    u32 h=r.h>0?r.h:1;
    for(i32 y=r.y<0?0:r.y; y<y1; y++){
        u32 t=((y-r.y)*255)/h; u32 c=lerp(top,bot,t);
        for(i32 x=r.x<0?0:r.x; x<x1; x++) fb_blend(fb,x,y,c,a);
    }
}
static u32 corner_alpha(i32 dx, i32 dy, i32 radius){
    i32 d2=dx*dx+dy*dy;
    if(d2<=(radius-1)*(radius-1)) return 255;
    if(d2>=(radius+1)*(radius+1)) return 0;
    i32 d=(i32)isqrt_u((u32)d2);
    i32 frac=radius+1-d; if(frac<0)frac=0; if(frac>2)frac=2;
    u32 v=(u32)frac*127; return v>255?255:v;
}
void fb_round_rect_grad(Fb* fb, Rect r, u32 radius, u32 top, u32 bot){
    i32 rad=radius; if(rad>r.w/2)rad=r.w/2; if(rad>r.h/2)rad=r.h/2;
    i32 x0=r.x,y0=r.y,x1=r.x+r.w,y1=r.y+r.h; u32 h=r.h>0?r.h:1;
    for(i32 y=y0; y<y1; y++){
        if(y<0||y>=(i32)fb->h) continue;
        u32 t=((u32)(y-y0)*255)/h; u32 c=lerp(top,bot,t);
        for(i32 x=x0; x<x1; x++){
            if(x<0||x>=(i32)fb->w) continue;
            i32 cx=(x<x0+rad)?x0+rad:((x>=x1-rad)?x1-rad-1:x);
            i32 cy=(y<y0+rad)?y0+rad:((y>=y1-rad)?y1-rad-1:y);
            if(cx!=x||cy!=y){ u32 a=corner_alpha(x-cx,y-cy,rad); if(a) fb_blend(fb,x,y,c,a); }
            else fb_blend(fb,x,y,c,255);
        }
    }
}
void fb_round_rect(Fb* fb, Rect r, u32 radius, u32 color){ fb_round_rect_grad(fb,r,radius,color,color); }
void fb_round_frame(Fb* fb, Rect r, u32 radius, u32 color, u32 a){
    i32 rad=radius; if(rad>r.w/2)rad=r.w/2; if(rad>r.h/2)rad=r.h/2;
    i32 x0=r.x,y0=r.y,x1=r.x+r.w,y1=r.y+r.h;
    for(i32 y=y0; y<y1; y++){
        if(y<0||y>=(i32)fb->h) continue;
        for(i32 x=x0; x<x1; x++){
            if(x<0||x>=(i32)fb->w) continue;
            i32 cx=(x<x0+rad)?x0+rad:((x>=x1-rad)?x1-rad-1:x);
            i32 cy=(y<y0+rad)?y0+rad:((y>=y1-rad)?y1-rad-1:y);
            int on_edge=(x==x0||x==x1-1||y==y0||y==y1-1);
            if(cx!=x||cy!=y){
                i32 d=(i32)isqrt_u((u32)((x-cx)*(x-cx)+(y-cy)*(y-cy)));
                if(d>=rad-1&&d<=rad) fb_blend(fb,x,y,color,a);
            } else if(on_edge) fb_blend(fb,x,y,color,a);
        }
    }
}
void fb_drop_shadow(Fb* fb, Rect r, u32 radius, u32 spread){
    u32 layers=spread>0?spread:1;
    for(u32 i=0;i<layers;i++){
        u32 off=layers-i; u32 a=6+i*4;
        Rect sr={ r.x-(i32)off, r.y-(i32)off+2, r.w+(i32)off*2, r.h+(i32)off*2 };
        fb_round_frame(fb,sr,radius+off,0x000000,a);
    }
}
void fb_disc(Fb* fb, u32 cx, u32 cy, u32 radius, u32 color){
    i32 r=radius, cxi=cx, cyi=cy;
    for(i32 dy=-r; dy<=r; dy++) for(i32 dx=-r; dx<=r; dx++){
        i32 d2=dx*dx+dy*dy;
        u32 a=(d2<=(r-1)*(r-1))?255:((d2>=(r+1)*(r+1))?0:140);
        if(a){ i32 x=cxi+dx,y=cyi+dy; if(x>=0&&y>=0) fb_blend(fb,x,y,color,a); }
    }
}
void fb_thick_line(Fb* fb, i32 x0, i32 y0, i32 x1, i32 y1, u32 color, i32 thick){
    i32 dx=x1-x0; if(dx<0)dx=-dx; i32 sx=x0<x1?1:-1;
    i32 dy=y1-y0; if(dy<0)dy=-dy; dy=-dy; i32 sy=y0<y1?1:-1;
    i32 err=dx+dy;
    for(;;){
        for(i32 t=0;t<thick;t++) if(x0>=0&&(y0+t)>=0) fb_blend(fb,x0,y0+t,color,255);
        if(x0==x1&&y0==y1) break;
        i32 e2=2*err;
        if(e2>=dy){ err+=dy; x0+=sx; }
        if(e2<=dx){ err+=dx; y0+=sy; }
    }
}
void fb_pulsar_logo(Fb* fb, u32 cx, u32 cy, u32 r, u32 tint, int glow){
    i32 cxi=cx, cyi=cy, ri=r;
    if(glow){
        for(i32 dy=-(ri+10); dy<=ri+10; dy++) for(i32 dx=-(ri+10); dx<=ri+10; dx++){
            i32 d=(i32)isqrt_u((u32)(dx*dx+dy*dy));
            if(d>ri&&d<=ri+10){ u32 a=((u32)(ri+10-d)*8); if(a>50)a=50;
                i32 x=cxi+dx,y=cyi+dy; if(x>=0&&y>=0) fb_blend(fb,x,y,tint,a); }
        }
    }
    /* disco translucido interno */
    for(i32 dy=-ri; dy<=ri; dy++) for(i32 dx=-ri; dx<=ri; dx++){
        i32 d2=dx*dx+dy*dy;
        if(d2<=(ri-3)*(ri-3)){ i32 x=cxi+dx,y=cyi+dy; if(x>=0&&y>=0) fb_blend(fb,x,y,tint,28); }
    }
    /* anel */
    for(i32 dy=-ri; dy<=ri; dy++) for(i32 dx=-ri; dx<=ri; dx++){
        i32 d=(i32)isqrt_u((u32)(dx*dx+dy*dy));
        if(d>=ri-3&&d<=ri){ u32 a=(d==ri||d==ri-3)?160:255;
            i32 x=cxi+dx,y=cyi+dy; if(x>=0&&y>=0) fb_blend(fb,x,y,tint,a); }
    }
    /* traçado de batimento (ECG) */
    i32 span=(ri*7)/10;
    i32 pts[7][2]={ {-span,0},{-span/2,0},{-span/4,-ri/6},{0,-ri/2},{span/6,ri/3},{span/2,0},{span,0} };
    for(int k=0;k<6;k++)
        fb_thick_line(fb, cxi+pts[k][0],cyi+pts[k][1], cxi+pts[k+1][0],cyi+pts[k+1][1], 0xFFFFFF, 2);
    fb_disc(fb, cxi, cyi-ri/2, 3, 0xFFFFFF);
}
