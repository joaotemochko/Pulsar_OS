/* wlclient — tradutor Wayland <-> Pulsar para UM programa (iniciado pelo
 * wlsrv com a conexao no descritor 3).
 *
 * Do lado do cliente fala o protocolo Wayland (wire format, descritores por
 * SCM_RIGHTS); do lado do Pulsar e um app comum do gerenciador de janelas
 * (HELLO/POLL/COMMIT da libpulsar). Mouse e teclado do WM viram eventos
 * wl_pointer/wl_keyboard (os codigos de tecla do Pulsar ja sao os do evdev,
 * os mesmos do Wayland).
 *
 * Cena: a janela (o primeiro xdg_toplevel) pode ter subsuperficies
 * (wl_subcompositor) e popups (xdg_popup: menus, listas, dicas). O WM so
 * conhece uma superficie por janela, entao a cena e composta AQUI: o
 * conteudo da janela, depois as subsuperficies, depois os popups, com
 * alpha. Popups ficam dentro da area da janela (o posicionador desliza ou
 * espelha o popup para caber). A entrada vai para a superficie sob o
 * ponteiro; com um popup "agarrado" (grab), clicar fora dele o fecha.
 *
 * Suporte: wl_compositor 4, wl_subcompositor 1, wl_shm 1, xdg_wm_base 2,
 * wl_seat 5, wl_output 2. Se o cliente fizer algo invalido, recebe
 * wl_display.error e so este processo termina. */
#include "pulsar.h"
#include <string.h>

#define CONN 3
#define MAXOBJ 512
#define KIND_LINUX 99

enum { T_FREE, T_DISPLAY, T_REGISTRY, T_CALLBACK, T_COMPOSITOR, T_SURFACE, T_REGION, T_SHM, T_POOL,
       T_BUFFER, T_WMBASE, T_XDGSURF, T_TOPLEVEL, T_SEAT, T_POINTER, T_KEYBOARD, T_OUTPUT, T_POSITIONER,
       T_SUBCOMP, T_SUBSURF, T_POPUP };

/* papel de uma wl_surface */
enum { R_NONE, R_TOPLEVEL, R_SUB, R_POPUP };

typedef struct {
    u32 id;
    u8 type;
    u32 version;
    /* pool: base, size, fd, shm, refs (buffers vivos), dead (destroy pedido) |
     * buffer: pool, offset, w, h, stride, fmt | xdg_surface: surface |
     * toplevel/popup/subsurface: surface */
    u64 base, size;
    i32 fd, shm, refs, dead;
    u32 ref, offset, w, h, stride, fmt;
    u32 cbs[16];
    int ncb;
    /* ---- surface ---- */
    int attached;              /* attach desde o ultimo commit */
    i32 dx0, dy0, dx1, dy1;    /* dano acumulado (caixa; dx1 <= dx0 = nenhum) */
    u8 role, mapped, alpha, grab;
    u32 parent;                /* surface pai (subsurface/popup) */
    u32 xdg, roleobj;          /* xdg_surface; xdg_popup/subsurface */
    i32 px, py;                /* subsurface: no pai | popup: na geometria do pai */
    i32 gx, gy, gw, gh;        /* xdg_surface.set_window_geometry (gw = 0: nenhuma) */
    u32* st; i32 sw, sh;       /* conteudo comprometido (subsurface/popup) */
    i32 z;                     /* ordem entre irmaos / de abertura dos popups */
    /* ---- positioner: w, h, ax, ay, aw, ah, anchor, gravity, ajuste, ox, oy ---- */
    i32 pos[11];
} Obj;

static Obj objs[MAXOBJ];
static App app;
static int have_app;
static u32 serial = 1;
static char title[32] = "Linux";
static u32 top_surface, top_xdg, top_level;       /* a janela */
static u32 pointer_id, keyboard_id;
static u32 kb_version;
static i32 cfg_w, cfg_h;
static i32 zcount = 1;

/* ------------------------------------------------------------ objetos */

static Obj* find(u32 id) {
    if (!id) return 0;
    for (int i = 0; i < MAXOBJ; i++) if (objs[i].type != T_FREE && objs[i].id == id) return &objs[i];
    return 0;
}

static Obj* add(u32 id, int type, u32 version) {
    for (int i = 0; i < MAXOBJ; i++)
        if (objs[i].type == T_FREE) {
            memset(&objs[i], 0, sizeof objs[i]);
            objs[i].id = id; objs[i].type = (u8)type; objs[i].version = version; objs[i].fd = -1;
            return &objs[i];
        }
    return 0;
}

/* ------------------------------------------------------------ saida (eventos) */

static u8 out[64 * 1024];
static u32 outlen;
static i32 outfds[8];
static int noutfds;

typedef struct { u8* p; u32 start; } Ev;

static Ev ev_begin(u32 obj, u32 op) {
    Ev e = { out, outlen };
    *(u32*)(out + outlen) = obj;
    *(u32*)(out + outlen + 4) = op;          /* tamanho entra em ev_end */
    outlen += 8;
    return e;
}
static void put_u32(u32 v) { *(u32*)(out + outlen) = v; outlen += 4; }
static void put_str(const char* s) {
    u32 n = (u32)strlen(s) + 1;
    put_u32(n);
    memcpy(out + outlen, s, n);
    memset(out + outlen + n, 0, (4 - n % 4) % 4);
    outlen += (n + 3) & ~3u;
}
static void put_array(const u32* v, u32 count) {
    put_u32(count * 4);
    for (u32 i = 0; i < count; i++) put_u32(v[i]);
}
static void ev_end(Ev e) {
    u32 size = outlen - e.start;
    *(u32*)(out + e.start + 4) |= size << 16;
}

static void flush(void) {
    if (!outlen) return;
    PIovec iov = { out, outlen };
    u8 ctl[sizeof(PCmsgHdr) + 8 * 4];
    PMsgHdr m; memset(&m, 0, sizeof m);
    m.iov = &iov; m.iovlen = 1;
    if (noutfds) {
        PCmsgHdr* c = (PCmsgHdr*)ctl;
        c->len = sizeof(PCmsgHdr) + 4 * (u64)noutfds; c->level = 1; c->type = 1;
        memcpy(ctl + sizeof(PCmsgHdr), outfds, 4 * noutfds);
        m.control = ctl; m.controllen = (c->len + 7) & ~7ull;
    }
    i64 r = sys_sendmsg(CONN, &m, 0x4000 /* MSG_NOSIGNAL */);
    for (int i = 0; i < noutfds; i++) sys_close(outfds[i]);
    noutfds = 0;
    if (r < 0) sys_exit_group();             /* cliente foi embora */
    outlen = 0;
}

static void ensure_room(void) { if (outlen > sizeof out - 4096) flush(); }

static void delete_id(u32 id) {
    Ev e = ev_begin(1, 1); put_u32(id); ev_end(e);
}

static __attribute__((noreturn)) void protocol_error(u32 obj, u32 code, const char* msg) {
    Ev e = ev_begin(1, 0); put_u32(obj); put_u32(code); put_str(msg); ev_end(e);
    flush();
    puts_("[wlclient] erro de protocolo do cliente: "); puts_(msg); puts_("\n");
    sys_exit_group();
}

