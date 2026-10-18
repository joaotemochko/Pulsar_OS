/* wlpopup — teste da cena Wayland do Pulsar com libwayland-client:
 *
 *   - janela com geometria (xdg_surface.set_window_geometry), como as
 *     decoracoes do lado do cliente (GTK): o buffer tem 20 px de margem
 *     transparente em volta;
 *   - subsuperficie ARGB semitransparente (wl_subcompositor);
 *   - popup com sombra e grab (xdg_popup), posicionado por um
 *     xdg_positioner que nao cabe para baixo: o compositor tem que
 *     espelhar (flip_y) e deslizar (slide_x) para dentro da janela.
 *
 * Imprime qual superficie recebe ponteiro, botoes e teclado. Termina com
 * "fim: ok" depois que um clique fora do popup o fecha (popup_done) e um
 * clique seguinte chega a subsuperficie; ou "FALHA" apos 60 s.
 * O pool de memoria e destruido logo depois de criar os buffers (como o
 * cairo faz): os buffers precisam continuar validos. */
#define _GNU_SOURCE
#include <errno.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <time.h>
#include <unistd.h>
#include <wayland-client.h>
#include "xdg-shell-client-protocol.h"

#define M 20                    /* margem transparente (sombra) da janela */
#define WW 400
#define WH 300
#define SW 120
#define SH 60
#define PW 150                  /* geometria do popup */
#define PH 110
#define PM 10                   /* margem de sombra do popup */

static struct wl_compositor* compositor;
static struct wl_subcompositor* subcomp;
static struct wl_shm* shm;
static struct xdg_wm_base* wm;
static struct wl_seat* seat;
static struct wl_surface *win, *sub, *pop;
static struct xdg_popup* popup;
static int configured, popup_cfg, popup_done_seen, frame_done;
static int pcx = -1, pcy, pcw, pch;
static int kb_on_popup, kb_back_on_win, ptr_on_popup, press_on_sub, press_after_done;

static const char* name(struct wl_surface* s) {
    return s == win ? "janela" : s == sub ? "subsuperficie" : s == pop ? "popup" : "?";
}

/* buffer ARGB premultiplicado preenchido por fn(x, y) */
static struct wl_buffer* make_buffer(int w, int h, uint32_t (*fn)(int, int)) {
    int fd = memfd_create("wlpopup", MFD_CLOEXEC);
    size_t size = (size_t)w * h * 4;
    ftruncate(fd, size);
    uint32_t* px = mmap(NULL, size, PROT_READ | PROT_WRITE, MAP_SHARED, fd, 0);
    for (int y = 0; y < h; y++)
        for (int x = 0; x < w; x++) px[y * w + x] = fn(x, y);
    struct wl_shm_pool* pool = wl_shm_create_pool(shm, fd, size);
    struct wl_buffer* b = wl_shm_pool_create_buffer(pool, 0, w, h, w * 4, WL_SHM_FORMAT_ARGB8888);
    wl_shm_pool_destroy(pool);              /* o buffer segura a memoria */
    munmap(px, size);
    close(fd);
    return b;
}

static uint32_t win_px(int x, int y) {
    if (x < M || y < M || x >= M + WW || y >= M + WH) return 0x20000000;   /* sombra fraca */
    x -= M; y -= M;
    if (y < 36) return 0xFF3A3F4B;                                         /* "barra" do cliente */
    return ((x / 20 + y / 20) & 1) ? 0xFFE8E8EE : 0xFFF6F6FA;
}
static uint32_t sub_px(int x, int y) {
    if (x < 3 || y < 3 || x >= SW - 3 || y >= SH - 3) return 0xFF0A7A2A;   /* borda opaca */
    return 0x80008000;                                                     /* verde 50% */
}
static uint32_t pop_px(int x, int y) {
    if (x < PM || y < PM || x >= PM + PW || y >= PM + PH) return 0x40000000;
    x -= PM; y -= PM;
    if (y >= 8 + 0 * 32 && y < 8 + 0 * 32 + 24 && x >= 8 && x < PW - 8) return 0xFF0A64D8;  /* item realcado */
    if ((y - 8) % 32 < 24 && x >= 16 && x < 16 + (y / 32 + 2) * 18) return 0xFF505560;  /* "texto" */
    return 0xFFFFFFFF;
}

