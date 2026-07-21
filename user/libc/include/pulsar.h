/* pulsar.h — libc mínima do Pulsar OS (freestanding, AArch64).
 * Só o necessário para apps de GUI: syscalls, tipos, e utilidades.
 */
#ifndef _PULSAR_H
#define _PULSAR_H

typedef unsigned char      u8;
typedef unsigned short     u16;
typedef unsigned int       u32;
typedef unsigned long      u64;
typedef signed   char      i8;
typedef short              i16;
typedef int                i32;
typedef long               i64;
typedef unsigned long      size_t;
typedef long               ssize_t;

#define NULL ((void*)0)

/* ---- syscall bruto (svc #0): num em x8, args x0..x2, ret x0 ---- */
static inline u64 __syscall(u64 num, u64 a0, u64 a1, u64 a2) {
    register u64 x8 __asm__("x8") = num;
    register u64 x0 __asm__("x0") = a0;
    register u64 x1 __asm__("x1") = a1;
    register u64 x2 __asm__("x2") = a2;
    __asm__ volatile("svc #0"
        : "+r"(x0)
        : "r"(x8), "r"(x1), "r"(x2)
        : "memory");
    return x0;
}

/* ---- números de syscall (espelham o kernel do Pulsar) ---- */
enum {
    SYS_WRITE=1, SYS_YIELD=2, SYS_EXIT=3, SYS_FB_INFO=4, SYS_FB_MAP=5,
    SYS_FB_PRESENT=6, SYS_INPUT_POLL=7, SYS_FS_COUNT=8, SYS_FS_STAT=9,
    SYS_SPAWN=10, SYS_IPC_SEND=11, SYS_IPC_RECV=12, SYS_IPC_REPLY=13,
    SYS_GETPID=14, SYS_FS_READ=15, SYS_FS_WRITE=16, SYS_FS_CREATE=17,
    SYS_SET_FOCUS=18, SYS_REGISTER=19, SYS_LOOKUP=20, SYS_SURF_MAP=21,
    SYS_SURF_INFO=22, SYS_IPC_TRY_RECV=23, SYS_IS_ALIVE=24, SYS_UPTIME=25,
    SYS_NET_STATUS=26, SYS_NET_UDP_SEND=27, SYS_HTTP_GET=28,
};

/* ---- wrappers de alto nível ---- */
static inline void  sys_write(const char* s, u64 n){ __syscall(SYS_WRITE,(u64)s,n,0); }
static inline void  sys_yield(void){ __syscall(SYS_YIELD,0,0,0); }
static inline void  sys_exit(void){ __syscall(SYS_EXIT,0,0,0); for(;;){} }
static inline u64   sys_fb_map(void){ return __syscall(SYS_FB_MAP,0,0,0); }
static inline u64   sys_fb_info(void){ return __syscall(SYS_FB_INFO,0,0,0); }
static inline void  sys_fb_present(void){ __syscall(SYS_FB_PRESENT,0,0,0); }
static inline u64   sys_input_poll(void){ return __syscall(SYS_INPUT_POLL,0,0,0); }
static inline i64   sys_getpid(void){ return (i64)__syscall(SYS_GETPID,0,0,0); }
/* rede: status retorna IP empacotado (a<<24|b<<16|c<<8|d), 0 se sem rede */
static inline u64   sys_net_status(void){ return __syscall(SYS_NET_STATUS,0,0,0); }
/* envia UDP: ip=(a<<24|b<<16|c<<8|d), dport/sport, buffer+len */
static inline int   sys_net_udp_send(u32 ip, u16 dport, u16 sport, const void* buf, u32 len){
    u64 ports = ((u64)dport<<16)|(u64)sport;
    u64 bl = ((u64)(u64)buf<<32)|(u64)len;
    return (int)__syscall(SYS_NET_UDP_SEND, ip, ports, bl);
}
/* HTTP GET. buf deve ter >= 512+15000 bytes. Layout de entrada em buf:
 *   [0..2]=host_len(be), [2..4]=path_len(be), host, path.
 * Saida: corpo bruto (headers+html) escrito em buf+512. Retorna bytes. */
static inline int   sys_http_get(u32 ip, u16 port, void* buf){
    return (int)__syscall(SYS_HTTP_GET, ip, port, (u64)buf);
}
static inline i64   sys_spawn(const char* n, u64 len){ return (i64)__syscall(SYS_SPAWN,(u64)n,len,0); }
static inline void  sys_register(u64 svc){ __syscall(SYS_REGISTER,svc,0,0); }
static inline i64   sys_lookup(u64 svc){ return (i64)__syscall(SYS_LOOKUP,svc,0,0); }
static inline u64   sys_surf_map(u64 slot){ return __syscall(SYS_SURF_MAP,slot,0,0); }
static inline u64   sys_uptime(void){ return __syscall(SYS_UPTIME,0,0,0); }
static inline void  sys_set_focus(u64 pid){ __syscall(SYS_SET_FOCUS,pid,0,0); }
static inline i64   sys_is_alive(u64 pid){ return (i64)__syscall(SYS_IS_ALIVE,pid,0,0); }

