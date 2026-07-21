/* vela.c — Vela Pulsar: o navegador do Pulsar OS (em C).
 * Faz HTTP GET via syscall, extrai o texto do HTML e o exibe.
 * Sem DNS ainda: navega por IP (barra mostra o alvo). */
#include "pulsar.h"

#define COL_BG     0x00FFFFFF
#define COL_CHROME 0x00F0F2F6
#define COL_BAR    0x00FFFFFF
#define COL_TXT    0x001A2028
#define COL_DIM    0x008494A8
#define COL_LINK   0x00337AE6
#define COL_GO     0x00579BFF
#define COL_STATUS 0x00E8ECF2

/* buffer de rede: [0..512] cabecalho de req, [512..] corpo recebido */
static u8 netbuf[16384];
/* texto extraido do HTML para exibicao */
static char page[8192];
static int page_len=0;
static char status[64];

static int slen(const char* s){ int n=0; while(s[n])n++; return n; }
static void sset(char* d, const char* s){ int i=0; while(s[i]){d[i]=s[i];i++;} d[i]=0; }

/* extrai texto do HTML: pula tags <...>, colapsa espacos, decodifica poucas entidades */
static void render_html(const u8* body, int len){
    page_len=0;
    int in_tag=0, in_script=0;
    int last_space=1;
    /* achar inicio do corpo (apos \r\n\r\n) */
    int start=0;
    for(int i=0;i+3<len;i++){
        if(body[i]=='\r'&&body[i+1]=='\n'&&body[i+2]=='\r'&&body[i+3]=='\n'){ start=i+4; break; }
    }
    for(int i=start;i<len && page_len<8000;i++){
        u8 c=body[i];
        /* detectar <script> e </script> grosseiramente */
        if(c=='<'){
            in_tag=1;
            /* olhar se e script/style */
            if(i+7<len){
                if((body[i+1]=='s'||body[i+1]=='S')&&(body[i+2]=='c'||body[i+2]=='C')) in_script=1;
                if((body[i+1]=='s'||body[i+1]=='S')&&(body[i+2]=='t'||body[i+2]=='T')) in_script=1;
            }
            if(body[i+1]=='/') in_script=0;
            continue;
        }
        if(c=='>'){ in_tag=0; continue; }
        if(in_tag||in_script) continue;
        /* fora de tag: texto */
        if(c=='\n'||c=='\r'||c=='\t'||c==' '){
            if(!last_space){ page[page_len++]=' '; last_space=1; }
        } else {
            page[page_len++]=(char)c; last_space=0;
        }
    }
    page[page_len]=0;
}

int main(void){
    u64 wm; { i64 p; do { p=sys_lookup(SVC_WM); if(p<0) sys_yield(); } while(p<0); wm=(u64)p; }
    u64 slot; u32 cw=720, ch=520;
    wm_hello(wm,7,&slot,&cw,&ch);   /* kind 7 = vela */
    Fb surf=surface_open(slot);

    sset(status, "Pronto. Clique em Ir para carregar.");

    /* alvo inicial: example.com (93.184.216.34) por IP */
    u32 target_ip = (10<<24)|(0<<16)|(2<<8)|2;   /* host via SLIRP */
    const char* target_host = "pulsar.local";
    const char* target_path = "/";

    Rect gobtn = { cw-90, 12, 74, 34 };
    int scroll=0;

    for(;;){
        WmEvent ev=wm_poll(wm);
        if(ev.close) sys_exit();
        if(ev.menu==APP_MENU_CLEAR){ page_len=0; page[0]=0; sset(status,"Limpo."); }

        if(ev.click && ev.mx>=0 && ev.my>=0){
            /* botao Ir */
            if(ev.mx>=gobtn.x && ev.mx<gobtn.x+gobtn.w && ev.my>=gobtn.y && ev.my<gobtn.y+gobtn.h){
                sset(status,"Conectando...");
                /* montar cabecalho da req no netbuf */
                int hl=slen(target_host), pl=slen(target_path);
                netbuf[0]=(hl>>8)&0xFF; netbuf[1]=hl&0xFF;
                netbuf[2]=(pl>>8)&0xFF; netbuf[3]=pl&0xFF;
                for(int i=0;i<hl;i++) netbuf[4+i]=target_host[i];
                for(int i=0;i<pl;i++) netbuf[4+hl+i]=target_path[i];
                int n=sys_http_get(target_ip, 8080, netbuf);
                if(n>0){
                    render_html(netbuf+512, n);
                    scroll=0;
                    char st[64]; sset(st,"Carregado: "); 
                    /* anexar contagem de bytes */
                    int p=slen(st); int v=n; char tmp[8]; int t=0;
                    if(v==0)tmp[t++]='0'; while(v>0){tmp[t++]='0'+v%10;v/=10;}
                    while(t>0)st[p++]=tmp[--t]; sset(st+p," bytes");
                    sset(status,st);
                } else {
                    sset(status,"Falha na conexao.");
                }
            }
        }

        /* ---- desenho ---- */
        fb_fill(&surf,0,0,cw,ch,COL_BG);

        /* barra de ferramentas (chrome) */
        fb_fill(&surf,0,0,cw,58,COL_CHROME);
        /* campo de endereco */
        Rect addr={12,12,cw-114,34};
        fb_round_rect(&surf,addr,10,COL_BAR);
        /* icone de cadeado/globo simples */
        fb_disc(&surf,30,29,5,COL_DIM);
        char url[80]; sset(url,"http://"); sset(url+7,target_host);
        text_ui(&surf,46,20,url,COL_TXT);
        /* botao Ir */
        fb_round_rect(&surf,gobtn,9,COL_GO);
        text_ui(&surf,gobtn.x+gobtn.w/2-8,gobtn.y+9,"Ir",0x00FFFFFF);

        /* barra de status */
        fb_fill(&surf,0,58,cw,24,COL_STATUS);
        text_ui_a(&surf,14,62,status,COL_DIM,230);

        /* area de conteudo: renderiza o texto da pagina com quebra de linha */
        i32 cx=16, cy=94 - scroll, maxw=cw-32;
        i32 lineh=20;
        int i=0;
        char word[128];
        while(i<page_len && cy<(i32)ch){
            /* extrai uma palavra */
            int w=0;
            while(i<page_len && page[i]!=' ' && w<127){ word[w++]=page[i++]; }
            word[w]=0;
            if(i<page_len && page[i]==' ') i++;
            if(w==0) continue;
            i32 ww=text_ui_width(word);
            if(cx+ww>16+maxw){ cx=16; cy+=lineh; }
            if(cy>=88 && cy<(i32)ch-4) text_ui(&surf,cx,cy,word,COL_TXT);
            cx+=ww+text_ui_width(" ");
        }
        if(page_len==0){
            text_ui_a(&surf,16,110,"Vela Pulsar — o navegador do Pulsar OS",COL_DIM,220);
            text_ui_a(&surf,16,136,"Clique em Ir para baixar example.com via HTTP.",COL_DIM,200);
        }

        wm_commit(wm,slot);
        sys_yield();
    }
    return 0;
}
