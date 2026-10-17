/* pulsar.h — libc minima do Pulsar OS (freestanding; AArch64 e RISC-V).
 * Syscalls, utilidades, graficos com AA, texto UTF-8 e o protocolo do WM.
 * Espelha a plib (Rust) para que apps em C tenham a mesma aparencia. */
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

/* ---- syscall bruto: AArch64 svc #0 (num em x8) / RISC-V ecall (num em a7);
 *      argumentos em x0..x2 / a0..a2, retorno em x0 / a0 ---- */
static inline u64 __syscall(u64 num, u64 a0, u64 a1, u64 a2) {
#if defined(__riscv)
    register u64 r7 __asm__("a7") = num;
    register u64 r0 __asm__("a0") = a0;
    register u64 r1 __asm__("a1") = a1;
    register u64 r2 __asm__("a2") = a2;
    __asm__ volatile("ecall" : "+r"(r0) : "r"(r7), "r"(r1), "r"(r2) : "memory");
    return r0;
#else
    register u64 x8 __asm__("x8") = num;
    register u64 x0 __asm__("x0") = a0;
    register u64 x1 __asm__("x1") = a1;
    register u64 x2 __asm__("x2") = a2;
    __asm__ volatile("svc #0" : "+r"(x0) : "r"(x8), "r"(x1), "r"(x2) : "memory");
    return x0;
#endif
}

/* ---- numeros de syscall (espelham o kernel) ---- */
enum {
    SYS_WRITE=1, SYS_YIELD=2, SYS_EXIT=3, SYS_FB_INFO=4, SYS_FB_MAP=5,
    SYS_FB_PRESENT=6, SYS_INPUT_POLL=7, SYS_FS_COUNT=8, SYS_FS_STAT=9,
    SYS_SPAWN=10, SYS_IPC_SEND=11, SYS_IPC_RECV=12, SYS_IPC_REPLY=13,
    SYS_GETPID=14, SYS_FS_READ=15, SYS_FS_WRITE=16, SYS_FS_CREATE=17,
    SYS_SET_FOCUS=18, SYS_REGISTER=19, SYS_LOOKUP=20, SYS_SURF_MAP=21,
    SYS_SURF_INFO=22, SYS_IPC_TRY_RECV=23, SYS_IS_ALIVE=24, SYS_UPTIME=25,
    SYS_NET_STATUS=26, SYS_NET_UDP_SEND=27, SYS_HTTP_GET=28, SYS_SLEEP=29,
    SYS_TIME_MS=30, SYS_RTC=31, SYS_SYSINFO=32, SYS_PROC_INFO=33, SYS_KILL=34,
    SYS_CPU_LOAD=35, SYS_POWEROFF=36, SYS_TCP_CONNECT=41, SYS_TCP_SEND=42, SYS_TCP_RECV=43,
    SYS_TCP_CLOSE=44, SYS_TCP_STATUS=45, SYS_UDP_RECV=46, SYS_SBRK=47, SYS_GETRANDOM=48,
    SYS_THREAD_CREATE=49, SYS_FUTEX_WAIT=50, SYS_FUTEX_WAKE=51, SYS_EXIT_GROUP=52,
};

