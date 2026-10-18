/* pangotest — a base do GTK nos programas Linux do Pulsar:
 *
 *   glib        laco principal (timeouts, idle), GRegex (pcre2), GObject com
 *               propriedade e sinal, GVariant, GThreadPool, arquivos (gio) e
 *               processo filho (g_spawn: fc-match)
 *   pango       layout com markup e quebra de linha; troca de fonte por
 *               caractere (latim, ✓, arabe, hebraico, devanagari numa linha
 *               so, pedindo apenas "sans-serif") e texto bidirecional
 *   gdk-pixbuf  le um PNG, reduz para 1/3 e grava
 *
 * Desenha o resultado numa janela Wayland (~10 s; "pangotest loop" fica
 * aberto) e termina com "fim: ok" se tudo conferiu. */
#define _GNU_SOURCE
#include <gdk-pixbuf/gdk-pixbuf.h>
#include <gio/gio.h>
#include <glib-object.h>
#include <glib.h>
#include <pango/pangocairo.h>
#include <poll.h>
#include <stdio.h>
#include <string.h>
#include <sys/mman.h>
#include <unistd.h>
#include <wayland-client.h>
#include "xdg-shell-client-protocol.h"

#define W 720
#define H 520

static int falhas;
static void check(int ok, const char* what) {
    printf("[pangotest] %s    %s\n", ok ? "ok  " : "FALHOU", what);
    if (!ok) falhas++;
}

/* ---- GObject: um contador com propriedade "value" e sinal "changed" ---- */
typedef struct { GObject parent; int value; } PulsarCounter;
typedef struct { GObjectClass parent; } PulsarCounterClass;
G_DEFINE_TYPE(PulsarCounter, pulsar_counter, G_TYPE_OBJECT)
enum { PROP_0, PROP_VALUE };
static guint sig_changed;
static void pc_set(GObject* o, guint id, const GValue* v, GParamSpec* ps) {
    PulsarCounter* c = (PulsarCounter*)o;
    if (id == PROP_VALUE) { c->value = g_value_get_int(v); g_signal_emit(o, sig_changed, 0, c->value); }
}
static void pc_get(GObject* o, guint id, GValue* v, GParamSpec* ps) {
    if (id == PROP_VALUE) g_value_set_int(v, ((PulsarCounter*)o)->value);
}
static void pulsar_counter_class_init(PulsarCounterClass* k) {
    GObjectClass* oc = G_OBJECT_CLASS(k);
    oc->set_property = pc_set;
    oc->get_property = pc_get;
    g_object_class_install_property(oc, PROP_VALUE, g_param_spec_int("value", "Valor", "o valor", 0, 1000, 0, G_PARAM_READWRITE));
    sig_changed = g_signal_new("changed", G_TYPE_FROM_CLASS(k), G_SIGNAL_RUN_LAST, 0, NULL, NULL, NULL,
                               G_TYPE_NONE, 1, G_TYPE_INT);
}
static void pulsar_counter_init(PulsarCounter* c) {}
static int changed_sum, notified;
static void on_changed(PulsarCounter* c, int v, gpointer d) { changed_sum += v; }
static void on_notify(GObject* o, GParamSpec* ps, gpointer d) { notified++; }

/* ---- laco principal ---- */
static int ticks, idles;
static gboolean on_tick(gpointer loop) {
    if (++ticks == 3) { g_main_loop_quit(loop); return G_SOURCE_REMOVE; }
    return G_SOURCE_CONTINUE;
}
static gboolean on_idle(gpointer d) { idles++; return G_SOURCE_REMOVE; }

/* ---- pool de threads ---- */
static gint pool_sum;
static void pool_job(gpointer data, gpointer user) { g_atomic_int_add(&pool_sum, GPOINTER_TO_INT(data)); }

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

