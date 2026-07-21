/* netmon.c — Monitor de Rede do Pulsar (app nativo em C).
 * Mostra o status da rede (IP, online/offline) e envia pings UDP
 * ao clicar no botao, usando as syscalls de rede do kernel. */
#include "pulsar.h"

#define COL_BG    0x001A2230
#define COL_CARD  0x0022303F
#define COL_TXT   0x00E4ECF5
#define COL_DIM   0x008494A8
#define COL_OK    0x0032D27A   /* verde online */
#define COL_OFF   0x00FF5F57   /* vermelho offline */
#define COL_BTN   0x00579BFF    /* azul acao */
#define COL_BTN_TX 0x00FFFFFF

static int i2s(u64 v, char* b){
    char t[24]; int n=0,ti=0;
    if(v==0){t[ti++]='0';}
    while(v>0){t[ti++]='0'+(int)(v%10);v/=10;}
    while(ti>0) b[n++]=t[--ti];
    b[n]=0; return n;
}
/* monta "a.b.c.d" a partir do IP empacotado */
static int ip2s(u32 ip, char* b){
    int n=0;
    for(int i=3;i>=0;i--){
        u32 oct=(ip>>(i*8))&0xFF;
        n+=i2s(oct,b+n);
        if(i>0) b[n++]='.';
    }
    b[n]=0; return n;
}

int main(void){
    u64 wm; { i64 p; do { p=sys_lookup(SVC_WM); if(p<0) sys_yield(); } while(p<0); wm=(u64)p; }
    u64 slot; u32 cw=420, ch=340;
    wm_hello(wm,6,&slot,&cw,&ch);   /* kind 6 = netmon */
    Fb surf=surface_open(slot);

    int pings=0;
    u32 last_ip=0;

    /* botao "Enviar Ping" */
    Rect btn = { 40, 250, cw-80, 52 };

    for(;;){
        WmEvent ev=wm_poll(wm);
        if(ev.close) sys_exit();

        u32 ip = (u32)sys_net_status();
        last_ip = ip;

        /* clique no botao -> envia UDP ao gateway (10.0.2.2:9) */
        if(ev.click && ev.mx>=0 && ev.my>=0){
            if(ev.mx>=btn.x && ev.mx<btn.x+btn.w && ev.my>=btn.y && ev.my<btn.y+btn.h && ip){
                const char* msg="ping do Pulsar OS";
                int n=0; while(msg[n]) n++;
                u32 gw = (10<<24)|(0<<16)|(2<<8)|2;   /* 10.0.2.2 */
                if(sys_net_udp_send(gw, 9, 40000, msg, n)) pings++;
            }
        }

        /* ---- desenho ---- */
        fb_fill(&surf,0,0,cw,ch,COL_BG);
        /* titulo */
        text_ui(&surf, 40, 26, "Monitor de Rede", COL_TXT);

        /* card de status */
        Rect card = { 40, 66, cw-80, 150 };
        fb_round_rect(&surf, card, 14, COL_CARD);

        int online = ip!=0;
        /* indicador (disco) */
        fb_disc(&surf, 68, 100, 8, online?COL_OK:COL_OFF);
        text_ui(&surf, 88, 92, online?"Online":"Offline", online?COL_OK:COL_OFF);

        /* linha IP */
        text_ui(&surf, 64, 130, "Endereco IP:", COL_DIM);
        if(online){
            char ips[20]; ip2s(ip, ips);
            text_ui(&surf, 200, 130, ips, COL_TXT);
        } else {
            text_ui(&surf, 200, 130, "---", COL_DIM);
        }
        /* linha gateway */
        text_ui(&surf, 64, 158, "Gateway:", COL_DIM);
        text_ui(&surf, 200, 158, "10.0.2.2", COL_TXT);
        /* pings enviados */
        text_ui(&surf, 64, 186, "Pings enviados:", COL_DIM);
        { char pb[12]; i2s(pings,pb); text_ui(&surf, 200, 186, pb, COL_TXT); }

        /* botao */
        u32 bcol = online?COL_BTN:0x00404A58;
        fb_round_rect(&surf, btn, 12, bcol);
        const char* bl = "Enviar Ping UDP";
        i32 blw=text_ui_width(bl);
        text_ui(&surf, btn.x+btn.w/2-blw/2, btn.y+btn.h/2-8, bl, COL_BTN_TX);

        wm_commit(wm,slot);
        sys_yield();
    }
    return 0;
}
