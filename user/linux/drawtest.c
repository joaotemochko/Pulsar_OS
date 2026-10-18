/* drawtest — texto e desenho 2D pela pilha do Linux (a mesma que o GTK usa):
 *
 *   harfbuzz   shaping de verdade: acento combinante composto, kerning (GPOS),
 *              arabe (direita para a esquerda, formas contextuais) e
 *              devanagari (reordenacao da vogal i)
 *   cairo      gradientes, curvas, transparencia, texto pelo fontconfig e os
 *              glifos do harfbuzz; grava PNG (e le de volta) e PDF
 *   pixman     rasteriza tudo por baixo do cairo
 *
 * O desenho aparece numa janela Wayland (~10 s; "drawtest loop" fica aberto).
 * Termina com "fim: ok" se tudo conferiu. */
#define _GNU_SOURCE
#include <cairo.h>
#include <cairo-ft.h>
#include <cairo-pdf.h>
#include <fontconfig/fontconfig.h>
#include <ft2build.h>
#include FT_FREETYPE_H
#include <hb.h>
#include <hb-ft.h>
#include <math.h>
#include <poll.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <sys/stat.h>
#include <time.h>
#include <unistd.h>
#include <wayland-client.h>
#include "xdg-shell-client-protocol.h"

#define W 640
#define H 420

static int falhas;
static void check(int ok, const char* what) {
    printf("[drawtest] %s    %s\n", ok ? "ok  " : "FALHOU", what);
    if (!ok) falhas++;
}
static long ms_now(void) {
    struct timespec t; clock_gettime(CLOCK_MONOTONIC, &t);
    return t.tv_sec * 1000 + t.tv_nsec / 1000000;
}

/* arquivo da fonte que o fontconfig escolhe para a familia */
static char* font_file(const char* family) {
    FcPattern* p = FcPatternCreate();
    FcPatternAddString(p, FC_FAMILY, (const FcChar8*)family);
    FcConfigSubstitute(NULL, p, FcMatchPattern);
    FcDefaultSubstitute(p);
    FcResult r;
    FcPattern* m = FcFontMatch(NULL, p, &r);
    FcChar8* f = NULL;
    char* out = NULL;
    if (m && FcPatternGetString(m, FC_FILE, 0, &f) == FcResultMatch) out = strdup((const char*)f);
    if (m) FcPatternDestroy(m);
    FcPatternDestroy(p);
    return out;
}

typedef struct { FT_Face ft; hb_font_t* hb; cairo_font_face_t* cf; } Font;

static Font open_font(FT_Library lib, const char* family, int px) {
    Font f = { 0 };
    char* file = font_file(family);
    if (!file || FT_New_Face(lib, file, 0, &f.ft)) { printf("[drawtest] fonte %s nao abriu\n", family); return f; }
    FT_Set_Pixel_Sizes(f.ft, 0, px);
    f.hb = hb_ft_font_create_referenced(f.ft);
    f.cf = cairo_ft_font_face_create_for_ft_face(f.ft, 0);
    free(file);
    return f;
}

/* shaping de `text` (direcao, escrita e lingua adivinhadas) */
static hb_buffer_t* shape(Font* f, const char* text) {
    hb_buffer_t* b = hb_buffer_create();
    hb_buffer_add_utf8(b, text, -1, 0, -1);
    hb_buffer_guess_segment_properties(b);
    hb_shape(f->hb, b, NULL, 0);
    return b;
}

/* desenha os glifos de um buffer do harfbuzz com o cairo, a partir de (x, y) */
static double draw_shaped(cairo_t* cr, Font* f, hb_buffer_t* b, double x, double y, int px) {
    unsigned n;
    hb_glyph_info_t* gi = hb_buffer_get_glyph_infos(b, &n);
    hb_glyph_position_t* gp = hb_buffer_get_glyph_positions(b, &n);
    cairo_glyph_t* gl = cairo_glyph_allocate(n);
    double cx = x;
    for (unsigned i = 0; i < n; i++) {
        gl[i].index = gi[i].codepoint;
        gl[i].x = cx + gp[i].x_offset / 64.0;
        gl[i].y = y - gp[i].y_offset / 64.0;
        cx += gp[i].x_advance / 64.0;
    }
    cairo_set_font_face(cr, f->cf);
    cairo_set_font_size(cr, px);
    cairo_show_glyphs(cr, gl, n);
    cairo_glyph_free(gl);
    return cx - x;
}