static int show(cairo_surface_t* img, int loop) {
    struct wl_display* dpy = NULL;
    for (int i = 0; i < 100 && !(dpy = wl_display_connect(NULL)); i++) g_usleep(100000);
    if (!dpy) return 0;
    struct wl_registry* reg = wl_display_get_registry(dpy);
    wl_registry_add_listener(reg, &reg_l, 0);
    wl_display_roundtrip(dpy);
    struct wl_surface* s = wl_compositor_create_surface(compositor);
    struct xdg_surface* xs = xdg_wm_base_get_xdg_surface(wm, s);
    xdg_surface_add_listener(xs, &xs_l, 0);
    struct xdg_toplevel* tl = xdg_surface_get_toplevel(xs);
    xdg_toplevel_add_listener(tl, &tl_l, 0);
    xdg_toplevel_set_title(tl, "pango + gdk-pixbuf");
    wl_surface_commit(s);
    while (!configured && wl_display_dispatch(dpy) >= 0) {}
    int fd = memfd_create("pangotest", MFD_CLOEXEC);
    ftruncate(fd, W * H * 4);
    uint32_t* buf = mmap(NULL, W * H * 4, PROT_READ | PROT_WRITE, MAP_SHARED, fd, 0);
    cairo_surface_flush(img);
    unsigned char* src = cairo_image_surface_get_data(img);
    for (int y = 0; y < H; y++) memcpy(buf + y * W, src + y * cairo_image_surface_get_stride(img), W * 4);
    struct wl_shm_pool* pool = wl_shm_create_pool(shm, fd, W * H * 4);
    struct wl_buffer* b = wl_shm_pool_create_buffer(pool, 0, W, H, W * 4, WL_SHM_FORMAT_XRGB8888);
    wl_shm_pool_destroy(pool);
    close(fd);
    wl_surface_attach(s, b, 0, 0);
    wl_surface_damage(s, 0, 0, W, H);
    wl_surface_commit(s);
    gint64 until = g_get_monotonic_time() + 10 * G_USEC_PER_SEC;
    while (!closed && (loop || g_get_monotonic_time() < until)) {
        wl_display_flush(dpy);
        while (wl_display_prepare_read(dpy) != 0) wl_display_dispatch_pending(dpy);
        struct pollfd pf = { wl_display_get_fd(dpy), POLLIN, 0 };
        if (poll(&pf, 1, 100) > 0) wl_display_read_events(dpy);
        else wl_display_cancel_read(dpy);
        wl_display_dispatch_pending(dpy);
    }
    return 1;
}

/* pixbuf (RGB/RGBA) -> superficie do cairo em (x, y) */
static void paint_pixbuf(cairo_t* cr, GdkPixbuf* pb, double x, double y) {
    int w = gdk_pixbuf_get_width(pb), h = gdk_pixbuf_get_height(pb), n = gdk_pixbuf_get_n_channels(pb);
    cairo_surface_t* s = cairo_image_surface_create(CAIRO_FORMAT_RGB24, w, h);
    const guchar* p = gdk_pixbuf_read_pixels(pb);
    for (int j = 0; j < h; j++) {
        uint32_t* d = (uint32_t*)(cairo_image_surface_get_data(s) + j * cairo_image_surface_get_stride(s));
        const guchar* r = p + j * gdk_pixbuf_get_rowstride(pb);
        for (int i = 0; i < w; i++) d[i] = r[i * n] << 16 | r[i * n + 1] << 8 | r[i * n + 2];
    }
    cairo_surface_mark_dirty(s);
    cairo_set_source_surface(cr, s, x, y);
    cairo_paint(cr);
    cairo_surface_destroy(s);
}