/* ------------------------------------------------------------ entrada (pedidos) */

static u8 in[64 * 1024];
static u32 inlen;
static i32 infds[32];
static int ninfds;

static int read_client(void) {
    if (inlen >= sizeof in) protocol_error(1, 1, "mensagem grande demais");
    PIovec iov = { in + inlen, sizeof in - inlen };
    u8 ctl[sizeof(PCmsgHdr) + 16 * 4];
    PMsgHdr m; memset(&m, 0, sizeof m);
    m.iov = &iov; m.iovlen = 1; m.control = ctl; m.controllen = sizeof ctl;
    i64 n = sys_recvmsg(CONN, &m, 0x40 /* MSG_DONTWAIT */ | 0x40000000 /* MSG_CMSG_CLOEXEC */);
    if (n == -11) return 1;                   /* nada agora */
    if (n <= 0) return 0;
    inlen += (u32)n;
    if (m.controllen >= sizeof(PCmsgHdr)) {
        PCmsgHdr* c = (PCmsgHdr*)ctl;
        int k = (int)((c->len - sizeof(PCmsgHdr)) / 4);
        for (int i = 0; i < k && ninfds < 32; i++) infds[ninfds++] = ((i32*)(ctl + sizeof(PCmsgHdr)))[i];
    }
    return 1;
}

static i32 take_fd(void) {
    if (!ninfds) protocol_error(1, 1, "descritor esperado");
    i32 f = infds[0];
    memmove(infds, infds + 1, sizeof(i32) * --ninfds);
    return f;
}

/* cursor de argumentos */
typedef struct { const u8* p; const u8* end; } Args;
static u32 a_u32(Args* a) { if (a->p + 4 > a->end) protocol_error(1, 1, "mensagem curta"); u32 v = *(const u32*)a->p; a->p += 4; return v; }
static i32 a_i32(Args* a) { return (i32)a_u32(a); }
static const char* a_str(Args* a) {
    u32 n = a_u32(a);
    const char* s = (const char*)a->p;
    if (n == 0) return "";
    if (a->p + ((n + 3) & ~3u) > a->end || s[n - 1]) protocol_error(1, 1, "texto invalido");
    a->p += (n + 3) & ~3u;
    return s;
}

/* ------------------------------------------------------------ globais */

static const struct { const char* name; int type; u32 version; } GLOBALS[] = {
    { "wl_compositor", T_COMPOSITOR, 4 },
    { "wl_shm", T_SHM, 1 },
    { "xdg_wm_base", T_WMBASE, 2 },
    { "wl_seat", T_SEAT, 5 },
    { "wl_output", T_OUTPUT, 2 },
    { "wl_subcompositor", T_SUBCOMP, 1 },
};
#define NGLOBALS (sizeof GLOBALS / sizeof GLOBALS[0])

static void bind_global(u32 name, const char* iface, u32 version, u32 id) {
    if (name < 1 || name > NGLOBALS || strcmp(iface, GLOBALS[name - 1].name) != 0)
        protocol_error(1, 0, "global invalido");
    if (version > GLOBALS[name - 1].version) version = GLOBALS[name - 1].version;
    Obj* o = add(id, GLOBALS[name - 1].type, version);
    if (!o) protocol_error(1, 2, "objetos demais");
    if (o->type == T_SHM) {
        Ev e = ev_begin(id, 0); put_u32(0); ev_end(e);   /* ARGB8888 */
        e = ev_begin(id, 0); put_u32(1); ev_end(e);      /* XRGB8888 */
    } else if (o->type == T_SEAT) {
        Ev e = ev_begin(id, 0); put_u32(3); ev_end(e);   /* ponteiro + teclado */
        if (version >= 2) { e = ev_begin(id, 1); put_str("seat0"); ev_end(e); }
    } else if (o->type == T_OUTPUT) {
        SysInfo si; sys_sysinfo(&si);
        Ev e = ev_begin(id, 0);
        put_u32(0); put_u32(0); put_u32(340); put_u32(215); put_u32(0);
        put_str("Pulsar"); put_str("Tela"); put_u32(0);
        ev_end(e);
        e = ev_begin(id, 1); put_u32(3); put_u32(si.fb_w); put_u32(si.fb_h); put_u32(60000); ev_end(e);
        if (version >= 2) { e = ev_begin(id, 3); put_u32(1); ev_end(e); e = ev_begin(id, 2); ev_end(e); }
    }
}

/* ------------------------------------------------------------ janela */

static void send_configure(void) {
    if (!top_level) return;
    u32 states[1] = { 4 };                                /* activated */
    Ev e = ev_begin(top_level, 0); put_u32((u32)cfg_w); put_u32((u32)cfg_h); put_array(states, 1); ev_end(e);
    e = ev_begin(top_xdg, 0); put_u32(serial++); ev_end(e);
}

static void handle_wm(WmEvent e);

/* ------------------------------------------------------------ teclado (XKB) */

static char layout[16] = "abnt2";
static u64 layout_checked;

/* layout atual do pulsar.cfg (keyboard=...); devolve 1 se mudou */
static int read_layout(void) {
    static char buf[2048];
    i64 fd = sys_open("/pulsar.cfg", P_O_RDONLY, 0);
    if (fd < 0) return 0;
    i64 n = sys_read(fd, buf, sizeof buf - 1);
    sys_close(fd);
    if (n <= 0) return 0;
    buf[n] = 0;
    for (char* p = buf; *p; ) {
        char* e = p; while (*e && *e != '\n') e++;
        if (!strncmp(p, "keyboard=", 9)) {
            char nl[16]; int k = 0;
            for (char* q = p + 9; q < e && k < 15 && *q != ' ' && *q != '\r'; q++) nl[k++] = *q;
            nl[k] = 0;
            if (k && strcmp(nl, layout)) { strcpy(layout, nl); return 1; }
            return 0;
        }
        p = *e ? e + 1 : e;
    }
    return 0;
}

/* wl_keyboard.keymap: o mapa XKB do layout (gerado por tools/genkeymaps.py),
 * numa memoria compartilhada terminada em NUL */
static void send_keymap(void) {
    if (!keyboard_id) return;
    read_layout();
    char path[64]; char* p = fmt_s(path, "/usr/share/pulsar/xkb/"); p = fmt_s(p, layout); fmt_s(p, ".xkb");
    PStat st;
    i64 fd = sys_open(path, P_O_RDONLY, 0);
    i64 km = sys_memfd(1);
    if (fd < 0 || km < 0 || sys_fstat(fd, &st) < 0 || noutfds >= 7) {
        if (fd >= 0) sys_close(fd);
        if (km >= 0) sys_close(km);
        return;
    }
    u64 size = (u64)st.size + 1;
    sys_ftruncate(km, size);
    static char chunk[4096];
    u64 off = 0;
    for (i64 n; (n = sys_read(fd, chunk, sizeof chunk)) > 0; off += (u64)n) sys_pwrite(km, chunk, (u64)n, off);
    sys_close(fd);
    outfds[noutfds++] = (i32)km;
    Ev e = ev_begin(keyboard_id, 0); put_u32(1); put_u32((u32)size); ev_end(e);   /* xkb_v1 */
}

