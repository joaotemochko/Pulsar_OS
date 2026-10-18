/* wlhello — cliente Wayland minimo, falando o protocolo direto (sem
 * libwayland): registry, wl_compositor, wl_shm (memfd), xdg_wm_base e
 * xdg_toplevel, wl_seat (ponteiro e teclado) e callbacks de quadro.
 * No Pulsar ele vira uma janela comum, pelo servico de compatibilidade
 * Wayland (wlsrv + wlclient).
 *
 *   wlhello        anima 120 quadros, imprime as estatisticas e sai
 *   wlhello loop   fica aberto ate fechar a janela ou apertar Esc ou Q
 *   wlhello parcial [loop]  (900 quadros) fundo fixo; so a bolinha muda e so ela e declarada
 *                  em wl_surface.damage (o compositor recompoe so ali) */
#define _GNU_SOURCE
#include <errno.h>
#include <math.h>
#include <poll.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <sys/socket.h>
#include <sys/un.h>
#include <time.h>
#include <unistd.h>

#define W 480
#define H 320

static int sock;
static uint32_t next_id = 2;
static uint8_t out[4096];
static size_t outlen;

/* ---- envio */
static uint32_t* msg_begin(uint32_t obj, uint32_t op) {
    uint32_t* m = (uint32_t*)(out + outlen);
    m[0] = obj; m[1] = op;
    outlen += 8;
    return m;
}
static void u32(uint32_t v) { memcpy(out + outlen, &v, 4); outlen += 4; }
static void str(const char* s) {
    uint32_t n = strlen(s) + 1;
    u32(n);
    memset(out + outlen, 0, (n + 3) & ~3u);
    memcpy(out + outlen, s, n);
    outlen += (n + 3) & ~3u;
}
static void msg_end(uint32_t* m) { m[1] |= (uint32_t)((uint8_t*)(out + outlen) - (uint8_t*)m) << 16; }
static void flush_fd(int fd) {
    struct iovec io = {out, outlen};
    char ctl[CMSG_SPACE(sizeof(int))];
    struct msghdr mh = {.msg_iov = &io, .msg_iovlen = 1};
    if (fd >= 0) {
        memset(ctl, 0, sizeof ctl);
        mh.msg_control = ctl; mh.msg_controllen = sizeof ctl;
        struct cmsghdr* c = CMSG_FIRSTHDR(&mh);
        c->cmsg_level = SOL_SOCKET; c->cmsg_type = SCM_RIGHTS; c->cmsg_len = CMSG_LEN(sizeof(int));
        memcpy(CMSG_DATA(c), &fd, sizeof fd);
    }
    if (sendmsg(sock, &mh, 0) < 0) { perror("[wlhello] sendmsg"); exit(1); }
    outlen = 0;
}
static void flush(void) { if (outlen) flush_fd(-1); }

/* ---- estado */
static uint32_t reg_id, compositor_g, shm_g, wm_g, seat_g;
static uint32_t compositor, shm, wm, seat, surface, xsurf, toplevel, pointer, keyboard, pool, buffer;
static uint32_t frame_cb, sync_cb;
static int configured, closing, frame_done = 1, synced;
static int pointer_events, buttons, keys, releases, keymap_fd = -1;
static int px = W / 2, py = H / 2;
static uint32_t color = 0x0A84FF;

static void handle(uint32_t obj, uint32_t op, uint32_t* a, int len) {
    if (obj == 1 && op == 0) {                         /* wl_display.error */
        fprintf(stderr, "[wlhello] erro do servidor: obj %u codigo %u: %s\n", a[0], a[1], (char*)&a[3]);
        exit(2);
    }
    if (obj == reg_id && op == 0) {                    /* registry.global */
        uint32_t name = a[0];
        const char* iface = (const char*)&a[2];
        if (!strcmp(iface, "wl_compositor")) compositor_g = name;
        else if (!strcmp(iface, "wl_shm")) shm_g = name;
        else if (!strcmp(iface, "xdg_wm_base")) wm_g = name;
        else if (!strcmp(iface, "wl_seat")) seat_g = name;
    } else if (obj == sync_cb && op == 0) synced = 1;
    else if (obj == wm && op == 0) {                   /* ping -> pong */
        uint32_t* m = msg_begin(wm, 3); u32(a[0]); msg_end(m);
    } else if (obj == xsurf && op == 0) {              /* configure -> ack */
        uint32_t* m = msg_begin(xsurf, 4); u32(a[0]); msg_end(m);
        configured = 1;
    } else if (obj == toplevel && op == 1) closing = 1;
    else if (obj == frame_cb && op == 0) frame_done = 1;
    else if (obj == buffer && op == 0) releases++;
    else if (obj == pointer) {
        pointer_events++;
        if (op == 0) { px = (int32_t)a[2] / 256; py = (int32_t)a[3] / 256; }
        if (op == 2) { px = (int32_t)a[1] / 256; py = (int32_t)a[2] / 256; }
        if (op == 3 && a[3] == 1) { buttons++; color = color == 0x0A84FF ? 0xFF375F : 0x0A84FF; }
    } else if (obj == keyboard) {
        if (op == 3 && a[3] == 1) {
            keys++;
            printf("[wlhello] tecla %u\n", a[2]);
            if (a[2] == 1 || a[2] == 16) closing = 1;                     /* Esc ou Q */
        }
    }
    (void)len;
}

