/* wlkeys — cliente Wayland com as bibliotecas de verdade: libwayland-client
 * (+ xdg-shell) para a janela e libxkbcommon (+ compose) para o teclado,
 * como um toolkit faz. Recebe o mapa XKB do Pulsar (layout do pulsar.cfg:
 * abnt, abnt2, us ou us-intl), traduz as teclas, compoe as teclas mortas
 * (tabela de $XCOMPOSEFILE) e, no Enter, imprime o texto digitado.
 *
 *   wlkeys                 uma linha e sai
 *   wlkeys loop            uma linha por Enter, ate fechar a janela
 *   wlkeys trocar <layout> (teste) depois da 1a linha muda o layout no
 *                          pulsar.cfg, espera o mapa novo e le a 2a linha */
#define _GNU_SOURCE
#include <locale.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <unistd.h>
#include <wayland-client.h>
#include <xkbcommon/xkbcommon.h>
#include <xkbcommon/xkbcommon-compose.h>
#include "xdg-shell-client-protocol.h"

#define W 420
#define H 160

static struct wl_compositor* compositor;
static struct wl_shm* shm;
static struct xdg_wm_base* wm;
static struct wl_seat* seat;
static struct wl_surface* surface;
static struct xdg_toplevel* toplevel;
static struct xkb_context* xkb;
static struct xkb_keymap* keymap;
static struct xkb_state* state;
static struct xkb_compose_state* compose;
static uint32_t* pixels;
static struct wl_buffer* buffer;
static int configured, done, loop, keymaps, lines;
static const char* switch_to;

/* troca keyboard=... no pulsar.cfg (como as Configuracoes fazem) */
static void set_layout(const char* name) {
    static char buf[4096], out[4200];
    FILE* f = fopen("/pulsar.cfg", "r");
    size_t n = f ? fread(buf, 1, sizeof buf - 1, f) : 0;
    if (f) fclose(f);
    buf[n] = 0;
    char* o = out;
    for (char* line = strtok(buf, "\n"); line; line = strtok(NULL, "\n"))
        o += sprintf(o, "%s\n", strncmp(line, "keyboard=", 9) ? line : "");
    o += sprintf(o, "keyboard=%s\n", name);
    f = fopen("/pulsar.cfg", "w");
    if (f) { fwrite(out, 1, o - out, f); fclose(f); }
}
static char text[512];
static size_t tlen;
static char layout_desc[96] = "?";

static void redraw(void) {
    /* barra que cresce com o texto digitado */
    for (int y = 0; y < H; y++)
        for (int x = 0; x < W; x++)
            pixels[y * W + x] = (x < (int)(tlen * 8) % W && y > 60 && y < 100) ? 0xFF0A84FF : 0xFFF2F2F7;
    wl_surface_attach(surface, buffer, 0, 0);
    wl_surface_damage_buffer(surface, 0, 0, W, H);
    wl_surface_commit(surface);
}