/* modificadores no formato XKB: Shift 0x1, Lock 0x2, Control 0x4, Mod1 (Alt)
 * 0x8, Mod2 (NumLock) 0x10, Mod4 (Super) 0x40, Mod5 (AltGr) 0x80 */
static void send_mods(u8 m) {
    u32 dep = (m & MOD_SHIFT ? 0x1 : 0) | (m & MOD_CTRL ? 0x4 : 0) | (m & MOD_ALT ? 0x8 : 0)
            | (m & MOD_META ? 0x40 : 0) | (m & MOD_ALTGR ? 0x80 : 0);
    u32 lock = (m & MOD_CAPS ? 0x2 : 0) | 0x10;
    Ev v = ev_begin(keyboard_id, 4); put_u32(serial++); put_u32(dep); put_u32(0); put_u32(lock); put_u32(0); ev_end(v);
}

/* ------------------------------------------------------------ cena */

/* Coordenadas "da janela" = as da superficie do WM: a origem e o canto da
 * geometria (xdg_surface.set_window_geometry) do toplevel. */

static u32* base_st;           /* conteudo do toplevel em coordenadas da janela */
static i32 base_w, base_h;
static int base_valid;         /* base_st espelha o toplevel (so mantido com filhos) */
static i32 wd0, wd1, wd2, wd3; /* dano da janela: x0, y0, x1, y1 (wd2 <= wd0 = nenhum) */
static u32 pend_cbs[64];       /* callbacks de quadro a disparar depois de compor */
static int npend_cbs;

static void win_damage(i32 x, i32 y, i32 w, i32 h) {
    if (w <= 0 || h <= 0) return;
    if (wd2 <= wd0) { wd0 = x; wd1 = y; wd2 = x + w; wd3 = y + h; return; }
    if (x < wd0) wd0 = x;
    if (y < wd1) wd1 = y;
    if (x + w > wd2) wd2 = x + w;
    if (y + h > wd3) wd3 = y + h;
}

static Obj* surf(u32 id) { Obj* s = find(id); return s && s->type == T_SURFACE ? s : 0; }

/* um popup/subsuperficie visivel: mapeado e com a cadeia de pais ativa */
static int visible(Obj* s) {
    for (int guard = 0; s && guard < 16; guard++) {
        if (s->id == top_surface) return 1;
        if (!s->mapped || s->role == R_NONE) return 0;
        s = surf(s->parent);
    }
    return 0;
}

/* canto superior esquerdo da SUPERFICIE em coordenadas da janela */
static void abs_pos(Obj* s, i32* x, i32* y) {
    i32 ax = 0, ay = 0;
    for (int guard = 0; s && guard < 16; guard++) {
        if (s->id == top_surface) { ax -= s->gw ? s->gx : 0; ay -= s->gw ? s->gy : 0; break; }
        if (s->role == R_SUB) { ax += s->px; ay += s->py; }
        else if (s->role == R_POPUP) {
            /* popup: posicao relativa a geometria do pai; a superficie
             * comeca antes da propria geometria (sombras) */
            ax += s->px - (s->gw ? s->gx : 0); ay += s->py - (s->gw ? s->gy : 0);
            Obj* p = surf(s->parent);
            if (p) { ax += p->gw ? p->gx : 0; ay += p->gw ? p->gy : 0; }
        } else break;
        s = surf(s->parent);
    }
    *x = ax; *y = ay;
}

static int has_children(void) {
    for (int i = 0; i < MAXOBJ; i++) {
        Obj* s = &objs[i];
        if (s->type == T_SURFACE && (s->role == R_SUB || s->role == R_POPUP) && s->mapped && visible(s)) return 1;
    }
    return 0;
}

/* premultiplicado (ARGB8888 do Wayland) sobre o destino */
static inline u32 over(u32 d, u32 s) {
    u32 a = s >> 24;
    if (a == 255) return s & 0xFFFFFF;
    if (a == 0 && !(s & 0xFFFFFF)) return d;
    u32 ia = 255 - a;
    u32 rb = ((d & 0xFF00FF) * ia + 0x800080) >> 8 & 0xFF00FF;
    u32 g = ((d & 0x00FF00) * ia + 0x008000) >> 8 & 0x00FF00;
    u32 r = (s & 0xFF00FF) + rb, gg = (s & 0x00FF00) + g;
    if (r & 0x01000000) r |= 0xFF0000;
    if (r & 0x100) r |= 0xFF;
    if (gg > 0xFF00) gg = 0xFF00;
    return (r & 0xFF00FF) | (gg & 0xFF00);
}

/* desenha a superficie s (conteudo em s->st) na janela, so dentro de clip */
static void draw_surface(Obj* s, i32 cx0, i32 cy0, i32 cx1, i32 cy1) {
    if (!s->st) return;
    i32 ax, ay; abs_pos(s, &ax, &ay);
    i32 x0 = ax > cx0 ? ax : cx0, y0 = ay > cy0 ? ay : cy0;
    i32 x1 = ax + s->sw < cx1 ? ax + s->sw : cx1, y1 = ay + s->sh < cy1 ? ay + s->sh : cy1;
    for (i32 y = y0; y < y1; y++) {
        u32* d = app.fb.base + (u64)y * SURF_W;
        const u32* sp = s->st + (u64)(y - ay) * s->sw - ax;
        if (!s->alpha) memcpy(d + x0, sp + x0, (u64)(x1 - x0) * 4);
        else for (i32 x = x0; x < x1; x++) d[x] = over(d[x], sp[x]);
    }
}

/* filhos (subsuperficies) de p em ordem de z, recursivo */
static void draw_subs(u32 parent, i32 cx0, i32 cy0, i32 cx1, i32 cy1) {
    i32 last = -0x7fffffff;
    for (;;) {                                     /* menor z ainda nao desenhado */
        Obj* best = 0;
        for (int i = 0; i < MAXOBJ; i++) {
            Obj* s = &objs[i];
            if (s->type != T_SURFACE || s->role != R_SUB || s->parent != parent || !s->mapped) continue;
            if (s->z > last && (!best || s->z < best->z)) best = s;
        }
        if (!best) return;
        last = best->z;
        draw_surface(best, cx0, cy0, cx1, cy1);
        draw_subs(best->id, cx0, cy0, cx1, cy1);
    }
}

/* recompõe [x0,x1)x[y0,y1) da janela a partir de base_st + filhos */
static void compose(i32 x0, i32 y0, i32 x1, i32 y1) {
    if (x0 < 0) x0 = 0;
    if (y0 < 0) y0 = 0;
    if (x1 > app.w) x1 = app.w;
    if (y1 > app.h) y1 = app.h;
    if (x0 >= x1 || y0 >= y1 || !base_valid) return;
    for (i32 y = y0; y < y1; y++)
        memcpy(app.fb.base + (u64)y * SURF_W + x0, base_st + (u64)y * base_w + x0, (u64)(x1 - x0) * 4);
    draw_subs(top_surface, x0, y0, x1, y1);
    /* popups por ordem de abertura (os mais novos por cima) */
    i32 last = -0x7fffffff;
    for (;;) {
        Obj* best = 0;
        for (int i = 0; i < MAXOBJ; i++) {
            Obj* s = &objs[i];
            if (s->type != T_SURFACE || s->role != R_POPUP || !visible(s)) continue;
            if (s->z > last && (!best || s->z < best->z)) best = s;
        }
        if (!best) break;
        last = best->z;
        draw_surface(best, x0, y0, x1, y1);
        draw_subs(best->id, x0, y0, x1, y1);
    }
}

