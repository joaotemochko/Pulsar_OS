/* wm.c — cliente do WM: superficie propria, eventos e tema. */
#include "pulsar.h"
#include <string.h>

static void pack_title(const char* t, u64* out){
    /* ate 24 bytes, cortando num limite de caractere UTF-8 (nao no meio de um "—") */
    int n=0; while(n<24 && t[n]) n++;
    while(n>0 && t[n] && ((u8)t[n]&0xC0)==0x80) n--;
    for(int i=0;i<3;i++){
        u64 v=0;
        for(int b=0;b<8;b++){ int k=i*8+b; v|=((u64)(u8)(k<n?t[k]:0))<<(b*8); }
        out[i]=v;
    }
}

static WmEvent decode(const Msg* m){
    WmEvent e;
    u64 f=m->data[0], b=m->data[3], k=m->data[4];
    e.close=(f&1)!=0; e.focused=(f&2)!=0; e.dark=(f&4)!=0; e.more=(f&8)!=0; e.visible=(f&16)!=0;
    e.changed=0;
    e.w=(i32)((f>>16)&0xFFFF); e.h=(i32)((f>>32)&0xFFFF);
    e.mx=(i32)m->data[1]; e.my=(i32)m->data[2];
    e.down=(b&1)!=0; e.click=(b&2)!=0; e.rclick=(b&4)!=0; e.dbl=(b&8)!=0;
    e.wheel=(i32)(i8)(u8)(b>>8);
    e.key=(u16)k; e.mods=(u8)(k>>16); e.ch=(u32)(k>>32);
    e.menu=(u16)m->data[5]; e.accent=(u32)(m->data[5]>>32)&0xFFFFFF;
    return e;
}

/* Fala com o WM. Se ele caiu (o init o reinicia), espera o novo, registra
 * a janela de novo e repete o pedido: o app sobrevive a queda do WM. */
static void wm_call(App* a, Msg* m){
    Msg orig=*m;
    while(ipc_send(a->wm,m)<0){
        u64 old=a->wm;
        for(;;){
            i64 p=sys_lookup(SVC_WM);
            if(p>=0 && (u64)p!=old && sys_is_alive((u64)p)){ a->wm=(u64)p; break; }
            sys_sleep(50);
        }
        Msg h; memset(&h,0,sizeof h);
        h.tag=WM_OP_HELLO; h.data[0]=a->kind; h.data[1]=(u64)a->w; h.data[2]=(u64)a->h;
        pack_title(a->title,&h.data[3]);
        if(ipc_send(a->wm,&h)<0) continue;
        a->slot=h.data[0];
        a->w=(i32)h.data[1]<SURF_W?(i32)h.data[1]:SURF_W;
        a->h=(i32)h.data[2]<SURF_H?(i32)h.data[2]:SURF_H;
        a->fb.base=(u32*)sys_surf_map(a->slot);
        a->fb.w=a->w; a->fb.h=a->h; a->fb.clip=rect(0,0,a->w,a->h);
        a->redraw=1;
        *m=orig;
        if(m->tag==WM_OP_COMMIT){ m->data[0]=a->slot; m->data[1]=((u64)a->w<<16)|(u64)a->h; }
    }
}

static void apply(App* a, WmEvent* e){
    if(a->redraw){ a->redraw=0; e->changed=1; }
    if(e->w>0 && e->h>0 && (e->w!=a->w || e->h!=a->h)){
        a->w=e->w<SURF_W?e->w:SURF_W; a->h=e->h<SURF_H?e->h:SURF_H;
        a->fb.w=a->w; a->fb.h=a->h; a->fb.clip=rect(0,0,a->w,a->h);
        e->changed=1;
    }
    if(e->accent && (e->dark!=a->th.dark || e->accent!=a->th.accent)){
        a->th=theme_make(e->dark,e->accent); e->changed=1;
    }
    if(e->focused!=a->focused){ a->focused=e->focused; e->changed=1; }
}

void app_init(App* a, u64 kind, const char* title, i32 w, i32 h){
    i64 p; while((p=sys_lookup(SVC_WM))<0) sys_sleep(5);
    a->wm=(u64)p;
    Msg m; memset(&m,0,sizeof m);
    m.tag=WM_OP_HELLO; m.data[0]=kind; m.data[1]=(u64)w; m.data[2]=(u64)h;
    pack_title(title,&m.data[3]);
    ipc_send(a->wm,&m);
    a->kind=kind;
    strncpy(a->title,title,24); a->title[24]=0;
    a->redraw=0;
    a->slot=m.data[0];
    a->w=(i32)m.data[1]; a->h=(i32)m.data[2];
    a->open_file=m.data[3]?(i64)m.data[3]-1:-1;
    a->fb.base=(u32*)sys_surf_map(a->slot);
    a->fb.w=a->w; a->fb.h=a->h; a->fb.stride=SURF_W; a->fb.clip=rect(0,0,a->w,a->h);
    a->th=theme_make(0,0x0A84FF);
    a->focused=1;
    app_poll(a);
}

WmEvent app_poll(App* a){
    Msg m; memset(&m,0,sizeof m); m.tag=WM_OP_POLL;
    wm_call(a,&m);
    WmEvent e=decode(&m); apply(a,&e); return e;
}

/* como app_poll, mas nao espera novidades (para quem tem trabalho em andamento) */
WmEvent app_peek(App* a){
    Msg m; memset(&m,0,sizeof m); m.tag=WM_OP_POLL; m.data[0]=1;
    wm_call(a,&m);
    WmEvent e=decode(&m); apply(a,&e); return e;
}

WmEvent app_commit_rect(App* a, Rect d){
    Msg m; memset(&m,0,sizeof m); m.tag=WM_OP_COMMIT;
    m.data[0]=a->slot; m.data[1]=((u64)a->w<<16)|(u64)a->h;
    /* data[2]: retangulo alterado (bit 63 = presente; x 15 bits em 48..62,
     * y em 32..47, w em 16..31, h em 0..15); vazio = tudo */
    if(d.w > 0 && d.h > 0)
        m.data[2]=1ull<<63 | ((u64)(d.x&0x7FFF))<<48 | ((u64)(d.y&0xFFFF))<<32
                 | ((u64)(d.w&0xFFFF))<<16 | (u64)(d.h&0xFFFF);
    wm_call(a,&m);
    WmEvent e=decode(&m); apply(a,&e); return e;
}

WmEvent app_commit(App* a){ return app_commit_rect(a, rect(0,0,0,0)); }

void app_title(App* a, const char* t){
    Msg m; memset(&m,0,sizeof m); m.tag=WM_OP_TITLE;
    pack_title(t,&m.data[0]);
    strncpy(a->title,t,24); a->title[24]=0;
    wm_call(a,&m);
}

void app_bye(App* a){
    Msg m; memset(&m,0,sizeof m); m.tag=WM_OP_BYE;
    ipc_send(a->wm,&m);
}