/* IPC: mensagem = tag (u64) + 6 palavras */
typedef struct { u64 tag; u64 data[6]; } Msg;
static inline i64  ipc_send(u64 dst, Msg* m){ return (i64)__syscall(SYS_IPC_SEND,dst,(u64)m,0); }
static inline u64  ipc_recv(Msg* m){ return __syscall(SYS_IPC_RECV,(u64)m,0,0); }
static inline void ipc_reply(u64 to, Msg* m){ __syscall(SYS_IPC_REPLY,to,(u64)m,0); }
static inline i64  ipc_try_recv(Msg* m){ return (i64)__syscall(SYS_IPC_TRY_RECV,(u64)m,0,0); }

/* ---- utilidades de string/memória (implementadas em string.c) ---- */
void* memcpy(void* d, const void* s, size_t n);
void* memset(void* d, int c, size_t n);
size_t strlen(const char* s);
void puts_(const char* s);  /* escreve string na serial */

/* ---- gráficos: framebuffer simples ---- */
typedef struct { u32* base; u32 w; u32 h; } Fb;
Fb   fb_open(void);
void fb_fill(Fb* fb, u32 x, u32 y, u32 w, u32 h, u32 color);
void fb_pixel(Fb* fb, u32 x, u32 y, u32 color);
void fb_blend(Fb* fb, u32 x, u32 y, u32 color, u32 alpha);


/* ---- geometria e primitivas de UI (estilo Pulsar Aqua) ---- */
typedef struct { i32 x, y, w, h; } Rect;
static inline int rect_has(Rect r, i32 px, i32 py){
    return px>=r.x && px<r.x+r.w && py>=r.y && py<r.y+r.h;
}

u32  isqrt_u(u32 n);
u32  lerp(u32 c0, u32 c1, u32 t);
void fb_vgrad(Fb* fb, Rect r, u32 top, u32 bot);
void fb_vgrad_a(Fb* fb, Rect r, u32 top, u32 bot, u32 a);
void fb_round_rect(Fb* fb, Rect r, u32 radius, u32 color);
void fb_round_rect_grad(Fb* fb, Rect r, u32 radius, u32 top, u32 bot);
void fb_round_frame(Fb* fb, Rect r, u32 radius, u32 color, u32 a);
void fb_drop_shadow(Fb* fb, Rect r, u32 radius, u32 spread);
void fb_disc(Fb* fb, u32 cx, u32 cy, u32 radius, u32 color);
void fb_thick_line(Fb* fb, i32 x0, i32 y0, i32 x1, i32 y1, u32 color, i32 thick);
void fb_pulsar_logo(Fb* fb, u32 cx, u32 cy, u32 r, u32 tint, int glow);

/* ---- texto anti-aliased (fonte Poppins, gerada em font_ui.c) ---- */
i32  text_ui(Fb* fb, i32 x, i32 y, const char* s, u32 color);
i32  text_ui_a(Fb* fb, i32 x, i32 y, const char* s, u32 color, u32 alpha);
i32  text_ui_width(const char* s);

/* ---- superfícies por cliente + protocolo WM ---- */
#define SURF_W 512
#define SURF_H 512
#define SVC_FS 1
#define SVC_WM 2
enum { FS_OP_COUNT=1, FS_OP_STAT=2, FS_OP_READ=3, FS_OP_WRITE=4 };
enum { WM_OP_HELLO=1, WM_OP_COMMIT=2, WM_OP_POLL=3 };
enum { WM_FLAG_CLOSE=1, WM_FLAG_FOCUS=2, WM_MOUSE_DOWN=1, WM_MOUSE_CLICK=2 };

typedef struct { int close, focused; i32 mx, my; int click; u16 key; u16 menu; } WmEvent;
/* acoes de menu enviadas pelo WM ao app em foco (WmEvent.menu) */
#define APP_MENU_SAVE  1
#define APP_MENU_NEW   2
#define APP_MENU_COPY  3
#define APP_MENU_PASTE 4
#define APP_MENU_UNDO  5
#define APP_MENU_CLEAR 6
#define APP_MENU_PAUSE 7
#define APP_MENU_RESET 8
Fb      surface_open(u64 slot);
/* HELLO: registra no WM; preenche *slot,*cw,*ch. Retorna 0 ok. */
void    wm_hello(u64 wm, u64 kind, u64* slot, u32* cw, u32* ch);
void    wm_commit(u64 wm, u64 slot);
WmEvent wm_poll(u64 wm);

/* input event decodificado (bit63=valido) */
typedef struct { int valid; u16 type; u16 code; i32 value; } InputEv;
InputEv input_poll_ev(void);
#define EV_KEY 1
#define EV_ABS 3
#define ABS_X 0
#define ABS_Y 1
#define BTN_LEFT 0x110

#endif