static inline void  sys_write(const char* s, u64 n){ __syscall(SYS_WRITE,(u64)s,n,0); }
static inline void  sys_yield(void){ __syscall(SYS_YIELD,0,0,0); }
static inline __attribute__((noreturn)) void sys_exit(void){ __syscall(SYS_EXIT,0,0,0); for(;;){} }
static inline i64   sys_getpid(void){ return (i64)__syscall(SYS_GETPID,0,0,0); }
static inline u64   sys_net_status(void){ return __syscall(SYS_NET_STATUS,0,0,0); }
static inline int   sys_net_udp_send(u32 ip, u16 dport, u16 sport, const void* buf, u32 len){
    u64 ports = ((u64)dport<<16)|(u64)sport;
    u64 bl = ((u64)buf<<32)|(u64)len;
    return (int)__syscall(SYS_NET_UDP_SEND, ip, ports, bl);
}
/* ---- sockets (o kernel faz so o transporte; HTTP/DNS ficam no app) ---- */
/* abre conexao TCP (nao bloqueia): handle ou -1 */
static inline i64   sys_tcp_connect(u32 ip, u16 port){ return (i64)__syscall(SYS_TCP_CONNECT,ip,port,0); }
/* bytes aceitos (0 = tente de novo), -1 erro */
static inline i64   sys_tcp_send(i64 h, const void* p, u64 n){ return (i64)__syscall(SYS_TCP_SEND,(u64)h,(u64)p,n); }
/* bytes lidos, 0 = fim, -2 = nada ainda, -1 = erro */
static inline i64   sys_tcp_recv(i64 h, void* p, u64 n){ return (i64)__syscall(SYS_TCP_RECV,(u64)h,(u64)p,n); }
static inline void  sys_tcp_close(i64 h){ __syscall(SYS_TCP_CLOSE,(u64)h,0,0); }
/* 0 conectando, 1 aberta, 2 fechada pelo par, -1 erro */
static inline i64   sys_tcp_status(i64 h){ return (i64)__syscall(SYS_TCP_STATUS,(u64)h,0,0); }
/* datagrama para `port`: buf = [ip:4][porta:2][0:2] + dados; total ou -2 */
static inline i64   sys_udp_recv(u16 port, void* buf, u64 max){ return (i64)__syscall(SYS_UDP_RECV,port,(u64)buf,max); }
static inline i64   sys_spawn(const char* n, u64 len){ return (i64)__syscall(SYS_SPAWN,(u64)n,len,0); }
static inline i64   sys_lookup(u64 svc){ return (i64)__syscall(SYS_LOOKUP,svc,0,0); }
static inline u64   sys_surf_map(u64 slot){ return __syscall(SYS_SURF_MAP,slot,0,0); }
static inline i64   sys_is_alive(u64 pid){ return (i64)__syscall(SYS_IS_ALIVE,pid,0,0); }
static inline void  sys_sleep(u64 ms){ __syscall(SYS_SLEEP,ms,0,0); }
static inline u64   sys_time_ms(void){ return __syscall(SYS_TIME_MS,0,0,0); }
static inline u64   sys_rtc(void){ return __syscall(SYS_RTC,0,0,0); }
static inline i64   sys_kill(u64 pid){ return (i64)__syscall(SYS_KILL,pid,0,0); }
static inline i64   sys_cpu_load(u64 cpu){ return (i64)__syscall(SYS_CPU_LOAD,cpu,0,0); }
static inline u64   sys_fs_count(void){ return __syscall(SYS_FS_COUNT,0,0,0); }
static inline i64   sys_fs_stat(u64 idx, void* buf44){ return (i64)__syscall(SYS_FS_STAT,idx,(u64)buf44,0); }
static inline i64   sys_fs_read(u64 idx, void* dst, u64 max){ return (i64)__syscall(SYS_FS_READ,idx,(u64)dst,max); }
static inline void  sys_poweroff(void){ __syscall(SYS_POWEROFF,0,0,0); }

typedef struct {
    u32 arch_id, ncpus, mem_total_kb, mem_free_kb, fb_w, fb_h;
    u64 uptime_ms;
    char arch_name[16];
} SysInfo;
static inline void  sys_sysinfo(SysInfo* s){ __syscall(SYS_SYSINFO,(u64)s,0,0); }

typedef struct { u64 pid; u32 state, cpu; u64 cpu_ms; char name[24]; } ProcInfo;
/* 0 ok, -1 slot vazio, -2 fim da tabela */
static inline i64   sys_proc_info(u64 slot, ProcInfo* p){ return (i64)__syscall(SYS_PROC_INFO,slot,(u64)p,0); }

/* IPC: mensagem = tag + 6 palavras */
typedef struct { u64 tag; u64 data[6]; } Msg;
static inline i64  ipc_send(u64 dst, Msg* m){ return (i64)__syscall(SYS_IPC_SEND,dst,(u64)m,0); }

/* bytes aleatorios do kernel (virtio-rng): n (<= 4096) ou -1.
 * sys_getrandom(NULL, 0) diz se a fonte e criptografica (1) ou nao (0). */
