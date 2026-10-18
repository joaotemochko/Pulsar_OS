/* fonttest — fontes do sistema pela pilha do Linux: fontconfig escolhe as
 * fontes (familias genericas, nomes do Windows -> Liberation, reserva por cobertura de
 * caracteres) e o FreeType rasteriza; o texto aparece numa janela Wayland.
 *
 *   fonttest        mostra a janela por ~10 s, imprime o resultado e sai
 *   fonttest loop   fica aberto ate fechar a janela
 *
 * Termina com "fim: ok" se cada pedido caiu na familia esperada e cada
 * linha teve pixels desenhados. */
#define _GNU_SOURCE
#include <fontconfig/fontconfig.h>
#include <ft2build.h>
#include FT_FREETYPE_H
#include <poll.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <time.h>
#include <unistd.h>
#include <wayland-client.h>
#include "xdg-shell-client-protocol.h"

#define W 560
#define H 300

static long ms_now(void) {
    struct timespec t; clock_gettime(CLOCK_MONOTONIC, &t);
    return t.tv_sec * 1000 + t.tv_nsec / 1000000;
}

/* fontconfig: melhor fonte para a familia (sem o parser de nomes, em que
 * '-' separa o tamanho: "sans-serif" viraria "sans") */
static FcPattern* match(const char* family, int bold, FcCharSet* need) {
    FcPattern* p = FcPatternCreate();
    if (family) FcPatternAddString(p, FC_FAMILY, (const FcChar8*)family);
    if (bold) FcPatternAddInteger(p, FC_WEIGHT, FC_WEIGHT_BOLD);
    if (need) FcPatternAddCharSet(p, FC_CHARSET, need);
    FcConfigSubstitute(NULL, p, FcMatchPattern);
    FcDefaultSubstitute(p);
    FcResult r;
    FcPattern* m = FcFontMatch(NULL, p, &r);
    FcPatternDestroy(p);
    return m;
}

static int checks_ok = 1;

static FcPattern* expect(const char* family, int bold, FcCharSet* need, const char* want) {
    FcPattern* m = match(family, bold, need);
    FcChar8 *fam = (FcChar8*)"?", *file = (FcChar8*)"?";
    if (m) { FcPatternGetString(m, FC_FAMILY, 0, &fam); FcPatternGetString(m, FC_FILE, 0, &file); }
    int ok = m && !strcmp((const char*)fam, want);
    if (!ok) checks_ok = 0;
    printf("[fonttest] %-22s -> %-20s %s %s\n", family ? family : "(U+2713)", fam, ok ? "ok" : "ESPERADO:", ok ? (const char*)file : want);
    return m;
}

/* ---- texto com FreeType ---- */
static uint32_t* px;

static int utf8(const char** s) {
    const unsigned char* p = (const unsigned char*)*s;
    int c = *p++;
    if (c >= 0xF0) { c = (c & 7) << 18 | (p[0] & 63) << 12 | (p[1] & 63) << 6 | (p[2] & 63); p += 3; }
    else if (c >= 0xE0) { c = (c & 15) << 12 | (p[0] & 63) << 6 | (p[1] & 63); p += 2; }
    else if (c >= 0xC0) { c = (c & 31) << 6 | (p[0] & 63); p += 1; }
    *s = (const char*)p;
    return c;
}

/* desenha `text` na linha de base y; caracteres ausentes na fonte vem da
 * `fallback`. Devolve quantos pixels receberam tinta. */
static long draw_text(FT_Face face, FT_Face fallback, int x, int y, const char* text, uint32_t color) {
    long ink = 0;
    while (*text) {
        int c = utf8(&text);
        FT_Face f = face;
        FT_UInt gi = FT_Get_Char_Index(f, c);
        if (!gi && fallback) { f = fallback; gi = FT_Get_Char_Index(f, c); }
        if (FT_Load_Glyph(f, gi, FT_LOAD_RENDER)) continue;
        FT_GlyphSlot g = f->glyph;
        for (unsigned r = 0; r < g->bitmap.rows; r++)
            for (unsigned k = 0; k < g->bitmap.width; k++) {
                int a = g->bitmap.buffer[r * g->bitmap.pitch + k];
                int X = x + g->bitmap_left + (int)k, Y = y - g->bitmap_top + (int)r;
                if (!a || X < 0 || Y < 0 || X >= W || Y >= H) continue;
                uint32_t d = px[Y * W + X];
                uint32_t o = 0xFF000000;
                for (int sh = 0; sh < 24; sh += 8) {
                    int dc = d >> sh & 255, sc = color >> sh & 255;
                    o |= (uint32_t)((sc * a + dc * (255 - a)) / 255) << sh;
                }
                px[Y * W + X] = o;
                ink++;
            }
        x += g->advance.x >> 6;
    }
    return ink;
}