/* ---- xdg ---- */
static void wm_ping(void* d, struct xdg_wm_base* b, uint32_t s) { xdg_wm_base_pong(b, s); }
static const struct xdg_wm_base_listener wm_l = { wm_ping };
static void xs_configure(void* d, struct xdg_surface* x, uint32_t s) { xdg_surface_ack_configure(x, s); (*(int*)d)++; }
static const struct xdg_surface_listener xs_l = { xs_configure };
static void tl_configure(void* d, struct xdg_toplevel* t, int32_t w, int32_t h, struct wl_array* st) {}
static void tl_close(void* d, struct xdg_toplevel* t) { exit(0); }
static const struct xdg_toplevel_listener tl_l = { tl_configure, tl_close };
static void pp_configure(void* d, struct xdg_popup* p, int32_t x, int32_t y, int32_t w, int32_t h) {
    pcx = x; pcy = y; pcw = w; pch = h;
    printf("[wlpopup] popup configurado em %d,%d %dx%d\n", x, y, w, h);
}
static void pp_done(void* d, struct xdg_popup* p) {
    popup_done_seen = 1;
    printf("[wlpopup] popup_done (clique fora fechou o menu)\n");
    xdg_popup_destroy(popup);
    popup = NULL;
    wl_surface_destroy(pop);
    pop = NULL;
}
static void pp_repositioned(void* d, struct xdg_popup* p, uint32_t t) {}
static const struct xdg_popup_listener pp_l = { pp_configure, pp_done, pp_repositioned };

/* ---- entrada ---- */
static void p_enter(void* d, struct wl_pointer* p, uint32_t s, struct wl_surface* su, wl_fixed_t x, wl_fixed_t y) {
    printf("[wlpopup] ponteiro entrou: %s (%d,%d)\n", name(su), wl_fixed_to_int(x), wl_fixed_to_int(y));
    if (su == pop) ptr_on_popup = 1;
}
static void p_leave(void* d, struct wl_pointer* p, uint32_t s, struct wl_surface* su) {}
static void p_motion(void* d, struct wl_pointer* p, uint32_t t, wl_fixed_t x, wl_fixed_t y) {}
static struct wl_surface* ptr_surface;
static void p_enter2(void* d, struct wl_pointer* p, uint32_t s, struct wl_surface* su, wl_fixed_t x, wl_fixed_t y) {
    ptr_surface = su;
    p_enter(d, p, s, su, x, y);
}
static void p_button(void* d, struct wl_pointer* p, uint32_t s, uint32_t t, uint32_t b, uint32_t st) {
    if (!st) return;
    printf("[wlpopup] clique em: %s\n", name(ptr_surface));
    if (ptr_surface == sub && popup_done_seen) press_on_sub = 1;
    if (popup_done_seen) press_after_done = 1;
}
static void p_axis(void* d, struct wl_pointer* p, uint32_t t, uint32_t a, wl_fixed_t v) {}
static void p_frame(void* d, struct wl_pointer* p) {}
static void p_axis_source(void* d, struct wl_pointer* p, uint32_t s) {}
static void p_axis_stop(void* d, struct wl_pointer* p, uint32_t t, uint32_t a) {}
static void p_axis_discrete(void* d, struct wl_pointer* p, uint32_t a, int32_t v) {}
static const struct wl_pointer_listener ptr_l = { p_enter2, p_leave, p_motion, p_button, p_axis, p_frame,
                                                  p_axis_source, p_axis_stop, p_axis_discrete };