/* base_st do tamanho da janela; se acabou de ser criada, copia o que esta na tela */
static void ensure_base(void) {
    if (base_st && base_w == app.w && base_h == app.h && base_valid) return;
    if (!base_st || base_w != app.w || base_h != app.h) {
        free(base_st);
        base_st = malloc((u64)app.w * app.h * 4);
        base_w = app.w; base_h = app.h;
        if (!base_st) { base_valid = 0; return; }
    }
    for (i32 y = 0; y < base_h; y++)
        memcpy(base_st + (u64)y * base_w, app.fb.base + (u64)y * SURF_W, (u64)base_w * 4);
    base_valid = 1;
}

/* copia o retangulo do buffer para dst (stride dst_w), convertendo alpha
 * do toplevel (sobre branco) quando opaque_bg */
static void copy_rows(u32* dst, i32 dst_stride, i32 dx, i32 dy, const u8* src, u32 stride,
                      i32 sx, i32 sy, i32 w, i32 h, int argb_over_white) {
    for (i32 y = 0; y < h; y++) {
        u32* d = dst + (u64)(dy + y) * dst_stride + dx;
        const u32* s = (const u32*)(src + (u64)(sy + y) * stride) + sx;
        if (!argb_over_white) memcpy(d, s, (u64)w * 4);
        else for (i32 x = 0; x < w; x++) d[x] = over(0xFFFFFF, s[x]);
    }
}

static void release_pool(Obj* pool) {
    if (pool->base) sys_munmap(pool->base, pool->size);
    if (pool->fd >= 0) sys_close(pool->fd);
    pool->base = 0; pool->fd = -1;
}

/* o commit da superficie s: novo conteudo (se houve attach) e callbacks */
static void surface_commit(Obj* s) {
    if (s->attached) {
        s->attached = 0;
        Obj* buf = find(s->ref);
        i32 ax, ay;
        if (!buf || buf->type != T_BUFFER) {                       /* attach(NULL): desmapeia */
            if (s->mapped && s->role != R_TOPLEVEL && visible(s)) { abs_pos(s, &ax, &ay); win_damage(ax, ay, s->sw, s->sh); }
            s->mapped = 0;
        } else {
            Obj* pool = (Obj*)buf->base;                           /* vivo enquanto houver buffers */
            i32 bw = (i32)buf->w, bh = (i32)buf->h;
            int ok = pool && pool->base && (u64)buf->offset + (u64)buf->stride * buf->h <= pool->size;
            if (ok && !pool->shm)                                  /* arquivo comum: copia privada, rele */
                sys_pread(pool->fd, (void*)(pool->base + buf->offset), (u64)buf->stride * buf->h, buf->offset);
            /* dano declarado (coordenadas da superficie) ou tudo */
            i32 x0 = 0, y0 = 0, x1 = bw, y1 = bh;
            int sized = s->sw == bw && s->sh == bh && s->mapped;
            if (sized && s->dx1 > s->dx0) {
                x0 = s->dx0 < 0 ? 0 : s->dx0; y0 = s->dy0 < 0 ? 0 : s->dy0;
                x1 = s->dx1 > bw ? bw : s->dx1; y1 = s->dy1 > bh ? bh : s->dy1;
            }
            const u8* src = ok ? (const u8*)pool->base + buf->offset : 0;
            if (s->role == R_TOPLEVEL) {
                if (!have_app) {
                    i32 w = s->gw ? s->gw : bw, h = s->gw ? s->gh : bh;
                    app_init(&app, KIND_LINUX, title, w, h);
                    have_app = 1;
                    cfg_w = app.w; cfg_h = app.h;
                }
                /* buffer -> janela: desloca pela geometria, recorta na janela */
                i32 ox = s->gw ? s->gx : 0, oy = s->gw ? s->gy : 0;
                i32 wx0 = x0 - ox, wy0 = y0 - oy, wx1 = x1 - ox, wy1 = y1 - oy;
                if (wx0 < 0) wx0 = 0;
                if (wy0 < 0) wy0 = 0;
                if (wx1 > app.w) wx1 = app.w;
                if (wy1 > app.h) wy1 = app.h;
                int kids = has_children();
                if (kids) ensure_base();
                u32* dst = kids && base_valid ? base_st : app.fb.base;
                i32 dstride = kids && base_valid ? base_w : SURF_W;
                if (!sized)                                        /* tamanho novo: o que o buffer nao cobre fica branco */
                    for (i32 y = 0; y < app.h; y++)
                        for (i32 x = 0; x < app.w; x++) dst[(u64)y * dstride + x] = 0xFFFFFF;
                if (src && wx0 < wx1 && wy0 < wy1) {
                    if (kids && base_valid)
                        copy_rows(base_st, base_w, wx0, wy0, src, buf->stride, wx0 + ox, wy0 + oy, wx1 - wx0, wy1 - wy0, buf->fmt == 0);
                    else {
                        copy_rows(app.fb.base, SURF_W, wx0, wy0, src, buf->stride, wx0 + ox, wy0 + oy, wx1 - wx0, wy1 - wy0, buf->fmt == 0);
                        base_valid = 0;                            /* sem filhos: a tela e a copia */
                    }
                }
                if (!sized) win_damage(0, 0, app.w, app.h);
                else win_damage(wx0, wy0, wx1 - wx0, wy1 - wy0);
                s->sw = bw; s->sh = bh; s->mapped = 1;
            } else if (s->role == R_SUB || s->role == R_POPUP) {
                int was = s->mapped && visible(s);
                if (was) { abs_pos(s, &ax, &ay); win_damage(ax, ay, s->sw, s->sh); }
                if (!s->st || s->sw != bw || s->sh != bh) {
                    free(s->st);
                    s->st = malloc((u64)bw * bh * 4);
                    s->sw = bw; s->sh = bh;
                    x0 = 0; y0 = 0; x1 = bw; y1 = bh;
                    if (s->st) memset(s->st, 0, (u64)bw * bh * 4);
                }
                if (s->st && src && x0 < x1 && y0 < y1)
                    copy_rows(s->st, bw, x0, y0, src, buf->stride, x0, y0, x1 - x0, y1 - y0, 0);
                s->alpha = buf->fmt == 0;
                s->mapped = 1;
                if (visible(s)) {
                    if (have_app) ensure_base();
                    abs_pos(s, &ax, &ay);
                    if (was) win_damage(ax + x0, ay + y0, x1 - x0, y1 - y0);
                    else win_damage(ax, ay, bw, bh);
                }
            }
            Ev e = ev_begin(buf->id, 0); ev_end(e);                /* wl_buffer.release: ja copiado */
        }
    }
    s->dx0 = s->dy0 = 0; s->dx1 = s->dy1 = -1;
    for (int i = 0; i < s->ncb; i++)
        if (npend_cbs < 64) pend_cbs[npend_cbs++] = s->cbs[i];
        else { Ev e = ev_begin(s->cbs[i], 0); put_u32((u32)sys_time_ms()); ev_end(e); delete_id(s->cbs[i]); }
    s->ncb = 0;
}