static void scene(cairo_t* cr, Font* latin, Font* arab, Font* deva, hb_buffer_t** lines) {
    /* fundo em gradiente */
    cairo_pattern_t* bg = cairo_pattern_create_linear(0, 0, W, H);
    cairo_pattern_add_color_stop_rgb(bg, 0, 0.16, 0.35, 0.80);
    cairo_pattern_add_color_stop_rgb(bg, 1, 0.45, 0.25, 0.70);
    cairo_set_source(cr, bg);
    cairo_paint(cr);
    cairo_pattern_destroy(bg);
    /* painel arredondado semitransparente */
    double x = 24, y = 24, w = W - 48, h = H - 48, r = 18;
    cairo_new_sub_path(cr);
    cairo_arc(cr, x + w - r, y + r, r, -M_PI / 2, 0);
    cairo_arc(cr, x + w - r, y + h - r, r, 0, M_PI / 2);
    cairo_arc(cr, x + r, y + h - r, r, M_PI / 2, M_PI);
    cairo_arc(cr, x + r, y + r, r, M_PI, 3 * M_PI / 2);
    cairo_close_path(cr);
    cairo_set_source_rgba(cr, 1, 1, 1, 0.88);
    cairo_fill(cr);
    /* circulo com gradiente radial e curva de Bezier */
    cairo_pattern_t* rg = cairo_pattern_create_radial(540, 110, 6, 545, 118, 52);
    cairo_pattern_add_color_stop_rgb(rg, 0, 1.0, 0.85, 0.40);
    cairo_pattern_add_color_stop_rgb(rg, 1, 0.95, 0.40, 0.15);
    cairo_set_source(cr, rg);
    cairo_arc(cr, 545, 118, 50, 0, 2 * M_PI);
    cairo_fill(cr);
    cairo_pattern_destroy(rg);
    cairo_set_line_width(cr, 5);
    cairo_set_line_cap(cr, CAIRO_LINE_CAP_ROUND);
    cairo_set_source_rgba(cr, 0.10, 0.55, 0.30, 0.8);
    cairo_move_to(cr, 60, 360);
    cairo_curve_to(cr, 200, 290, 330, 400, 590, 330);
    cairo_stroke(cr);
    /* texto pela API simples do cairo (fontconfig escolhe "sans-serif") */
    cairo_select_font_face(cr, "sans-serif", CAIRO_FONT_SLANT_NORMAL, CAIRO_FONT_WEIGHT_BOLD);
    cairo_set_font_size(cr, 26);
    cairo_set_source_rgb(cr, 0.11, 0.11, 0.12);
    cairo_move_to(cr, 48, 74);
    cairo_show_text(cr, "cairo + harfbuzz no Pulsar");
    /* linhas com shaping do harfbuzz */
    cairo_set_source_rgb(cr, 0.15, 0.15, 0.17);
    draw_shaped(cr, latin, lines[0], 48, 122, 24);
    draw_shaped(cr, latin, lines[1], 48, 160, 24);
    cairo_set_source_rgb(cr, 0.05, 0.35, 0.60);
    double aw = draw_shaped(cr, arab, lines[2], 0, -1000, 30);  /* mede (fora da tela) */
    draw_shaped(cr, arab, lines[2], W - 48 - aw, 220, 30);     /* alinhado a direita */
    cairo_set_source_rgb(cr, 0.55, 0.20, 0.10);
    draw_shaped(cr, deva, lines[3], 48, 280, 30);
    cairo_select_font_face(cr, "monospace", CAIRO_FONT_SLANT_NORMAL, CAIRO_FONT_WEIGHT_NORMAL);
    cairo_set_font_size(cr, 14);
    cairo_set_source_rgb(cr, 0.12, 0.45, 0.25);
    char info[160];
    snprintf(info, sizeof info, "cairo %s, harfbuzz %s, pixman %s", cairo_version_string(), hb_version_string(),
             "0.44.2");
    cairo_move_to(cr, 48, 318);
    cairo_show_text(cr, info);
}

/* ---- Wayland (mostra a imagem) ---- */
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

