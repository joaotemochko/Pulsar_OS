/* netmon.c — Monitor de Rede do Pulsar OS (C). Mostra o estado da rede
 * virtio-net (IP, gateway, DNS do SLIRP) e envia pings UDP de teste. */
#include "pulsar.h"

static App app;
static u32 ip = 0;
static int pings = 0, fails = 0;
static u64 last_ping = 0;

static void ip2s(u32 v, char* b){
    char* p = b;
    for(int i = 3; i >= 0; i--){ p = fmt_u(p, (v >> (i * 8)) & 0xFF); if(i) *p++ = '.'; }
    *p = 0;
}

static Rect btn_ping(void){ return rect(24, app.h - 58, 170, 34); }
static Rect btn_ref(void){ return rect(206, app.h - 58, 120, 34); }

static void row(Fb* fb, i32 y, const char* k, const char* v, const Theme* th){
    text_draw(fb, &FONT_UI, 40, y, k, th->text2);
    text_right(fb, &FONT_UI, app.w - 40, y, v, th->text);
}

static void draw(i32 mx, i32 my){
    Fb* fb = &app.fb;
    const Theme* th = &app.th;
    fb_fill(fb, rect(0, 0, app.w, app.h), th->dark ? 0x1C1D21 : 0xF5F5F7);
    /* cabecalho */
    int up = ip != 0;
    fb_disc(fb, 46, 46, 22, up ? 0x30C552 : 0xFF453A, 255);
    if(up){ fb_line(fb, 36, 47, 43, 54, 4, 0xFFFFFF, 255); fb_line(fb, 43, 54, 57, 38, 4, 0xFFFFFF, 255); }
    else { fb_line(fb, 38, 38, 54, 54, 4, 0xFFFFFF, 255); fb_line(fb, 54, 38, 38, 54, 4, 0xFFFFFF, 255); }
    text_draw(fb, &FONT_TITLE, 82, 22, up ? "Conectado" : "Sem rede", th->text);
    text_draw(fb, &FONT_SMALL, 82, 52, "virtio-net · Ethernet (QEMU user/SLIRP)", th->text2);
    /* cartao de detalhes */
    Rect card = rect(24, 92, app.w - 48, 4 * 30 + 16);
    fb_round_rect(fb, card, 10, th->dark ? 0x26272D : 0xFFFFFF);
    fb_round_border(fb, card, 10, th->dark ? 0xFFFFFF : 0x000000, th->dark ? 18 : 22);
    char b[32];
    if(up) ip2s(ip, b); else fmt_s(b, "-");
    row(fb, card.y + 12, "Endereço IP", b, th);
    fb_hline(fb, card.x + 16, card.y + 38, card.w - 32, th->sep, 255);
    row(fb, card.y + 44, "Gateway", up ? "10.0.2.2" : "-", th);
    fb_hline(fb, card.x + 16, card.y + 70, card.w - 32, th->sep, 255);
    row(fb, card.y + 76, "DNS", up ? "10.0.2.3" : "-", th);
    fb_hline(fb, card.x + 16, card.y + 102, card.w - 32, th->sep, 255);
    char* p = fmt_u(b, (u64)pings); p = fmt_s(p, " enviados");
    if(fails){ p = fmt_s(p, ", "); p = fmt_u(p, (u64)fails); fmt_s(p, " falhas"); }
    row(fb, card.y + 108, "Pings UDP", b, th);
    if(last_ping && sys_time_ms() - last_ping < 1500)
        text_draw(fb, &FONT_SMALL, 24, app.h - 82, "Datagrama enviado ao gateway (porta 9).", th->accent);
    Rect bp = btn_ping(), br = btn_ref();
    ui_button(fb, bp, "Enviar ping UDP", th, 1, rect_has(bp, mx, my));
    ui_button(fb, br, "Atualizar", th, 0, rect_has(br, mx, my));
}

static void ping(void){
    const char msg[] = "Pulsar OS ping";
    if(sys_net_udp_send((10 << 24) | (0 << 16) | (2 << 8) | 2, 9, 40001, msg, sizeof msg - 1)) pings++;
    else fails++;
    last_ping = sys_time_ms();
}

int main(void){
    app_init(&app, 6, "Rede", 420, 330);
    ip = (u32)sys_net_status();
    u64 tick = sys_time_ms();
    i32 lmx = -1, lmy = -1;
    draw(-1, -1);
    WmEvent e = app_commit(&app);
    for(;;){
        if(e.close) sys_exit();
        int dirty = e.changed;
        if(e.click){
            if(rect_has(btn_ping(), e.mx, e.my)){ ping(); dirty = 1; }
            if(rect_has(btn_ref(), e.mx, e.my)){ ip = (u32)sys_net_status(); dirty = 1; }
        }
        if(e.menu == APP_MENU_ADD){ ping(); dirty = 1; }
        if(e.menu == APP_MENU_RELOAD){ ip = (u32)sys_net_status(); dirty = 1; }
        if(e.key == KEY_ENTER){ ping(); dirty = 1; }
        int hov = rect_has(btn_ping(), e.mx, e.my) * 1 + rect_has(btn_ref(), e.mx, e.my) * 2;
        int lhov = rect_has(btn_ping(), lmx, lmy) * 1 + rect_has(btn_ref(), lmx, lmy) * 2;
        if(hov != lhov) dirty = 1;
        lmx = e.mx; lmy = e.my;
        if(sys_time_ms() - tick > 2000){ tick = sys_time_ms(); u32 n = (u32)sys_net_status(); if(n != ip){ ip = n; dirty = 1; } }
        if(last_ping && sys_time_ms() - last_ping > 1500 && sys_time_ms() - last_ping < 1600) dirty = 1;
        if(e.more){ e = app_poll(&app); continue; }
        if(dirty){ draw(e.mx, e.my); e = app_commit(&app); }
        else e = app_poll(&app);
    }
    return 0;
}