/* fim de um lote de pedidos: compõe o que mudou, entrega ao WM e dispara
 * os callbacks de quadro */
static void present(void) {
    if (wd2 > wd0 && have_app) {
        i32 x0 = wd0 < 0 ? 0 : wd0, y0 = wd1 < 0 ? 0 : wd1;
        i32 x1 = wd2 > app.w ? app.w : wd2, y1 = wd3 > app.h ? app.h : wd3;
        if (base_valid) compose(x0, y0, x1, y1);
        wd0 = wd1 = 0; wd2 = wd3 = -1;
        if (x0 < x1 && y0 < y1) {
            Rect d = (x0 == 0 && y0 == 0 && x1 == app.w && y1 == app.h) ? rect(0, 0, 0, 0) : rect(x0, y0, x1 - x0, y1 - y0);
            /* a resposta do COMMIT tambem traz eventos (teclas, cliques): nao perde */
            WmEvent we = app_commit_rect(&app, d);
            handle_wm(we);
        }
    }
    if (npend_cbs) {
        u32 now = (u32)sys_time_ms();
        for (int i = 0; i < npend_cbs; i++) {
            Ev e = ev_begin(pend_cbs[i], 0); put_u32(now); ev_end(e);
            Obj* cb = find(pend_cbs[i]);
            if (cb) cb->type = T_FREE;
            delete_id(pend_cbs[i]);
            ensure_room();
        }
        npend_cbs = 0;
    }
}

/* ------------------------------------------------------------ popups */

static void kb_refocus(void);

/* posicao (relativa a geometria do pai) para o posicionador p, com os
 * eixos opcionalmente espelhados (constraint_adjustment flip_x / flip_y) */
static void popup_xy(const i32* p, int fx, int fy, i32 w, i32 h, i32* ox, i32* oy) {
    static const i32 mx[9] = { 0, 1, 2, 4, 3, 7, 8, 5, 6 };   /* troca esquerda/direita */
    static const i32 my[9] = { 0, 2, 1, 3, 4, 6, 5, 8, 7 };   /* troca cima/baixo */
    i32 an = p[6] % 9, gr = p[7] % 9;
    if (an < 0) an = 0;
    if (gr < 0) gr = 0;
    if (fx) { an = mx[an]; gr = mx[gr]; }
    if (fy) { an = my[an]; gr = my[gr]; }
    /* ancora: 1 cima, 2 baixo, 3 esq, 4 dir, 5 cima-esq, 6 baixo-esq, 7 cima-dir, 8 baixo-dir */
    i32 x = (an == 3 || an == 5 || an == 6) ? p[2] : (an == 4 || an == 7 || an == 8) ? p[2] + p[4] : p[2] + p[4] / 2;
    i32 y = (an == 1 || an == 5 || an == 7) ? p[3] : (an == 2 || an == 6 || an == 8) ? p[3] + p[5] : p[3] + p[5] / 2;
    x += fx ? -p[9] : p[9];
    y += fy ? -p[10] : p[10];
    /* gravidade: para que lado o popup cresce a partir da ancora */
    if (gr == 3 || gr == 5 || gr == 6) x -= w; else if (!(gr == 4 || gr == 7 || gr == 8)) x -= w / 2;
    if (gr == 1 || gr == 5 || gr == 7) y -= h; else if (!(gr == 2 || gr == 6 || gr == 8)) y -= h / 2;
    *ox = x; *oy = y;
}

/* posiciona o popup s e manda xdg_popup.configure + xdg_surface.configure.
 * Tudo precisa caber na janela (o WM so ve uma superficie): espelha os
 * eixos que o cliente permitir e, se ainda nao couber, desliza. */
static void place_popup(Obj* s, const i32* p) {
    i32 w = p[0] > 0 ? p[0] : 1, h = p[1] > 0 ? p[1] : 1;
    i32 adj = p[8];
    Obj* par = surf(s->parent);
    i32 px0 = 0, py0 = 0;                          /* origem da geometria do pai na janela */
    if (par) { abs_pos(par, &px0, &py0); px0 += par->gw ? par->gx : 0; py0 += par->gw ? par->gy : 0; }
    i32 x, y;
    popup_xy(p, 0, 0, w, h, &x, &y);
    if ((adj & 4) && (px0 + x < 0 || px0 + x + w > app.w)) {
        i32 x2, y2; popup_xy(p, 1, 0, w, h, &x2, &y2);
        if (px0 + x2 >= 0 && px0 + x2 + w <= app.w) x = x2;
    }
    if ((adj & 8) && (py0 + y < 0 || py0 + y + h > app.h)) {
        i32 x2, y2; popup_xy(p, 0, 1, w, h, &x2, &y2);
        if (py0 + y2 >= 0 && py0 + y2 + h <= app.h) y = y2;
    }
    if (px0 + x + w > app.w) x = app.w - w - px0;
    if (px0 + x < 0) x = -px0;
    if (py0 + y + h > app.h) y = app.h - h - py0;
    if (py0 + y < 0) y = -py0;
    s->px = x; s->py = y;
    Ev e = ev_begin(s->roleobj, 0); put_u32((u32)x); put_u32((u32)y); put_u32((u32)w); put_u32((u32)h); ev_end(e);
    e = ev_begin(s->xdg, 0); put_u32(serial++); ev_end(e);
}

/* fecha (popup_done) todos os popups abertos, do mais novo ao mais velho */
static void dismiss_popups(void) {
    for (;;) {
        Obj* best = 0;
        for (int i = 0; i < MAXOBJ; i++) {
            Obj* s = &objs[i];
            if (s->type == T_SURFACE && s->role == R_POPUP && s->roleobj && (!best || s->z > best->z)) best = s;
        }
        if (!best) break;
        Ev e = ev_begin(best->roleobj, 1); ev_end(e);
        if (best->mapped && visible(best)) { i32 ax, ay; abs_pos(best, &ax, &ay); win_damage(ax, ay, best->sw, best->sh); }
        best->role = R_NONE; best->mapped = 0; best->grab = 0;  /* o cliente ainda destroi o objeto */
    }
    kb_refocus();
}

/* ------------------------------------------------------------ pedidos */