static void k_keymap(void* d, struct wl_keyboard* k, uint32_t f, int32_t fd, uint32_t sz) { close(fd); }
static void k_enter(void* d, struct wl_keyboard* k, uint32_t s, struct wl_surface* su, struct wl_array* keys) {
    printf("[wlpopup] teclado entrou: %s\n", name(su));
    if (su == pop) kb_on_popup = 1;
    if (su == win && popup_done_seen) kb_back_on_win = 1;
}
static void k_leave(void* d, struct wl_keyboard* k, uint32_t s, struct wl_surface* su) {}
static void k_key(void* d, struct wl_keyboard* k, uint32_t s, uint32_t t, uint32_t key, uint32_t st) {}
static void k_mods(void* d, struct wl_keyboard* k, uint32_t s, uint32_t a, uint32_t b, uint32_t c, uint32_t g) {}
static void k_repeat(void* d, struct wl_keyboard* k, int32_t r, int32_t dl) {}
static const struct wl_keyboard_listener kb_l = { k_keymap, k_enter, k_leave, k_key, k_mods, k_repeat };

static void reg_global(void* d, struct wl_registry* r, uint32_t n, const char* i, uint32_t v) {
    if (!strcmp(i, "wl_compositor")) compositor = wl_registry_bind(r, n, &wl_compositor_interface, 4);
    else if (!strcmp(i, "wl_subcompositor")) subcomp = wl_registry_bind(r, n, &wl_subcompositor_interface, 1);
    else if (!strcmp(i, "wl_shm")) shm = wl_registry_bind(r, n, &wl_shm_interface, 1);
    else if (!strcmp(i, "xdg_wm_base")) { wm = wl_registry_bind(r, n, &xdg_wm_base_interface, 2); xdg_wm_base_add_listener(wm, &wm_l, 0); }
    else if (!strcmp(i, "wl_seat")) seat = wl_registry_bind(r, n, &wl_seat_interface, 5);
}
static void reg_remove(void* d, struct wl_registry* r, uint32_t n) {}
static const struct wl_registry_listener reg_l = { reg_global, reg_remove };

static void frame_cb(void* d, struct wl_callback* cb, uint32_t t) { frame_done = 1; wl_callback_destroy(cb); }
static const struct wl_callback_listener frame_l = { frame_cb };