int main(int argc, char** argv) {
    setvbuf(stdout, NULL, _IONBF, 0);
    int loop = argc > 1 && !strcmp(argv[1], "loop");
    printf("[pangotest] glib %d.%d.%d, pango %s, gdk-pixbuf %s, charset %s, home %s\n", glib_major_version,
           glib_minor_version, glib_micro_version, pango_version_string(), GDK_PIXBUF_VERSION,
           g_get_codeset(), g_get_home_dir());

    /* glib: laco principal */
    GMainLoop* ml = g_main_loop_new(NULL, FALSE);
    g_timeout_add(30, on_tick, ml);
    g_idle_add(on_idle, NULL);
    gint64 t0 = g_get_monotonic_time();
    g_main_loop_run(ml);
    gint64 dt = (g_get_monotonic_time() - t0) / 1000;
    check(ticks == 3 && idles == 1 && dt >= 80, "GMainLoop: 3 timeouts de 30 ms e um idle");

    /* GRegex (pcre2) */
    GMatchInfo* mi = NULL;
    GRegex* re = g_regex_new("^(\\w+)@(\\w+)\\.com$", 0, 0, NULL);
    gboolean m = g_regex_match(re, "ana@pulsar.com", 0, &mi);
    char* who = m ? g_match_info_fetch(mi, 2) : NULL;
    char* rep = g_regex_replace_literal(g_regex_new("[aeiou]", 0, 0, NULL), "pulsar os", -1, 0, "*", 0, NULL);
    check(m && who && !strcmp(who, "pulsar") && !strcmp(rep, "p*ls*r *s"), "GRegex (pcre2): grupos e substituicao");

    /* GObject */
    PulsarCounter* c = g_object_new(pulsar_counter_get_type(), NULL);
    g_signal_connect(c, "changed", G_CALLBACK(on_changed), NULL);
    g_signal_connect(c, "notify::value", G_CALLBACK(on_notify), NULL);
    g_object_set(c, "value", 7, NULL);
    g_object_set(c, "value", 35, NULL);
    int v = 0;
    g_object_get(c, "value", &v, NULL);
    check(v == 35 && changed_sum == 42 && notified == 2, "GObject: propriedade, sinal e notify::");
    g_object_unref(c);

    /* GVariant */
    GVariant* var = g_variant_parse(NULL, "{'n': <42>, 's': <'ola'>}", NULL, NULL, NULL);
    gint32 n42 = 0;
    g_variant_lookup(var, "n", "i", &n42);
    char* vs = g_variant_print(var, TRUE);
    check(n42 == 42, "GVariant: parse e lookup");
    printf("[pangotest] GVariant: %s\n", vs);

    /* GThreadPool */
    GThreadPool* pool = g_thread_pool_new(pool_job, NULL, 4, TRUE, NULL);
    for (int i = 1; i <= 100; i++) g_thread_pool_push(pool, GINT_TO_POINTER(i), NULL);
    g_thread_pool_free(pool, FALSE, TRUE);
    check(pool_sum == 5050, "GThreadPool com 4 threads soma 1..100");

    /* gio: arquivo */
    g_mkdir_with_parents("/tmp", 0777);
    g_file_set_contents("/tmp/pangotest.txt", "Olá, gio!\n", -1, NULL);
    GFile* f = g_file_new_for_path("/tmp/pangotest.txt");
    GFileInfo* fi = g_file_query_info(f, G_FILE_ATTRIBUTE_STANDARD_SIZE, 0, NULL, NULL);
    char* data = NULL;
    g_file_load_contents(f, NULL, &data, NULL, NULL, NULL);
    check(fi && g_file_info_get_size(fi) == 11 && data && !strcmp(data, "Olá, gio!\n"), "gio: GFile grava, consulta e le");

    /* processo filho com saida capturada */
    char* out = NULL; int status = -1;
    char* av[] = { "/bin/fc-match", "monospace", NULL };
    gboolean sp = g_spawn_sync(NULL, av, NULL, 0, NULL, NULL, &out, NULL, &status, NULL);
    check(sp && out && strstr(out, "Noto Sans Mono") && g_spawn_check_wait_status(status, NULL),
          "g_spawn_sync roda fc-match e captura a saida");

    /* pango */
    cairo_surface_t* img = cairo_image_surface_create(CAIRO_FORMAT_ARGB32, W, H);
    cairo_t* cr = cairo_create(img);
    cairo_set_source_rgb(cr, 0.97, 0.97, 0.98);
    cairo_paint(cr);
    PangoLayout* lay = pango_cairo_create_layout(cr);

    pango_layout_set_markup(lay, "<span font='sans-serif bold 22'>pango no Pulsar</span>\n"
        "<span font='sans-serif 14'>Um parágrafo longo o bastante para ser <b>quebrado</b> em várias linhas pelo "
        "<i>pango</i>, com <span foreground='#0A64D8'>cores</span>, <u>sublinhado</u>, "
        "<tt>monoespaçado</tt> e <span size='larger'>tamanhos</span> diferentes.</span>", -1);
    pango_layout_set_width(lay, (W - 60) * PANGO_SCALE);
    pango_layout_set_wrap(lay, PANGO_WRAP_WORD);
    cairo_set_source_rgb(cr, 0.11, 0.11, 0.12);
    cairo_move_to(cr, 30, 24);
    pango_cairo_show_layout(cr, lay);
    int lines = pango_layout_get_line_count(lay);
    check(lines >= 3, "pango: markup e quebra de linha");

    /* troca de fonte por caractere e texto bidirecional, pedindo so "sans-serif" */
    PangoFontDescription* fd = pango_font_description_from_string("sans-serif 22");
    pango_layout_set_attributes(lay, NULL);          /* os do markup valeriam por posicao */
    pango_layout_set_font_description(lay, fd);
    pango_layout_set_width(lay, -1);
    const char* mix = "Pulsar ✓ سلام عليكم שלום नमस्ते";
    pango_layout_set_text(lay, mix, -1);
    PangoLayoutIter* it = pango_layout_get_iter(lay);
    GString* fams = g_string_new("");
    int rtl = 0;
    do {
        PangoLayoutRun* run = pango_layout_iter_get_run_readonly(it);
        if (!run) continue;
        PangoFontDescription* d = pango_font_describe(run->item->analysis.font);
        const char* fam = pango_font_description_get_family(d);
        if (!strstr(fams->str, fam)) g_string_append_printf(fams, "%s%s", fams->len ? ", " : "", fam);
        if (run->item->analysis.level & 1) rtl = 1;
        pango_font_description_free(d);
    } while (pango_layout_iter_next_run(it));
    pango_layout_iter_free(it);
    printf("[pangotest] fontes usadas: %s\n", fams->str);
    check(strstr(fams->str, "Noto Sans") && strstr(fams->str, "DejaVu Sans") && strstr(fams->str, "Noto Naskh Arabic")
          && strstr(fams->str, "Noto Sans Hebrew") && strstr(fams->str, "Noto Sans Devanagari"),
          "pango: troca de fonte por caractere (latim, ✓, arabe, hebraico, devanagari)");
    check(rtl, "pango + fribidi: trechos RTL no meio do texto LTR");
    cairo_set_source_rgb(cr, 0.05, 0.35, 0.60);
    cairo_move_to(cr, 30, 200);
    pango_cairo_show_layout(cr, lay);

    pango_layout_set_font_description(lay, pango_font_description_from_string("monospace 11"));
    char info[200];
    snprintf(info, sizeof info, "glib %d.%d.%d  pango %s  gdk-pixbuf %s  harfbuzz %s  cairo %s", glib_major_version,
             glib_minor_version, glib_micro_version, pango_version_string(), GDK_PIXBUF_VERSION,
             hb_version_string(), cairo_version_string());
    pango_layout_set_text(lay, info, -1);
    cairo_set_source_rgb(cr, 0.12, 0.45, 0.25);
    cairo_move_to(cr, 30, 260);
    pango_cairo_show_layout(cr, lay);
    cairo_surface_flush(img);

    /* gdk-pixbuf: o que foi desenhado ate aqui vira PNG, e lido, reduzido e gravado */
    cairo_surface_write_to_png(img, "/tmp/pangotest.png");
    GError* err = NULL;
    GdkPixbuf* pb = gdk_pixbuf_new_from_file("/tmp/pangotest.png", &err);
    GdkPixbuf* half = pb ? gdk_pixbuf_scale_simple(pb, W / 3, H / 3, GDK_INTERP_BILINEAR) : NULL;
    gboolean saved = half && gdk_pixbuf_save(half, "/tmp/pangotest-mini.png", "png", &err, NULL);
    GdkPixbuf* back = saved ? gdk_pixbuf_new_from_file("/tmp/pangotest-mini.png", NULL) : NULL;
    check(back && gdk_pixbuf_get_width(back) == W / 3 && gdk_pixbuf_get_height(back) == H / 3,
          "gdk-pixbuf: le PNG, reduz para 1/3 e grava");
    if (err) printf("[pangotest] gdk-pixbuf: %s\n", err->message);
    if (back) {
        cairo_set_source_rgb(cr, 0.75, 0.75, 0.78);
        cairo_rectangle(cr, W - 30 - W / 3 - 1, H - 20 - H / 3 - 1, W / 3 + 2, H / 3 + 2);
        cairo_fill(cr);
        paint_pixbuf(cr, back, W - 30 - W / 3, H - 20 - H / 3);
    }
    cairo_destroy(cr);

    check(show(img, loop), "janela Wayland");
    printf("[pangotest] fim: %s\n", falhas ? "FALHA" : "ok");
    return falhas ? 1 : 0;
}