static void destroy(Obj* o) {
    if (o->type == T_POOL) {
        o->dead = 1;
        if (o->refs > 0) { delete_id(o->id); o->id = 0; return; }   /* memoria segue viva pelos buffers */
        release_pool(o);
    }
    if (o->type == T_BUFFER) {
        Obj* p = (Obj*)o->base;
        if (p && p->type == T_POOL && --p->refs <= 0 && p->dead) { release_pool(p); p->type = T_FREE; }
    }
    if (o->type == T_SURFACE) {
        if (o->mapped && visible(o) && o->role != R_TOPLEVEL) { i32 ax, ay; abs_pos(o, &ax, &ay); win_damage(ax, ay, o->sw, o->sh); }
        free(o->st);
        o->st = 0;
    }
    if (o->type == T_POPUP || o->type == T_SUBSURF) {
        Obj* s = surf(o->ref);
        if (s && s->roleobj == o->id) {
            if (s->mapped && visible(s)) { i32 ax, ay; abs_pos(s, &ax, &ay); win_damage(ax, ay, s->sw, s->sh); }
            s->role = R_NONE; s->mapped = 0; s->roleobj = 0; s->grab = 0;
        }
        if (o->type == T_POPUP) kb_refocus();
    }
    u32 id = o->id;
    o->type = T_FREE;
    delete_id(id);
}

static void request(Obj* o, u32 op, Args* a) {
    switch (o->type) {
    case T_DISPLAY:
        if (op == 0) {                                     /* sync */
            u32 cb = a_u32(a);
            Ev e = ev_begin(cb, 0); put_u32(serial++); ev_end(e);
            delete_id(cb);
        } else if (op == 1) {                              /* get_registry */
            u32 id = a_u32(a);
            add(id, T_REGISTRY, 1);
            for (u32 i = 0; i < NGLOBALS; i++) {
                Ev e = ev_begin(id, 0); put_u32(i + 1); put_str(GLOBALS[i].name); put_u32(GLOBALS[i].version); ev_end(e);
            }
        } else protocol_error(1, 1, "wl_display");
        break;
    case T_REGISTRY:
        if (op == 0) { u32 name = a_u32(a); const char* ifc = a_str(a); u32 ver = a_u32(a); bind_global(name, ifc, ver, a_u32(a)); }
        break;
    case T_COMPOSITOR:
        if (op == 0) add(a_u32(a), T_SURFACE, o->version);
        else if (op == 1) add(a_u32(a), T_REGION, 1);
        break;
    case T_REGION:
        if (op == 0) destroy(o);
        break;
    case T_SHM:
        if (op == 0) {                                     /* create_pool(id, fd, size) */
            u32 id = a_u32(a);
            i32 fd = take_fd();
            i32 size = (i32)a_u32(a);
            Obj* p = add(id, T_POOL, 1);
            if (!p || size <= 0) protocol_error(o->id, 0, "pool invalido");
            p->fd = fd;
            p->size = (u64)size;
            PStat st; sys_fstat(fd, &st);
            p->shm = st.dev == 3;                          /* memfd/shm: memoria realmente compartilhada */
            i64 b = sys_mmap_fd(0, p->size, PROT_READ | PROT_WRITE, p->shm ? P_MAP_SHARED : P_MAP_PRIVATE, fd, 0);
            if (b < 0) protocol_error(o->id, 2, "mmap do pool falhou");
            p->base = (u64)b;
        }
        break;
    case T_POOL:
        if (op == 0) {                                     /* create_buffer */
            u32 id = a_u32(a);
            Obj* b = add(id, T_BUFFER, 1);
            if (!b) protocol_error(1, 2, "objetos demais");
            b->ref = o->id; b->offset = a_u32(a); b->w = a_u32(a); b->h = a_u32(a); b->stride = a_u32(a);
            b->fmt = a_u32(a);
            b->base = (u64)o;                              /* o pool vive enquanto houver buffers */
            o->refs++;
            if (b->fmt > 1 || b->stride < b->w * 4 || b->w > 8192 || b->h > 8192)
                protocol_error(o->id, 0, "formato ou stride invalido");
        } else if (op == 1) destroy(o);
        else if (op == 2) {                                /* resize */
            u64 ns = a_u32(a);
            if (ns > o->size) {
                sys_munmap(o->base, o->size);
                i64 b = sys_mmap_fd(0, ns, PROT_READ | PROT_WRITE, o->shm ? P_MAP_SHARED : P_MAP_PRIVATE, o->fd, 0);
                if (b < 0) protocol_error(o->id, 2, "mmap do pool falhou");
                o->base = (u64)b; o->size = ns;
            }
        }
        break;
    case T_BUFFER:
        if (op == 0) destroy(o);
        break;
    case T_SURFACE:
        switch (op) {
        case 0: destroy(o); break;
        case 1: o->ref = a_u32(a); o->attached = 1; break; /* attach(buffer, x, y) */
        case 2: case 9: {                                  /* damage / damage_buffer (escala 1) */
            i32 x = a_i32(a), y = a_i32(a), w = a_i32(a), h = a_i32(a);
            if (w <= 0 || h <= 0) break;
            if (w > 16384) w = 16384;                      /* INT32_MAX = "tudo" */
            if (h > 16384) h = 16384;
            if (x < -16384) x = -16384; else if (x > 16384) x = 16384;
            if (y < -16384) y = -16384; else if (y > 16384) y = 16384;
            if (o->dx1 <= o->dx0) { o->dx0 = x; o->dy0 = y; o->dx1 = x + w; o->dy1 = y + h; }
            else {
                if (x < o->dx0) o->dx0 = x;
                if (y < o->dy0) o->dy0 = y;
                if (x + w > o->dx1) o->dx1 = x + w;
                if (y + h > o->dy1) o->dy1 = y + h;
            }
            break;
        }
        case 3: {                                          /* frame */
            u32 cb = a_u32(a);
            add(cb, T_CALLBACK, 1);
            if (o->ncb < 16) o->cbs[o->ncb++] = cb;
            break;
        }
        case 6: surface_commit(o); break;                  /* commit */
        default: break;                                    /* regioes, escala, transformacao: ignorados */
        }
        break;
    case T_SUBCOMP:
        if (op == 0) destroy(o);
        else if (op == 1) {                                /* get_subsurface(id, surface, parent) */
            u32 id = a_u32(a);
            Obj* s = surf(a_u32(a));
            Obj* p = surf(a_u32(a));
            if (!s || !p || s == p || s->role != R_NONE) protocol_error(o->id, 0, "subsuperficie invalida");
            add(id, T_SUBSURF, 1)->ref = s->id;
            s->role = R_SUB; s->parent = p->id; s->roleobj = id; s->z = zcount++; s->px = s->py = 0;
        }
        break;
    case T_SUBSURF: {
        Obj* s = surf(o->ref);
        if (op == 0) destroy(o);
        else if (op == 1 && s) {                           /* set_position */
            i32 x = a_i32(a), y = a_i32(a);
            if (s->mapped && visible(s)) { i32 ax, ay; abs_pos(s, &ax, &ay); win_damage(ax, ay, s->sw, s->sh); }
            s->px = x; s->py = y;
            if (s->mapped && visible(s)) { i32 ax, ay; abs_pos(s, &ax, &ay); win_damage(ax, ay, s->sw, s->sh); }
        } else if ((op == 2 || op == 3) && s) {            /* place_above / place_below */
            Obj* sib = surf(a_u32(a));
            if (sib && sib != s) s->z = op == 2 ? sib->z + 1 : sib->z - 1;
            if (op == 3 && sib && sib->id == s->parent) s->z = -0x7ffffff0; /* abaixo do pai: primeiro */
            if (s->mapped && visible(s)) { i32 ax, ay; abs_pos(s, &ax, &ay); win_damage(ax, ay, s->sw, s->sh); }
        }                                                  /* set_sync/set_desync: tudo e aplicado no commit */
        break;
    }
    case T_WMBASE:
        if (op == 0) destroy(o);
        else if (op == 1) add(a_u32(a), T_POSITIONER, 1);
        else if (op == 2) {                                /* get_xdg_surface(id, surface) */
            u32 id = a_u32(a);
            Obj* x = add(id, T_XDGSURF, o->version);
            Obj* s = surf(a_u32(a));
            if (!x || !s) protocol_error(o->id, 0, "xdg_surface invalida");
            x->ref = s->id; s->xdg = id;
        } else if (op == 3) { a_u32(a); }                  /* pong */
        break;
    case T_POSITIONER:
        if (op == 0) destroy(o);
        else if (op == 1) { o->pos[0] = a_i32(a); o->pos[1] = a_i32(a); }
        else if (op == 2) { o->pos[2] = a_i32(a); o->pos[3] = a_i32(a); o->pos[4] = a_i32(a); o->pos[5] = a_i32(a); }
        else if (op == 3) o->pos[6] = a_i32(a);
        else if (op == 4) o->pos[7] = a_i32(a);
        else if (op == 5) o->pos[8] = a_i32(a);
        else if (op == 6) { o->pos[9] = a_i32(a); o->pos[10] = a_i32(a); }
        break;
    case T_XDGSURF: {
        Obj* s = surf(o->ref);
        if (op == 1) {                                     /* get_toplevel */
            u32 id = a_u32(a);
            add(id, T_TOPLEVEL, o->version)->ref = o->ref;
            if (!top_level && s) {
                top_level = id; top_xdg = o->id; top_surface = s->id;
                s->role = R_TOPLEVEL; s->roleobj = id;
                cfg_w = 0; cfg_h = 0;                      /* o cliente escolhe o tamanho */
                send_configure();
            }
        } else if (op == 2) {                              /* get_popup(id, parent, positioner) */
            u32 id = a_u32(a);
            Obj* px = find(a_u32(a));
            Obj* pos = find(a_u32(a));
            if (!s || !pos || pos->type != T_POSITIONER || s->role != R_NONE) protocol_error(o->id, 0, "popup invalido");
            Obj* p = add(id, T_POPUP, o->version);
            if (!p) protocol_error(1, 2, "objetos demais");
            p->ref = s->id;
            s->role = R_POPUP; s->roleobj = id; s->z = zcount++; s->mapped = 0;
            s->parent = px && px->type == T_XDGSURF ? px->ref : top_surface;
            place_popup(s, pos->pos);
        } else if (op == 3 && s) {                         /* set_window_geometry */
            i32 x = a_i32(a), y = a_i32(a), w = a_i32(a), h = a_i32(a);
            if (w > 0 && h > 0) { s->gx = x; s->gy = y; s->gw = w; s->gh = h; }
        } else if (op == 0) destroy(o);
        break;
    }
    case T_POPUP: {
        Obj* s = surf(o->ref);
        if (op == 0) destroy(o);
        else if (op == 1 && s) { a_u32(a); a_u32(a); s->grab = 1; kb_refocus(); }   /* grab(seat, serial) */
        break;
    }
    case T_TOPLEVEL:
        if (op == 2) {                                     /* set_title */
            const char* t = a_str(a);
            strncpy(title, t, sizeof title - 1);
            if (have_app) app_title(&app, title);
        } else if (op == 0) destroy(o);
        break;
    case T_SEAT:
        if (op == 0) { pointer_id = a_u32(a); add(pointer_id, T_POINTER, o->version); }
        else if (op == 1) {
            keyboard_id = a_u32(a); kb_version = o->version;
            add(keyboard_id, T_KEYBOARD, o->version);
            send_keymap();
            if (kb_version >= 4) { Ev e = ev_begin(keyboard_id, 5); put_u32(25); put_u32(600); ev_end(e); }
        }
        break;
    case T_POINTER:                                        /* 0 = set_cursor (cursor do sistema), 1 = release */
        if (op == 1) destroy(o);
        break;
    case T_KEYBOARD:                                       /* 0 = release */
        if (op == 0) destroy(o);
        break;
    case T_OUTPUT: case T_CALLBACK:
        break;
    }
}