/* ---- teclado */
static void kb_keymap(void* d, struct wl_keyboard* k, uint32_t format, int fd, uint32_t size) {
    (void)d; (void)k;
    if (format != WL_KEYBOARD_KEYMAP_FORMAT_XKB_V1) { close(fd); return; }
    char* s = mmap(NULL, size, PROT_READ, MAP_PRIVATE, fd, 0);
    close(fd);
    if (s == MAP_FAILED) return;
    struct xkb_keymap* km = xkb_keymap_new_from_string(xkb, s, XKB_KEYMAP_FORMAT_TEXT_V1, XKB_KEYMAP_COMPILE_NO_FLAGS);
    munmap(s, size);
    if (!km) { fprintf(stderr, "[wlkeys] mapa XKB invalido\n"); return; }
    if (state) xkb_state_unref(state);
    if (keymap) xkb_keymap_unref(keymap);
    keymap = km;
    state = xkb_state_new(keymap);
    snprintf(layout_desc, sizeof layout_desc, "%s", xkb_keymap_layout_get_name(keymap, 0));
    keymaps++;
    printf("[wlkeys] mapa XKB recebido: %s\n", layout_desc);
    fflush(stdout);
}
static void kb_enter(void* d, struct wl_keyboard* k, uint32_t s, struct wl_surface* f, struct wl_array* keys) { (void)d; (void)k; (void)s; (void)f; (void)keys; }
static void kb_leave(void* d, struct wl_keyboard* k, uint32_t s, struct wl_surface* f) { (void)d; (void)k; (void)s; (void)f; }
static void kb_key(void* d, struct wl_keyboard* k, uint32_t serial, uint32_t time, uint32_t key, uint32_t st) {
    (void)d; (void)k; (void)serial; (void)time;
    if (!state || st != WL_KEYBOARD_KEY_STATE_PRESSED) return;
    xkb_keycode_t code = key + 8;
    xkb_keysym_t sym = xkb_state_key_get_one_sym(state, code);
    char buf[64] = {0};
    if (compose) {
        xkb_compose_state_feed(compose, sym);
        switch (xkb_compose_state_get_status(compose)) {
        case XKB_COMPOSE_COMPOSING: return;                         /* tecla morta: espera a proxima */
        case XKB_COMPOSE_COMPOSED:
            xkb_compose_state_get_utf8(compose, buf, sizeof buf);
            xkb_compose_state_reset(compose);
            goto emit;
        case XKB_COMPOSE_CANCELLED: xkb_compose_state_reset(compose); return;
        case XKB_COMPOSE_NOTHING: break;
        }
    }
    if (sym == XKB_KEY_Return || sym == XKB_KEY_KP_Enter) {
        printf("[wlkeys] texto (%s): %.*s\n", layout_desc, (int)tlen, text);
        fflush(stdout);
        tlen = 0;
        lines++;
        if (switch_to && lines == 1) {
            set_layout(switch_to);
            printf("[wlkeys] pulsar.cfg: keyboard=%s; esperando o mapa novo\n", switch_to);
            fflush(stdout);
            return;
        }
        if (!loop && (!switch_to || lines >= 2)) done = 1;
        return;
    }
    if (sym == XKB_KEY_BackSpace) { if (tlen) tlen--; while (tlen && (text[tlen] & 0xC0) == 0x80) tlen--; redraw(); return; }
    xkb_state_key_get_utf8(state, code, buf, sizeof buf);
emit:
    if (buf[0] && (unsigned char)buf[0] >= 0x20 && tlen + strlen(buf) < sizeof text) {
        memcpy(text + tlen, buf, strlen(buf));
        tlen += strlen(buf);
        redraw();
    }
}
static void kb_mods(void* d, struct wl_keyboard* k, uint32_t s, uint32_t dep, uint32_t lat, uint32_t lock, uint32_t grp) {
    (void)d; (void)k; (void)s;
    if (state) xkb_state_update_mask(state, dep, lat, lock, 0, 0, grp);
}
static void kb_repeat(void* d, struct wl_keyboard* k, int32_t r, int32_t dl) { (void)d; (void)k; (void)r; (void)dl; }
static const struct wl_keyboard_listener kb_listener = { kb_keymap, kb_enter, kb_leave, kb_key, kb_mods, kb_repeat };

/* ---- seat, xdg, registry */
static void seat_caps(void* d, struct wl_seat* s, uint32_t caps) {
    (void)d;
    if (caps & WL_SEAT_CAPABILITY_KEYBOARD) wl_keyboard_add_listener(wl_seat_get_keyboard(s), &kb_listener, NULL);
}
static void seat_name(void* d, struct wl_seat* s, const char* n) { (void)d; (void)s; (void)n; }
static const struct wl_seat_listener seat_listener = { seat_caps, seat_name };

static void wm_ping(void* d, struct xdg_wm_base* b, uint32_t serial) { (void)d; xdg_wm_base_pong(b, serial); }
static const struct xdg_wm_base_listener wm_listener = { wm_ping };
static void xs_configure(void* d, struct xdg_surface* x, uint32_t serial) { (void)d; xdg_surface_ack_configure(x, serial); configured = 1; }
static const struct xdg_surface_listener xs_listener = { xs_configure };
static void tl_configure(void* d, struct xdg_toplevel* t, int32_t w, int32_t h, struct wl_array* s) { (void)d; (void)t; (void)w; (void)h; (void)s; }
static void tl_close(void* d, struct xdg_toplevel* t) { (void)d; (void)t; done = 1; }
static const struct xdg_toplevel_listener tl_listener = { tl_configure, tl_close };

