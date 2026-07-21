/* terminal.c — app de terminal do Pulsar em C. Desenha na sua superfície,
 * aceita digitação (via WM), e interpreta comandos simples. */
#include "pulsar.h"

#define MAXLINE 128
#define MAXHIST 14
#define COL_BG    0x001A2230
#define COL_FG    0x00D8E4F0
#define COL_PROMPT 0x0057D497
#define COL_DIM   0x007C8697

/* keymap evdev -> ascii (simplificado, minúsculas) */
static char keymap(u16 code){
    static const char* row1="1234567890";
    static const char* row2="qwertyuiop";
    static const char* row3="asdfghjkl";
    static const char* row4="zxcvbnm";
    if(code>=2 && code<=11) return row1[code-2];
    if(code>=16 && code<=25) return row2[code-16];
    if(code>=30 && code<=38) return row3[code-30];
    if(code>=44 && code<=50) return row4[code-44];
    if(code==57) return ' ';
    if(code==52) return '.';
    if(code==12) return '-';
    return 0;
}

static char hist[MAXHIST][MAXLINE];
static int  hist_len[MAXHIST];
static int  nhist=0;
static char line[MAXLINE];
static int  linelen=0;

static void push_hist(const char* s, int n){
    if(nhist>=MAXHIST){
        for(int i=1;i<MAXHIST;i++){ memcpy(hist[i-1],hist[i],MAXLINE); hist_len[i-1]=hist_len[i]; }
        nhist=MAXHIST-1;
    }
    int k=n<MAXLINE?n:MAXLINE-1;
    memcpy(hist[nhist],s,k); hist_len[nhist]=k; nhist++;
}

static int streq(const char* a, const char* b, int n){
    for(int i=0;i<n;i++) if(a[i]!=b[i]) return 0;
    return 1;
}

/* interpreta o comando digitado, empurrando saída no histórico */
static void run_cmd(void){
    push_hist(line, linelen);  /* eco do comando com prompt embutido no draw */
    if(linelen==0){ return; }
    if(linelen>=4 && streq(line,"help",4)){
        push_hist("comandos: help, echo, clear, ver, files", 40);
    } else if(linelen>=5 && streq(line,"clear",5)){
        nhist=0;
    } else if(linelen>=4 && streq(line,"echo",4)){
        int s=4; while(s<linelen && line[s]==' ') s++;
        push_hist(line+s, linelen-s);
    } else if(linelen>=3 && streq(line,"ver",3)){
        push_hist("Pulsar OS - terminal em C (libc minima)", 40);
    } else if(linelen>=5 && streq(line,"files",5)){
        /* conta arquivos via IPC do fsd */
        i64 fs=sys_lookup(SVC_FS);
        if(fs>=0){
            Msg m; memset(&m,0,sizeof m); m.tag=FS_OP_COUNT; ipc_send((u64)fs,&m);
            char buf[32]; int p=0;
            const char* pre="arquivos no disco: ";
            for(const char* q=pre;*q;q++) buf[p++]=*q;
            u64 n=m.data[0]; if(n==0) buf[p++]='0';
            else { char tmp[8]; int t=0; while(n){tmp[t++]='0'+n%10;n/=10;} while(t) buf[p++]=tmp[--t]; }
            push_hist(buf,p);
        }
    } else {
        push_hist("comando desconhecido (tente 'help')", 35);
    }
}

int main(void){
    puts_("[terminal] iniciando (C, superficie)\n");
    u64 wm; { i64 p; do { p=sys_lookup(SVC_WM); if(p<0) sys_yield(); } while(p<0); wm=(u64)p; }
    u64 slot; u32 cw=560, ch=380;
    wm_hello(wm,5,&slot,&cw,&ch);   /* kind 5 = terminal */
    Fb surf=surface_open(slot);

    push_hist("Pulsar Terminal - digite 'help'", 31);

    u32 blink=0;
    for(;;){
        WmEvent ev=wm_poll(wm);
        if(ev.close) sys_exit();
        /* acoes vindas da barra de menu do WM */
        if(ev.menu==APP_MENU_CLEAR){ nhist=0; push_hist("(limpo pelo menu)",17); }
        else if(ev.menu==APP_MENU_PASTE){ push_hist("colar: buffer vazio",19); }
        else if(ev.menu==APP_MENU_COPY){ push_hist("copiar: ok",10); }
        if(ev.key){
            if(ev.key==28){ run_cmd(); linelen=0; }        /* enter */
            else if(ev.key==14){ if(linelen>0) linelen--; } /* backspace */
            else { char c=keymap(ev.key); if(c && linelen<MAXLINE-1) line[linelen++]=c; }
        }

        /* desenha o terminal na superfície */
        fb_fill(&surf,0,0,cw,ch,COL_BG);
        i32 y=12;
        for(int i=0;i<nhist;i++){
            /* linhas do histórico; a última pode ser eco de comando */
            char tmp[MAXLINE+1];
            int n=hist_len[i]; if(n>MAXLINE)n=MAXLINE;
            memcpy(tmp,hist[i],n); tmp[n]=0;
            text_ui_a(&surf,12,y,tmp,COL_FG,230);
            y+=22;
        }
        /* linha de prompt atual */
        text_ui(&surf,12,y,"pulsar$",COL_PROMPT);
        char cur[MAXLINE+1]; memcpy(cur,line,linelen); cur[linelen]=0;
        i32 px=12+text_ui_width("pulsar$")+8;
        px+=text_ui(&surf,px,y,cur,COL_FG);
        /* cursor piscando */
        blink++;
        if((blink/20)%2==0) fb_fill(&surf,px+2,y+2,2,16,COL_FG);

        wm_commit(wm,slot);
        sys_yield();
    }
    return 0;
}