static void process(void) {
    u32 pos = 0;
    while (inlen - pos >= 8) {
        u32 id = *(u32*)(in + pos);
        u32 w = *(u32*)(in + pos + 4);
        u32 size = w >> 16, op = w & 0xFFFF;
        if (size < 8 || size % 4) protocol_error(1, 1, "tamanho invalido");
        if (inlen - pos < size) break;
        Obj* o = find(id);
        if (!o) protocol_error(1, 0, "objeto inexistente");
        Args a = { in + pos + 8, in + pos + size };
        request(o, op, &a);
        ensure_room();
        pos += size;
    }
    memmove(in, in + pos, inlen - pos);
    inlen -= pos;
}

/* ------------------------------------------------------------ eventos do WM */

static int win_focused;
static u32 kb_surf;            /* superficie com o foco do teclado (0 = nenhuma) */
static u32 ptr_surf;           /* superficie sob o ponteiro (0 = fora) */
static int last_down, swallow_up;
static i32 last_x = -1, last_y = -1;

static void pump_wm(void) {
    for (int guard = 0; guard < 16; guard++) {
        WmEvent e = app_peek(&app);
        handle_wm(e);
        if (!e.more) break;
    }
}

/* teclado: o popup agarrado mais novo, senao a janela (se focada) */
static void kb_refocus(void) {
    if (!keyboard_id) return;
    u32 want = 0;
    if (win_focused) {
        want = top_surface;
        i32 bz = -0x7fffffff;
        for (int i = 0; i < MAXOBJ; i++) {
            Obj* s = &objs[i];
            if (s->type == T_SURFACE && s->role == R_POPUP && s->grab && s->z > bz) { bz = s->z; want = s->id; }
        }
    }
    if (want == kb_surf) return;
    if (kb_surf && surf(kb_surf)) { Ev v = ev_begin(keyboard_id, 2); put_u32(serial++); put_u32(kb_surf); ev_end(v); }
    kb_surf = want;
    if (kb_surf) { Ev v = ev_begin(keyboard_id, 1); put_u32(serial++); put_u32(kb_surf); put_array(0, 0); ev_end(v); }
}

/* superficie mais alta sob (x, y) da janela; devolve coordenadas locais */
static Obj* hit_tree(u32 parent, i32 x, i32 y, i32* lx, i32* ly) {
    /* subsuperficies, do z mais alto ao mais baixo */
    i32 last = 0x7fffffff;
    for (;;) {
        Obj* best = 0;
        for (int i = 0; i < MAXOBJ; i++) {
            Obj* s = &objs[i];
            if (s->type != T_SURFACE || s->role != R_SUB || s->parent != parent || !s->mapped) continue;
            if (s->z < last && (!best || s->z > best->z)) best = s;
        }
        if (!best) break;
        last = best->z;
        Obj* r = hit_tree(best->id, x, y, lx, ly);
        if (r) return r;
        i32 ax, ay; abs_pos(best, &ax, &ay);
        if (x >= ax && y >= ay && x < ax + best->sw && y < ay + best->sh) { *lx = x - ax; *ly = y - ay; return best; }
    }
    return 0;
}