static void reg_global(void* d, struct wl_registry* r, uint32_t name, const char* iface, uint32_t v) {
    (void)d; (void)v;
    if (!strcmp(iface, wl_compositor_interface.name)) compositor = wl_registry_bind(r, name, &wl_compositor_interface, 4);
    else if (!strcmp(iface, wl_shm_interface.name)) shm = wl_registry_bind(r, name, &wl_shm_interface, 1);
    else if (!strcmp(iface, xdg_wm_base_interface.name)) {
        wm = wl_registry_bind(r, name, &xdg_wm_base_interface, 2);
        xdg_wm_base_add_listener(wm, &wm_listener, NULL);
    } else if (!strcmp(iface, wl_seat_interface.name)) {
        seat = wl_registry_bind(r, name, &wl_seat_interface, 5);
        wl_seat_add_listener(seat, &seat_listener, NULL);
    }
}
static void reg_remove(void* d, struct wl_registry* r, uint32_t n) { (void)d; (void)r; (void)n; }
static const struct wl_registry_listener reg_listener = { reg_global, reg_remove };

int main(int argc, char** argv) {
    loop = argc > 1 && !strcmp(argv[1], "loop");
    if (argc > 2 && !strcmp(argv[1], "trocar")) switch_to = argv[2];
    setlocale(LC_ALL, "");
    struct wl_display* dpy = NULL;
    for (int i = 0; i < 50 && !(dpy = wl_display_connect(NULL)); i++) usleep(100000);
    if (!dpy) { fprintf(stderr, "[wlkeys] sem servidor Wayland\n"); return 1; }
    xkb = xkb_context_new(XKB_CONTEXT_NO_FLAGS);
    if (getenv("WLKEYS_DEBUG")) { xkb_context_set_log_level(xkb, XKB_LOG_LEVEL_DEBUG); xkb_context_set_log_verbosity(xkb, 10); }
    const char* loc = getenv("LANG");
    struct xkb_compose_table* ct = xkb_compose_table_new_from_locale(xkb, loc && *loc ? loc : "C", XKB_COMPOSE_COMPILE_NO_FLAGS);
    if (ct) compose = xkb_compose_state_new(ct, XKB_COMPOSE_STATE_NO_FLAGS);
    printf("[wlkeys] compose: %s\n", ct ? "carregado" : "indisponivel");

    struct wl_registry* reg = wl_display_get_registry(dpy);
    wl_registry_add_listener(reg, &reg_listener, NULL);
    wl_display_roundtrip(dpy);
    if (!compositor || !shm || !wm || !seat) { fprintf(stderr, "[wlkeys] globais faltando\n"); return 2; }

    surface = wl_compositor_create_surface(compositor);
    struct xdg_surface* xs = xdg_wm_base_get_xdg_surface(wm, surface);
    xdg_surface_add_listener(xs, &xs_listener, NULL);
    toplevel = xdg_surface_get_toplevel(xs);
    xdg_toplevel_add_listener(toplevel, &tl_listener, NULL);
    xdg_toplevel_set_title(toplevel, "Teclado (XKB)");
    wl_surface_commit(surface);
    while (!configured && wl_display_dispatch(dpy) >= 0) {}

    int fd = memfd_create("wlkeys", MFD_CLOEXEC);
    ftruncate(fd, W * H * 4);
    pixels = mmap(NULL, W * H * 4, PROT_READ | PROT_WRITE, MAP_SHARED, fd, 0);
    struct wl_shm_pool* pool = wl_shm_create_pool(shm, fd, W * H * 4);
    buffer = wl_shm_pool_create_buffer(pool, 0, W, H, W * 4, WL_SHM_FORMAT_XRGB8888);
    redraw();
    printf("[wlkeys] pronto\n");
    fflush(stdout);
    while (!done && wl_display_dispatch(dpy) >= 0) {}
    wl_display_disconnect(dpy);
    if (switch_to) printf("[wlkeys] mapas recebidos: %d\n", keymaps);
    return keymaps > (switch_to ? 1 : 0) ? 0 : 3;
}