static inline i64   sys_getrandom(void* p, u64 n){ return (i64)__syscall(SYS_GETRANDOM,(u64)p,n,0); }

/* ---- threads (mesmo espaco de enderecamento) e futex ---- */
static inline i64   sys_thread_create(u64 entry, u64 stack_top, u64 arg){ return (i64)__syscall(SYS_THREAD_CREATE,entry,stack_top,arg); }
/* dorme se *a == v: 0 acordado, -1 valor ja mudou, -2 tempo esgotado (ms = 0: sem limite) */
static inline i64   sys_futex_wait(volatile u32* a, u32 v, u64 ms){ return (i64)__syscall(SYS_FUTEX_WAIT,(u64)a,v,ms); }
static inline i64   sys_futex_wake(volatile u32* a, u64 n){ return (i64)__syscall(SYS_FUTEX_WAKE,(u64)a,n,0); }
/* termina o processo inteiro; sys_exit numa thread termina so ela */
static inline __attribute__((noreturn)) void sys_exit_group(void){ __syscall(SYS_EXIT_GROUP,0,0,0); for(;;){} }

/* thread.c: threads e exclusao mutua */
typedef struct { volatile u32 v; } Mutex;          /* 0 livre, 1 travado, 2 travado com espera */
#define MUTEX_INIT { 0 }
void mutex_lock(Mutex* m);
void mutex_unlock(Mutex* m);
/* nova thread rodando fn(arg) numa pilha de `stack` bytes; tid ou -1 */
i64  thread_spawn(void (*fn)(void*), void* arg, u64 stack);
u32  cpu_count(void);
extern int threads_started;                         /* 1 depois da primeira thread */

/* heap: aumenta em n bytes; retorna o fim anterior ou -1 */
static inline i64   sys_sbrk(u64 n){ return (i64)__syscall(SYS_SBRK,n,0,0); }

/* ---- memoria dinamica (malloc.c) ---- */
void*  malloc(size_t n);
void   free(void* p);
void*  calloc(size_t n, size_t sz);
void*  realloc(void* p, size_t n);

/* ---- string/memoria (string.c) ---- */
void*  memcpy(void* d, const void* s, size_t n);
void*  memmove(void* d, const void* s, size_t n);
void*  memset(void* d, int c, size_t n);
int    memcmp(const void* a, const void* b, size_t n);
size_t strlen(const char* s);
int    strcmp(const char* a, const char* b);
int    strncmp(const char* a, const char* b, size_t n);
char*  strcpy(char* d, const char* s);
void   puts_(const char* s);
/* formatacao simples: inteiro -> texto; retorna o fim da string */
char*  fmt_u(char* out, u64 v);
char*  fmt_i(char* out, i64 v);
char*  fmt_s(char* out, const char* s);
char*  fmt_size(char* out, u64 bytes);
int    utf8_encode(u32 cp, char* out);
/* data civil a partir de segundos Unix */
void   civil_time(u64 secs, u32* y, u32* mo, u32* d, u32* h, u32* mi, u32* s, u32* wd);

/* ---- graficos (graphics.c) ---- */
typedef struct { i32 x, y, w, h; } Rect;
typedef struct { u32* base; i32 w, h, stride; Rect clip; } Fb;

static inline Rect rect(i32 x, i32 y, i32 w, i32 h){ Rect r={x,y,w,h}; return r; }
static inline int rect_has(Rect r, i32 px, i32 py){ return px>=r.x && px<r.x+r.w && py>=r.y && py<r.y+r.h; }
Rect rect_and(Rect a, Rect b);