int main(void) {
    setvbuf(stdout, NULL, _IONBF, 0);
    struct wl_display* dpy = NULL;
    for (int i = 0; i < 100 && !(dpy = wl_display_connect(NULL)); i++) usleep(100000);   /* servico subindo */
    if (!dpy) { printf("[wlpopup] sem servidor Wayland\nfim: FALHA\n"); return 1; }
    struct wl_registry* reg = wl_display_get_registry(dpy);
    wl_registry_add_listener(reg, &reg_l, 0);
    wl_display_roundtrip(dpy);
    if (!compositor || !subcomp || !shm || !wm || !seat) { printf("[wlpopup] globais faltando\nfim: FALHA\n"); return 1; }
    wl_pointer_add_listener(wl_seat_get_pointer(seat), &ptr_l, 0);
    wl_keyboard_add_listener(wl_seat_get_keyboard(seat), &kb_l, 0);

    /* janela com 20 px de "sombra" fora da geometria */
    win = wl_compositor_create_surface(compositor);
    struct xdg_surface* xs = xdg_wm_base_get_xdg_surface(wm, win);
    xdg_surface_add_listener(xs, &xs_l, &configured);
    struct xdg_toplevel* tl = xdg_surface_get_toplevel(xs);
    xdg_toplevel_add_listener(tl, &tl_l, 0);
    xdg_toplevel_set_title(tl, "Popups e subsuperficies");
    xdg_surface_set_window_geometry(xs, M, M, WW, WH);
    wl_surface_commit(win);
    while (!configured) wl_display_dispatch(dpy);

    /* subsuperficie: (40, 200) na superficie = (20, 180) na janela */
    sub = wl_compositor_create_surface(compositor);
    struct wl_subsurface* ss = wl_subcompositor_get_subsurface(subcomp, sub, win);
    wl_subsurface_set_position(ss, M + 20, M + 180);
    wl_subsurface_set_desync(ss);
    wl_surface_attach(sub, make_buffer(SW, SH, sub_px), 0, 0);
    wl_surface_damage(sub, 0, 0, SW, SH);
    wl_surface_commit(sub);

    wl_surface_attach(win, make_buffer(WW + 2 * M, WH + 2 * M, win_px), 0, 0);
    wl_surface_damage(win, 0, 0, WW + 2 * M, WH + 2 * M);
    struct wl_callback* cb = wl_surface_frame(win);
    wl_callback_add_listener(cb, &frame_l, 0);
    wl_surface_commit(win);
    while (!frame_done) wl_display_dispatch(dpy);
    printf("[wlpopup] janela e subsuperficie na tela\n");

    /* popup: ancora no canto inferior direito de (300,250 80x20), crescendo
     * para baixo/direita — nao cabe: tem que espelhar para cima e deslizar */
    pop = wl_compositor_create_surface(compositor);
    struct xdg_surface* pxs = xdg_wm_base_get_xdg_surface(wm, pop);
    xdg_surface_add_listener(pxs, &xs_l, &popup_cfg);
    struct xdg_positioner* pos = xdg_wm_base_create_positioner(wm);
    xdg_positioner_set_size(pos, PW, PH);
    xdg_positioner_set_anchor_rect(pos, 300, 250, 80, 20);
    xdg_positioner_set_anchor(pos, XDG_POSITIONER_ANCHOR_BOTTOM_RIGHT);
    xdg_positioner_set_gravity(pos, XDG_POSITIONER_GRAVITY_BOTTOM_RIGHT);
    xdg_positioner_set_constraint_adjustment(pos, XDG_POSITIONER_CONSTRAINT_ADJUSTMENT_SLIDE_X |
        XDG_POSITIONER_CONSTRAINT_ADJUSTMENT_SLIDE_Y | XDG_POSITIONER_CONSTRAINT_ADJUSTMENT_FLIP_Y);
    popup = xdg_surface_get_popup(pxs, xs, pos);
    xdg_popup_add_listener(popup, &pp_l, 0);
    xdg_positioner_destroy(pos);
    xdg_popup_grab(popup, seat, 0);
    xdg_surface_set_window_geometry(pxs, PM, PM, PW, PH);
    wl_surface_commit(pop);
    while (!popup_cfg) wl_display_dispatch(dpy);
    wl_surface_attach(pop, make_buffer(PW + 2 * PM, PH + 2 * PM, pop_px), 0, 0);
    wl_surface_damage(pop, 0, 0, PW + 2 * PM, PH + 2 * PM);
    wl_surface_commit(pop);
    wl_display_flush(dpy);
    printf("[wlpopup] popup aberto\n");

    struct timespec t0, t1;
    clock_gettime(CLOCK_MONOTONIC, &t0);
    while (!(popup_done_seen && press_on_sub)) {
        if (wl_display_dispatch(dpy) < 0) {
            const struct wl_interface* iface; uint32_t id;
            uint32_t code = wl_display_get_protocol_error(dpy, &iface, &id);
            printf("[wlpopup] conexao falhou: erro %d, protocolo %u (%s %u)\n", wl_display_get_error(dpy), code,
                   iface ? iface->name : "-", id);
            break;
        }
        clock_gettime(CLOCK_MONOTONIC, &t1);
        if (t1.tv_sec - t0.tv_sec > 60) break;
    }
    int pos_ok = pcx == 250 && pcy == 140 && pcw == PW && pch == PH;
    printf("[wlpopup] posicao espelhada+deslizada %s, teclado no popup %s, ponteiro no popup %s,\n"
           "[wlpopup] popup_done %s, teclado de volta na janela %s, clique na subsuperficie %s\n",
           pos_ok ? "ok" : "ERRADA", kb_on_popup ? "ok" : "nao", ptr_on_popup ? "ok" : "nao",
           popup_done_seen ? "ok" : "nao", kb_back_on_win ? "ok" : "nao", press_on_sub ? "ok" : "nao");
    int ok = pos_ok && kb_on_popup && ptr_on_popup && popup_done_seen && kb_back_on_win && press_on_sub;
    printf("[wlpopup] fim: %s\n", ok ? "ok" : "FALHA");
    return ok ? 0 : 1;
}