static uint8_t in[16384];
static size_t inlen;

static void dispatch_input(int block) {
    struct pollfd pf = {sock, POLLIN, 0};
    if (poll(&pf, 1, block ? 1000 : 0) <= 0) return;
    struct iovec io = {in + inlen, sizeof in - inlen};
    char ctl[CMSG_SPACE(sizeof(int) * 4)];
    struct msghdr mh = {.msg_iov = &io, .msg_iovlen = 1, .msg_control = ctl, .msg_controllen = sizeof ctl};
    ssize_t n = recvmsg(sock, &mh, 0);
    if (n <= 0) { fprintf(stderr, "[wlhello] servidor fechou a conexao\n"); exit(3); }
    for (struct cmsghdr* c = CMSG_FIRSTHDR(&mh); c; c = CMSG_NXTHDR(&mh, c))
        if (c->cmsg_type == SCM_RIGHTS) { memcpy(&keymap_fd, CMSG_DATA(c), sizeof(int)); close(keymap_fd); }
    inlen += n;
    size_t pos = 0;
    while (inlen - pos >= 8) {
        uint32_t* m = (uint32_t*)(in + pos);
        uint32_t size = m[1] >> 16;
        if (size < 8 || inlen - pos < size) break;
        handle(m[0], m[1] & 0xFFFF, m + 2, size - 8);
        pos += size;
    }
    memmove(in, in + pos, inlen - pos);
    inlen -= pos;
}

static uint32_t wl_bind(uint32_t name, const char* iface, uint32_t ver) {
    uint32_t id = next_id++;
    uint32_t* m = msg_begin(reg_id, 0); u32(name); str(iface); u32(ver); u32(id); msg_end(m);
    return id;
}

static int ball_x, ball_y;   /* centro da bolinha no ultimo draw */

static void draw(uint32_t* px32, int frame) {
    for (int y = 0; y < H; y++)
        for (int x = 0; x < W; x++) {
            uint32_t r = (x * 255 / W + frame) & 0xFF, g = y * 255 / H, b = 160;
            px32[y * W + x] = 0xFF000000 | r << 16 | g << 8 | b;
        }
    /* bolinha que segue o ponteiro (ou anda sozinha) */
    int cx = pointer_events ? px : (W / 2 + (int)(140 * sin(frame / 20.0)));
    int cy = pointer_events ? py : H / 2;
    ball_x = cx; ball_y = cy;
    for (int y = -24; y <= 24; y++)
        for (int x = -24; x <= 24; x++)
            if (x * x + y * y <= 576 && cx + x >= 0 && cx + x < W && cy + y >= 0 && cy + y < H)
                px32[(cy + y) * W + cx + x] = 0xFF000000 | color;
}

