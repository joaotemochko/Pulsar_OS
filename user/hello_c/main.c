/* hello_c — o menor app grafico em C para o Pulsar OS. Use-o como modelo:
 * registra uma janela no WM, desenha na propria superficie e reage aos
 * eventos (clique, tecla, tema). Abra pelo Terminal com:  open hello_c */
#include "pulsar.h"

int main(void){
    App app;
    app_init(&app, 99, "Olá, Pulsar", 360, 200);
    int clicks = 0;
    WmEvent e; e.changed = 1; e.close = 0; e.click = 0; e.more = 0;
    for(;;){
        if(e.close) sys_exit();
        if(e.click) clicks++;
        if(e.changed || e.click){
            Fb* fb = &app.fb;
            fb_fill(fb, rect(0, 0, app.w, app.h), app.th.win_bg);
            text_center(fb, &FONT_TITLE, rect(0, 40, app.w, 40), "Olá, Pulsar!", app.th.text);
            char b[48]; char* p = fmt_s(b, "cliques: "); fmt_u(p, (u64)clicks);
            text_center(fb, &FONT_UI, rect(0, 90, app.w, 24), b, app.th.text2);
            fb_round_rect(fb, rect(app.w / 2 - 60, 130, 120, 32), 8, app.th.accent);
            text_center(fb, &FONT_UI_BOLD, rect(app.w / 2 - 60, 130, 120, 32), "Clique aqui", 0xFFFFFF);
            e = app_commit(&app);
        } else e = app_poll(&app);
    }
    return 0;
}