static FT_Face open_face(FT_Library lib, FcPattern* m, int size) {
    FcChar8* file; int idx = 0;
    if (!m || FcPatternGetString(m, FC_FILE, 0, &file) != FcResultMatch) return NULL;
    FcPatternGetInteger(m, FC_INDEX, 0, &idx);
    FT_Face f;
    if (FT_New_Face(lib, (const char*)file, idx, &f)) return NULL;
    FT_Set_Pixel_Sizes(f, 0, size);
    return f;
}

/* ---- Wayland ---- */
static struct wl_compositor* compositor;
static struct wl_shm* shm;
static struct xdg_wm_base* wm;
static int configured, closed;
static void wm_ping(void* d, struct xdg_wm_base* b, uint32_t s) { xdg_wm_base_pong(b, s); }
static const struct xdg_wm_base_listener wm_l = { wm_ping };
static void xs_configure(void* d, struct xdg_surface* x, uint32_t s) { xdg_surface_ack_configure(x, s); configured = 1; }
static const struct xdg_surface_listener xs_l = { xs_configure };
static void tl_configure(void* d, struct xdg_toplevel* t, int32_t w, int32_t h, struct wl_array* st) {}
static void tl_close(void* d, struct xdg_toplevel* t) { closed = 1; }
static const struct xdg_toplevel_listener tl_l = { tl_configure, tl_close };
static void reg_global(void* d, struct wl_registry* r, uint32_t n, const char* i, uint32_t v) {
    if (!strcmp(i, "wl_compositor")) compositor = wl_registry_bind(r, n, &wl_compositor_interface, 4);
    else if (!strcmp(i, "wl_shm")) shm = wl_registry_bind(r, n, &wl_shm_interface, 1);
    else if (!strcmp(i, "xdg_wm_base")) { wm = wl_registry_bind(r, n, &xdg_wm_base_interface, 2); xdg_wm_base_add_listener(wm, &wm_l, 0); }
}
static void reg_remove(void* d, struct wl_registry* r, uint32_t n) {}
static const struct wl_registry_listener reg_l = { reg_global, reg_remove };