int main(int argc, char** argv) {
    int loop = argc > 1 && !strcmp(argv[1], "loop");
    int partial = argc > 1 && !strcmp(argv[1], "parcial");
    if (partial && argc > 2 && !strcmp(argv[2], "loop")) loop = 1;   /* wlhello parcial loop */
    const char* dir = getenv("XDG_RUNTIME_DIR");
    const char* disp = getenv("WAYLAND_DISPLAY");
    struct sockaddr_un sa = {.sun_family = AF_UNIX};
    snprintf(sa.sun_path, sizeof sa.sun_path, "%s/%s", dir ? dir : "/run", disp ? disp : "wayland-0");
    sock = socket(AF_UNIX, SOCK_STREAM | SOCK_CLOEXEC, 0);
    for (int i = 0; connect(sock, (struct sockaddr*)&sa, sizeof sa) != 0; i++) {
        if (i == 50) { fprintf(stderr, "[wlhello] sem servidor Wayland em %s\n", sa.sun_path); return 1; }
        usleep(100000);
    }
    printf("[wlhello] conectado a %s\n", sa.sun_path);

    reg_id = next_id++;
    uint32_t* m = msg_begin(1, 1); u32(reg_id); msg_end(m);           /* get_registry */
    sync_cb = next_id++;
    m = msg_begin(1, 0); u32(sync_cb); msg_end(m);                    /* sync */
    flush();
    while (!synced) dispatch_input(1);
    printf("[wlhello] globais: compositor=%u shm=%u xdg_wm_base=%u seat=%u\n", compositor_g, shm_g, wm_g, seat_g);
    if (!compositor_g || !shm_g || !wm_g) return 4;

    compositor = wl_bind(compositor_g, "wl_compositor", 4);
    shm = wl_bind(shm_g, "wl_shm", 1);
    wm = wl_bind(wm_g, "xdg_wm_base", 2);
    if (seat_g) seat = wl_bind(seat_g, "wl_seat", 5);
    surface = next_id++;
    m = msg_begin(compositor, 0); u32(surface); msg_end(m);
    xsurf = next_id++;
    m = msg_begin(wm, 2); u32(xsurf); u32(surface); msg_end(m);
    toplevel = next_id++;
    m = msg_begin(xsurf, 1); u32(toplevel); msg_end(m);
    m = msg_begin(toplevel, 2); str("Ola, Wayland!"); msg_end(m);
    m = msg_begin(toplevel, 3); str("wlhello"); msg_end(m);
    m = msg_begin(surface, 6); msg_end(m);                            /* commit inicial */
    if (seat) {
        pointer = next_id++; m = msg_begin(seat, 0); u32(pointer); msg_end(m);
        keyboard = next_id++; m = msg_begin(seat, 1); u32(keyboard); msg_end(m);
    }
    flush();
    while (!configured) dispatch_input(1);

    /* buffer numa memoria compartilhada */
    int fd = memfd_create("wlhello", MFD_CLOEXEC);
    size_t size = (size_t)W * H * 4;
    ftruncate(fd, size);
    uint32_t* pixels = mmap(NULL, size, PROT_READ | PROT_WRITE, MAP_SHARED, fd, 0);
    pool = next_id++;
    m = msg_begin(shm, 0); u32(pool); u32(size); msg_end(m);          /* create_pool (fd junto) */
    flush_fd(fd);
    buffer = next_id++;
    m = msg_begin(pool, 0); u32(buffer); u32(0); u32(W); u32(H); u32(W * 4); u32(0); msg_end(m);
    flush();

    struct timespec t0, t1;
    clock_gettime(CLOCK_MONOTONIC, &t0);
    int frames = 0;
    while (!closing && (loop || frames < (partial ? 900 : 120))) {
        if (frame_done) {
            frame_done = 0;
            int ox = ball_x, oy = ball_y;
            draw(pixels, partial ? 0 : frames);
            m = msg_begin(surface, 1); u32(buffer); u32(0); u32(0); msg_end(m);        /* attach */
            if (partial && frames > 0) {                       /* bolinha antiga + nova */
                m = msg_begin(surface, 9); u32(ox - 25); u32(oy - 25); u32(50); u32(50); msg_end(m);
                m = msg_begin(surface, 9); u32(ball_x - 25); u32(ball_y - 25); u32(50); u32(50); msg_end(m);
            } else {
                m = msg_begin(surface, 9); u32(0); u32(0); u32(W); u32(H); msg_end(m); /* damage_buffer */
            }
            frame_cb = next_id++;
            m = msg_begin(surface, 3); u32(frame_cb); msg_end(m);                      /* frame */
            m = msg_begin(surface, 6); msg_end(m);                                     /* commit */
            flush();
            frames++;
        }
        dispatch_input(1);
    }
    clock_gettime(CLOCK_MONOTONIC, &t1);
    long ms = (t1.tv_sec - t0.tv_sec) * 1000 + (t1.tv_nsec - t0.tv_nsec) / 1000000;
    printf("[wlhello] %d quadros em %ld ms (%ld fps); buffers devolvidos: %d\n", frames, ms, ms ? frames * 1000L / ms : 0, releases);
    printf("[wlhello] eventos: ponteiro %d, botoes %d, teclas %d, keymap %s\n", pointer_events, buttons, keys,
           keymap_fd >= 0 ? "recebido" : "nao");
    int ok = frames >= (partial ? 900 : 120) || closing;
    printf("[wlhello] fim: %s\n", ok && releases > 0 && keymap_fd >= 0 ? "ok" : "FALHA");
    return ok ? 0 : 1;
}