static Obj* hit(i32 x, i32 y, i32* lx, i32* ly) {
    /* popups (os mais novos primeiro); acerta a geometria visivel */
    i32 last = 0x7fffffff;
    for (;;) {
        Obj* best = 0;
        for (int i = 0; i < MAXOBJ; i++) {
            Obj* s = &objs[i];
            if (s->type != T_SURFACE || s->role != R_POPUP || !visible(s)) continue;
            if (s->z < last && (!best || s->z > best->z)) best = s;
        }
        if (!best) break;
        last = best->z;
        Obj* r = hit_tree(best->id, x, y, lx, ly);
        if (r) return r;
        i32 ax, ay; abs_pos(best, &ax, &ay);
        i32 gx = best->gw ? best->gx : 0, gy = best->gw ? best->gy : 0;
        i32 gw = best->gw ? best->gw : best->sw, gh = best->gw ? best->gh : best->sh;
        if (x >= ax + gx && y >= ay + gy && x < ax + gx + gw && y < ay + gy + gh) { *lx = x - ax; *ly = y - ay; return best; }
    }
    Obj* r = hit_tree(top_surface, x, y, lx, ly);
    if (r) return r;
    if (x >= 0 && y >= 0 && x < app.w && y < app.h) {
        Obj* t = surf(top_surface);
        if (!t) return 0;
        i32 ax, ay; abs_pos(t, &ax, &ay);
        *lx = x - ax; *ly = y - ay;
        return t;
    }
    return 0;
}

static int any_grab(void) {
    for (int i = 0; i < MAXOBJ; i++)
        if (objs[i].type == T_SURFACE && objs[i].role == R_POPUP && objs[i].grab) return 1;
    return 0;
}

static void ptr_button(u32 now, u32 button, int pressed) {
    Ev v = ev_begin(pointer_id, 3); put_u32(serial++); put_u32(now); put_u32(button); put_u32(pressed ? 1 : 0); ev_end(v);
}

/* traduz um evento do WM para eventos Wayland */
static void handle_wm(WmEvent e) {
    u32 now = (u32)sys_time_ms();
    if (e.close && top_level) { Ev v = ev_begin(top_level, 1); ev_end(v); }
    if (app.w != cfg_w || app.h != cfg_h) { cfg_w = app.w; cfg_h = app.h; send_configure(); }
    if (e.focused != win_focused) {
        win_focused = e.focused;
        if (!win_focused && any_grab()) dismiss_popups();  /* outra janela focada: menus fecham */
        kb_refocus();
    }
    if (pointer_id) {
        int sent = 0;
        i32 lx = 0, ly = 0;
        Obj* t = (e.mx >= 0 && e.my >= 0 && e.mx < app.w && e.my < app.h) ? hit(e.mx, e.my, &lx, &ly) : 0;
        u32 tid = t ? t->id : 0;
        if (ptr_surf && !surf(ptr_surf)) ptr_surf = 0;     /* a superficie foi destruida */
        if (tid != ptr_surf) {
            if (ptr_surf) { Ev v = ev_begin(pointer_id, 1); put_u32(serial++); put_u32(ptr_surf); ev_end(v); }
            if (tid) { Ev v = ev_begin(pointer_id, 0); put_u32(serial++); put_u32(tid); put_u32((u32)(lx * 256)); put_u32((u32)(ly * 256)); ev_end(v); }
            ptr_surf = tid; sent = 1;
        } else if (tid && (e.mx != last_x || e.my != last_y)) {
            Ev v = ev_begin(pointer_id, 2); put_u32(now); put_u32((u32)(lx * 256)); put_u32((u32)(ly * 256)); ev_end(v);
            sent = 1;
        }
        last_x = e.mx; last_y = e.my;
        /* clique fora dos popups agarrados: fecham, e o clique e consumido */
        int press = (e.down && !last_down) || e.click || e.rclick;
        Obj* ts = surf(tid);
        if (press && any_grab() && (!ts || ts->role != R_POPUP)) {
            dismiss_popups();
            swallow_up = e.down;                           /* o soltar deste clique tambem nao vai */
            last_down = e.down;
            if (sent) { Obj* p = find(pointer_id); if (p && p->version >= 5) { Ev v = ev_begin(pointer_id, 5); ev_end(v); } }
            return;
        }
        if (ptr_surf) {
            if (e.down != last_down) {
                if (!(swallow_up && !e.down)) { ptr_button(now, 0x110, e.down); sent = 1; }
                swallow_up = 0;
            }
            else if (e.click) { ptr_button(now, 0x110, 1); ptr_button(now, 0x110, 0); sent = 1; }  /* clique curto entre duas consultas */
            if (e.rclick) { ptr_button(now, 0x111, 1); ptr_button(now, 0x111, 0); sent = 1; }
            if (e.wheel) {
                Ev v = ev_begin(pointer_id, 4); put_u32(now); put_u32(0); put_u32((u32)(-e.wheel * 10 * 256)); ev_end(v);
                sent = 1;
            }
        }
        last_down = e.down;
        if (sent) {
            Obj* p = find(pointer_id);
            if (p && p->version >= 5) { Ev v = ev_begin(pointer_id, 5); ev_end(v); }
        }
    }
    if (keyboard_id && e.key) {
        if (!kb_surf) { win_focused = 1; kb_refocus(); }
        if (kb_surf) {
            send_mods(e.mods);
            Ev v = ev_begin(keyboard_id, 3); put_u32(serial++); put_u32(now); put_u32(e.key); put_u32(1); ev_end(v);
            v = ev_begin(keyboard_id, 3); put_u32(serial++); put_u32(now); put_u32(e.key); put_u32(0); ev_end(v);
        }
    }
    ensure_room();
}

int main(void) {
    add(1, T_DISPLAY, 1);
    wd0 = wd1 = 0; wd2 = wd3 = -1;
    for (;;) {
        PPollFd pf = { CONN, P_POLLIN, 0 };
        PTimespec ts = { 0, have_app ? 8000000 : 200000000 };
        sys_ppoll(&pf, 1, &ts);
        if (pf.revents & (P_POLLIN | P_POLLHUP | P_POLLERR)) {
            if (!read_client()) {                          /* o programa terminou */
                if (have_app) app_bye(&app);
                break;
            }
        }
        process();
        present();
        if (have_app) pump_wm();
        present();                                         /* dano gerado pelos eventos (popups fechados) */
        /* o usuario trocou o layout nas Configuracoes: manda o mapa novo */
        if (keyboard_id && sys_time_ms() - layout_checked > 2000) {
            layout_checked = sys_time_ms();
            if (read_layout()) send_keymap();
        }
        flush();
    }
    return 0;
}