static int show(cairo_surface_t* img, int loop) {
    struct wl_display* dpy = NULL;
    for (int i = 0; i < 100 && !(dpy = wl_display_connect(NULL)); i++) usleep(100000);
    if (!dpy) return 0;
    struct wl_registry* reg = wl_display_get_registry(dpy);
    wl_registry_add_listener(reg, &reg_l, 0);
    wl_display_roundtrip(dpy);
    struct wl_surface* s = wl_compositor_create_surface(compositor);
    struct xdg_surface* xs = xdg_wm_base_get_xdg_surface(wm, s);
    xdg_surface_add_listener(xs, &xs_l, 0);
    struct xdg_toplevel* tl = xdg_surface_get_toplevel(xs);
    xdg_toplevel_add_listener(tl, &tl_l, 0);
    xdg_toplevel_set_title(tl, "cairo + harfbuzz");
    wl_surface_commit(s);
    while (!configured && wl_display_dispatch(dpy) >= 0) {}
    int fd = memfd_create("drawtest", MFD_CLOEXEC);
    ftruncate(fd, W * H * 4);
    uint32_t* buf = mmap(NULL, W * H * 4, PROT_READ | PROT_WRITE, MAP_SHARED, fd, 0);
    cairo_surface_flush(img);
    unsigned char* src = cairo_image_surface_get_data(img);
    int stride = cairo_image_surface_get_stride(img);
    for (int y = 0; y < H; y++) memcpy(buf + y * W, src + y * stride, W * 4);
    struct wl_shm_pool* pool = wl_shm_create_pool(shm, fd, W * H * 4);
    struct wl_buffer* b = wl_shm_pool_create_buffer(pool, 0, W, H, W * 4, WL_SHM_FORMAT_ARGB8888);
    wl_shm_pool_destroy(pool);
    close(fd);
    wl_surface_attach(s, b, 0, 0);
    wl_surface_damage(s, 0, 0, W, H);
    wl_surface_commit(s);
    long until = ms_now() + 10000;
    while (!closed && (loop || ms_now() < until)) {
        wl_display_flush(dpy);
        while (wl_display_prepare_read(dpy) != 0) wl_display_dispatch_pending(dpy);
        struct pollfd pf = { wl_display_get_fd(dpy), POLLIN, 0 };
        if (poll(&pf, 1, 100) > 0) wl_display_read_events(dpy);
        else wl_display_cancel_read(dpy);
        wl_display_dispatch_pending(dpy);
    }
    return 1;
}