u32  mix(u32 bg, u32 fg, u32 a);
u32  shade(u32 c, i32 t);
u32  isqrt_u(u32 n);
void fb_set_clip(Fb* fb, Rect r);
void fb_fill(Fb* fb, Rect r, u32 c);
void fb_fill_a(Fb* fb, Rect r, u32 c, u32 a);
void fb_blend(Fb* fb, i32 x, i32 y, u32 c, u32 a);
void fb_hline(Fb* fb, i32 x, i32 y, i32 w, u32 c, u32 a);
void fb_vgrad(Fb* fb, Rect r, u32 top, u32 bot);
void fb_round_rect(Fb* fb, Rect r, i32 rad, u32 c);
void fb_round_rect_a(Fb* fb, Rect r, i32 rad, u32 c, u32 a);
void fb_round_grad(Fb* fb, Rect r, i32 rad, u32 top, u32 bot);
void fb_round_border(Fb* fb, Rect r, i32 rad, u32 c, u32 a);
void fb_shadow(Fb* fb, Rect r, i32 rad, i32 blur, i32 dy, u32 a);
void fb_disc(Fb* fb, i32 cx, i32 cy, i32 r, u32 c, u32 a);
void fb_ring(Fb* fb, i32 cx, i32 cy, i32 r, i32 t, u32 c, u32 a);
void fb_line(Fb* fb, i32 x0, i32 y0, i32 x1, i32 y1, i32 width, u32 c, u32 a);

/* ---- texto UTF-8 com as fontes Noto (text.c + font_*.c) ---- */
#include "fonts.h"
i32  text_draw(Fb* fb, const Font* f, i32 x, i32 y, const char* s, u32 c);
i32  text_draw_n(Fb* fb, const Font* f, i32 x, i32 y, const char* s, int n, u32 c);
i32  text_width(const Font* f, const char* s);
i32  text_width_n(const Font* f, const char* s, int n);
void text_center(Fb* fb, const Font* f, Rect r, const char* s, u32 c);
i32  text_right(Fb* fb, const Font* f, i32 right, i32 y, const char* s, u32 c);
u32  utf8_next(const char* s, int* i);

/* ---- tema (claro/escuro + destaque) ---- */
typedef struct {
    int dark; u32 accent;
    u32 win_bg, side_bg, text, text2, text3, sep, ctrl_bg, ctrl_border, field_bg, hover;
} Theme;
Theme theme_make(int dark, u32 accent);
void  ui_button(Fb* fb, Rect r, const char* label, const Theme* th, int primary, int hover);

/* ---- protocolo do WM ---- */
#define SURF_W 1280
#define SURF_H 816
#define SVC_FS 1
#define SVC_WM 2
enum { FS_OP_COUNT=1, FS_OP_STAT=2, FS_OP_READ=3 };
enum { WM_OP_HELLO=1, WM_OP_COMMIT=2, WM_OP_POLL=3, WM_OP_TITLE=4 };

#define APP_MENU_SAVE   1
#define APP_MENU_NEW    2
#define APP_MENU_COPY   3
#define APP_MENU_PASTE  4
#define APP_MENU_UNDO   5
#define APP_MENU_CLEAR  6
#define APP_MENU_PAUSE  7
#define APP_MENU_RESET  8
#define APP_MENU_RELOAD 12
#define APP_MENU_ADD    13

#define MOD_SHIFT 1
#define MOD_CTRL  2
#define MOD_ALT   4

/* codigos evdev */
#define KEY_ESC 1
#define KEY_BACKSPACE 14
#define KEY_TAB 15
#define KEY_ENTER 28
#define KEY_UP 103
#define KEY_LEFT 105
#define KEY_RIGHT 106
#define KEY_DOWN 108
#define KEY_HOME 102
#define KEY_END 107
#define KEY_PGUP 104
#define KEY_PGDN 109
#define KEY_DELETE 111
#define KEY_KPENTER 96

typedef struct {
    int close, focused, dark, visible, more, changed;
    i32 w, h, mx, my;
    int down, click, rclick, dbl;
    i32 wheel;
    u16 key; u8 mods; u32 ch;
    u16 menu; u32 accent;
} WmEvent;

typedef struct {
    u64 wm, slot;
    Fb fb;
    i32 w, h;
    Theme th;
    int focused;
    i64 open_file;   /* -1 = nenhum */
} App;

void    app_init(App* a, u64 kind, const char* title, i32 w, i32 h);
WmEvent app_poll(App* a);
WmEvent app_peek(App* a);   /* app_poll sem esperar (responde na hora) */
WmEvent app_commit(App* a);
void    app_title(App* a, const char* t);
static inline int ev_interesting(const WmEvent* e){
    return e->changed || e->click || e->rclick || e->wheel || e->key || e->menu;
}

#endif