int main(int argc, char** argv) {
    setvbuf(stdout, NULL, _IONBF, 0);
    int loop = argc > 1 && !strcmp(argv[1], "loop");

    long t0 = ms_now();
    if (!FcInit()) { printf("[fonttest] FcInit falhou\n[fonttest] fim: FALHA\n"); return 1; }
    FcFontSet* all = FcConfigGetFonts(NULL, FcSetSystem);
    printf("[fonttest] fontconfig %d: %d fontes em %ld ms\n", FcGetVersion(), all ? all->nfont : 0, ms_now() - t0);

    FcPattern* sans = expect("sans-serif", 0, NULL, "Noto Sans");
    FcPattern* serif = expect("serif", 0, NULL, "Noto Serif");
    FcPattern* mono = expect("monospace", 0, NULL, "Noto Sans Mono");
    FcPattern* bold = expect("sans-serif", 1, NULL, "Noto Sans");
    FcPattern* arial = expect("Arial", 0, NULL, "Liberation Sans");
    FcPattern* times = expect("Times New Roman", 0, NULL, "Liberation Serif");
    expect("Courier New", 0, NULL, "Liberation Mono");
    FcCharSet* cs = FcCharSetCreate();
    FcCharSetAddChar(cs, 0x2713);                    /* ✓: a Noto Sans nao tem, a DejaVu Sans sim */
    FcPattern* sym = expect(NULL, 0, cs, "DejaVu Sans");

    FT_Library lib;
    if (FT_Init_FreeType(&lib)) { printf("[fonttest] FreeType falhou\n[fonttest] fim: FALHA\n"); return 1; }
    int maj, min, pat; FT_Library_Version(lib, &maj, &min, &pat);
    FT_Face f_sans = open_face(lib, sans, 26), f_serif = open_face(lib, serif, 24), f_mono = open_face(lib, mono, 18);
    FT_Face f_bold = open_face(lib, bold, 18), f_arial = open_face(lib, arial, 18), f_times = open_face(lib, times, 18);
    FT_Face f_sym = open_face(lib, sym, 26);
    if (!f_sans || !f_serif || !f_mono || !f_bold || !f_arial || !f_times || !f_sym) {
        printf("[fonttest] FT_New_Face falhou\n[fonttest] fim: FALHA\n"); return 1;
    }

    px = calloc(W * H, 4);
    for (int i = 0; i < W * H; i++) px[i] = 0xFFFFFFFF;
    long t1 = ms_now();
    long ink[6];
    ink[0] = draw_text(f_sans, f_sym, 20, 50, "Olá, Pulsar! Ação ç ñ € ✓", 0x1D1D1F);
    ink[1] = draw_text(f_serif, f_sym, 20, 95, "Noto Serif — tipografia clássica", 0x0A4A8A);
    ink[2] = draw_text(f_bold, NULL, 20, 135, "Noto Sans Bold (fontconfig: weight=bold)", 0x1D1D1F);
    ink[3] = draw_text(f_arial, NULL, 20, 170, "Arial -> Liberation Sans (mesmas métricas)", 0x3A3A3C);
    ink[4] = draw_text(f_times, NULL, 20, 205, "Times New Roman -> Liberation Serif", 0x3A3A3C);
    char info[128];
    snprintf(info, sizeof info, "FreeType %d.%d.%d + fontconfig %d.%d.%d", maj, min, pat,
             FcGetVersion() / 10000, FcGetVersion() / 100 % 100, FcGetVersion() % 100);
    ink[5] = draw_text(f_mono, NULL, 20, 250, info, 0x1E8E3E);
    int inked = 1;
    for (int i = 0; i < 6; i++) if (ink[i] < 200) inked = 0;
    printf("[fonttest] %s; texto rasterizado em %ld ms, tinta por linha %ld %ld %ld %ld %ld %ld\n",
           info, ms_now() - t1, ink[0], ink[1], ink[2], ink[3], ink[4], ink[5]);

    /* janela */
    struct wl_display* dpy = NULL;
    for (int i = 0; i < 100 && !(dpy = wl_display_connect(NULL)); i++) usleep(100000);
    int shown = 0;
    if (dpy) {
        struct wl_registry* reg = wl_display_get_registry(dpy);
        wl_registry_add_listener(reg, &reg_l, 0);
        wl_display_roundtrip(dpy);
        struct wl_surface* s = wl_compositor_create_surface(compositor);
        struct xdg_surface* xs = xdg_wm_base_get_xdg_surface(wm, s);
        xdg_surface_add_listener(xs, &xs_l, 0);
        struct xdg_toplevel* tl = xdg_surface_get_toplevel(xs);
        xdg_toplevel_add_listener(tl, &tl_l, 0);
        xdg_toplevel_set_title(tl, "Fontes: fontconfig + FreeType");
        wl_surface_commit(s);
        while (!configured && wl_display_dispatch(dpy) >= 0) {}
        int fd = memfd_create("fonttest", MFD_CLOEXEC);
        ftruncate(fd, W * H * 4);
        uint32_t* buf = mmap(NULL, W * H * 4, PROT_READ | PROT_WRITE, MAP_SHARED, fd, 0);
        memcpy(buf, px, W * H * 4);
        struct wl_shm_pool* pool = wl_shm_create_pool(shm, fd, W * H * 4);
        struct wl_buffer* b = wl_shm_pool_create_buffer(pool, 0, W, H, W * 4, WL_SHM_FORMAT_XRGB8888);
        wl_shm_pool_destroy(pool);
        close(fd);
        wl_surface_attach(s, b, 0, 0);
        wl_surface_damage(s, 0, 0, W, H);
        wl_surface_commit(s);
        wl_display_flush(dpy);
        shown = 1;
        long until = ms_now() + 10000;
        while (!closed && (loop || ms_now() < until)) {
            wl_display_flush(dpy);
            while (wl_display_prepare_read(dpy) != 0) wl_display_dispatch_pending(dpy);
            struct pollfd pf = { wl_display_get_fd(dpy), POLLIN, 0 };
            if (poll(&pf, 1, 100) > 0) wl_display_read_events(dpy);
            else wl_display_cancel_read(dpy);
            wl_display_dispatch_pending(dpy);
        }
    }
    int ok = checks_ok && inked && shown;
    printf("[fonttest] fim: %s\n", ok ? "ok" : "FALHA");
    return ok ? 0 : 1;
}