int main(int argc, char** argv) {
    setvbuf(stdout, NULL, _IONBF, 0);
    int loop = argc > 1 && !strcmp(argv[1], "loop");
    FcInit();
    FT_Library lib;
    FT_Init_FreeType(&lib);
    Font latin = open_font(lib, "Noto Sans", 24);
    Font arab = open_font(lib, "Noto Naskh Arabic", 30);
    Font deva = open_font(lib, "Noto Sans Devanagari", 30);
    if (!latin.hb || !arab.hb || !deva.hb) { printf("[drawtest] fim: FALHA\n"); return 1; }

    /* 1) acento combinante: "e" + U+0301 vira o glifo de "é" (normalizacao) */
    hb_buffer_t* b = shape(&latin, "e\xcc\x81");
    unsigned n; hb_glyph_info_t* gi = hb_buffer_get_glyph_infos(b, &n);
    check(n == 1 && gi[0].codepoint == FT_Get_Char_Index(latin.ft, 0xE9), "e + acento combinante -> um glifo \"é\"");
    hb_buffer_destroy(b);

    /* 2) kerning: "AVATAR" com GPOS fica mais curto que a soma dos avancos */
    b = shape(&latin, "AVATAR");
    hb_glyph_position_t* gp = hb_buffer_get_glyph_positions(b, &n);
    long shaped = 0, raw = 0;
    for (unsigned i = 0; i < n; i++) shaped += gp[i].x_advance;
    for (const char* p = "AVATAR"; *p; p++) {
        FT_Load_Char(latin.ft, *p, FT_LOAD_NO_HINTING);
        raw += latin.ft->glyph->advance.x;
    }
    printf("[drawtest] AVATAR: %ld px com kerning, %ld px sem\n", shaped / 64, raw / 64);
    check(shaped < raw, "kerning (GPOS) aproxima AV/VA/TA");
    hb_buffer_destroy(b);

    /* 3) arabe: direita para a esquerda, letras com formas de ligacao */
    const char* salam = "\xd8\xb3\xd9\x84\xd8\xa7\xd9\x85 \xd8\xb9\xd9\x84\xd9\x8a\xd9\x83\xd9\x85";   /* سلام عليكم */
    b = shape(&arab, salam);
    gi = hb_buffer_get_glyph_infos(b, &n);
    int contextual = 0;
    /* o primeiro caractere (س, inicial) vem por ultimo no buffer RTL */
    if (n && gi[n - 1].codepoint != FT_Get_Char_Index(arab.ft, 0x0633)) contextual = 1;
    check(hb_buffer_get_direction(b) == HB_DIRECTION_RTL && contextual,
          "arabe: direcao RTL e forma inicial de \xd8\xb3 diferente da isolada");
    hb_buffer_destroy(b);

    /* 4) devanagari: em "कि" a vogal i (ि, U+093F) vem DEPOIS no texto mas e
     * desenhada ANTES da consoante: o primeiro glifo e o da vogal */
    b = shape(&deva, "\xe0\xa4\x95\xe0\xa4\xbf");
    gi = hb_buffer_get_glyph_infos(b, &n);
    check(n == 2 && gi[0].cluster == 0 && gi[0].codepoint != FT_Get_Char_Index(deva.ft, 0x0915),
          "devanagari: vogal i reordenada antes da consoante");
    hb_buffer_destroy(b);

    /* cena */
    hb_buffer_t* lines[4] = {
        shape(&latin, "AVATAR, Ação, \"fi\" — e\xcc\x81 composto"),
        shape(&latin, "Olá! ¿Qué tal? Grüße, ½ € © ™"),
        shape(&arab, salam),
        shape(&deva, "\xe0\xa4\xa8\xe0\xa4\xae\xe0\xa4\xb8\xe0\xa5\x8d\xe0\xa4\xa4\xe0\xa5\x87 \xe0\xa4\x95\xe0\xa4\xbf\xe0\xa4\xa4\xe0\xa4\xbe\xe0\xa4\xac"),  /* नमस्ते किताब */
    };
    long t0 = ms_now();
    cairo_surface_t* img = cairo_image_surface_create(CAIRO_FORMAT_ARGB32, W, H);
    cairo_t* cr = cairo_create(img);
    scene(cr, &latin, &arab, &deva, lines);
    check(cairo_status(cr) == CAIRO_STATUS_SUCCESS, "cena desenhada sem erro do cairo");
    cairo_destroy(cr);
    printf("[drawtest] cena %dx%d em %ld ms\n", W, H, ms_now() - t0);

    /* PNG: grava e le de volta */
    mkdir("/tmp", 0777);
    int pw = cairo_surface_write_to_png(img, "/tmp/drawtest.png") == CAIRO_STATUS_SUCCESS;
    cairo_surface_t* back = cairo_image_surface_create_from_png("/tmp/drawtest.png");
    uint32_t a = ((uint32_t*)cairo_image_surface_get_data(img))[200 * W + 545];
    uint32_t c = cairo_surface_status(back) ? 0 : ((uint32_t*)cairo_image_surface_get_data(back))[200 * W + 545];
    check(pw && cairo_image_surface_get_width(back) == W && a == c, "PNG gravado e lido de volta igual (libpng + zlib)");
    cairo_surface_destroy(back);

    /* PDF: a mesma cena em vetor */
    cairo_surface_t* pdf = cairo_pdf_surface_create("/tmp/drawtest.pdf", W, H);
    cr = cairo_create(pdf);
    scene(cr, &latin, &arab, &deva, lines);
    cairo_destroy(cr);
    cairo_surface_destroy(pdf);
    struct stat st;
    char head[5] = { 0 };
    FILE* fp = fopen("/tmp/drawtest.pdf", "rb");
    if (fp) { fread(head, 1, 4, fp); fclose(fp); }
    check(stat("/tmp/drawtest.pdf", &st) == 0 && st.st_size > 2000 && !strcmp(head, "%PDF"), "PDF com fontes embutidas");
    printf("[drawtest] /tmp/drawtest.pdf: %ld bytes\n", (long)st.st_size);

    check(show(img, loop), "janela Wayland com o desenho");
    printf("[drawtest] fim: %s\n", falhas ? "FALHA" : "ok");
    return falhas ? 1 : 0;
}
