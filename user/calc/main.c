/* calc.c — calculadora do Pulsar em C. App cliente do WM: superficie propria,
 * recebe cliques locais (ev.mx/ev.my) e desenha um teclado estilo macOS. */
#include "pulsar.h"

#define COL_BG      0x001E2530
#define COL_DISPLAY 0x00121821
#define COL_TXT     0x00F0F4FA
#define COL_OP      0x00FF9F0A   /* laranja (operadores) */
#define COL_OP_TXT  0x00FFFFFF
#define COL_NUM     0x00323A47   /* cinza escuro (numeros) */
#define COL_NUM_TXT 0x00F0F4FA
#define COL_FN      0x004A5262   /* funcoes (C, +/-, %) */

/* estado da calculadora */
static i64 acc = 0;        /* acumulador */
static i64 cur = 0;        /* numero sendo digitado */
static int has_cur = 0;    /* ha digito atual? */
static char pend = 0;      /* operador pendente: + - * / ou 0 */
static int just_eq = 0;    /* acabou de calcular '='? */

/* layout: grade 4 colunas x 5 linhas de botoes */
#define COLS 4
#define ROWS 5
#define PAD 12
#define GAP 10

/* rotulos dos botoes (linha a linha) */
static const char* LABELS[ROWS][COLS] = {
    {"C", "+/-", "%", "/"},
    {"7", "8", "9", "*"},
    {"4", "5", "6", "-"},
    {"1", "2", "3", "+"},
    {"0", "", ".", "="},   /* "0" ocupa 2 colunas; "" e placeholder */
};

/* converte i64 -> string decimal (com sinal). retorna comprimento. */
static int i64_to_str(i64 v, char* buf){
    int n=0; int neg = v<0;
    u64 x = neg ? (u64)(-v) : (u64)v;
    char tmp[24]; int t=0;
    if(x==0){ tmp[t++]='0'; }
    while(x>0){ tmp[t++]='0'+(int)(x%10); x/=10; }
    if(neg) buf[n++]='-';
    while(t>0) buf[n++]=tmp[--t];
    buf[n]=0;
    return n;
}

/* aplica o operador pendente: acc = acc (op) cur */
static void apply_pending(void){
    if(pend=='+') acc += cur;
    else if(pend=='-') acc -= cur;
    else if(pend=='*') acc *= cur;
    else if(pend=='/'){ if(cur!=0) acc /= cur; else acc = 0; }
    else acc = cur;   /* sem operador: acc recebe cur */
}

/* processa o rotulo de um botao */
static void press(const char* s){
    char c = s[0];
    if(c>='0' && c<='9' && s[1]==0){
        if(just_eq){ acc=0; pend=0; just_eq=0; }
        cur = cur*10 + (c-'0');
        has_cur = 1;
    } else if(c=='C'){
        acc=0; cur=0; has_cur=0; pend=0; just_eq=0;
    } else if(c=='+' && s[1]=='/'){       /* +/- : troca sinal */
        cur = -cur;
    } else if(c=='%'){
        cur = cur/100;
    } else if(c=='+'||c=='-'||c=='*'||c=='/'){
        if(has_cur){ apply_pending(); }
        pend = c; cur = 0; has_cur = 0; just_eq = 0;
    } else if(c=='='){
        if(has_cur || pend){ apply_pending(); }
        cur = acc; pend = 0; has_cur = 0; just_eq = 1;
    }
    /* "." ignorado (calculadora inteira por ora) */
}

/* qual valor mostrar no display */
static i64 display_val(void){
    if(has_cur || just_eq) return cur;
    if(pend) return acc;
    return has_cur ? cur : acc;
}

int main(void){
    u64 wm; { i64 p; do { p=sys_lookup(SVC_WM); if(p<0) sys_yield(); } while(p<0); wm=(u64)p; }
    u64 slot; u32 cw=280, ch=400;
    wm_hello(wm,3,&slot,&cw,&ch);   /* kind 3 = calculadora */
    Fb surf=surface_open(slot);

    /* geometria dos botoes (recalculada do tamanho da superficie) */
    i32 disp_h = 92;
    i32 bx0 = PAD, by0 = disp_h + PAD;
    i32 bw = (cw - 2*PAD - (COLS-1)*GAP) / COLS;
    i32 bh = (ch - by0 - PAD - (ROWS-1)*GAP) / ROWS;

    for(;;){
        WmEvent ev=wm_poll(wm);
        if(ev.close) sys_exit();
        if(ev.menu==APP_MENU_CLEAR){ acc=0;cur=0;has_cur=0;pend=0;just_eq=0; }

        /* clique local -> qual botao? */
        if(ev.click && ev.mx>=0 && ev.my>=0){
            i32 mx=ev.mx, my=ev.my;
            for(int r=0;r<ROWS;r++){
                for(int col=0;col<COLS;col++){
                    const char* lab=LABELS[r][col];
                    if(!lab[0]) continue;         /* placeholder */
                    i32 x = bx0 + col*(bw+GAP);
                    i32 y = by0 + r*(bh+GAP);
                    i32 wbtn = bw;
                    if(r==ROWS-1 && col==0) wbtn = bw*2 + GAP; /* "0" largo */
                    if(mx>=x && mx<x+wbtn && my>=y && my<y+bh){
                        press(lab);
                    }
                }
            }
        }

        /* ---- desenho ---- */
        fb_fill(&surf,0,0,cw,ch,COL_BG);
        /* display */
        Rect dr = { PAD, PAD, cw-2*PAD, disp_h-PAD };
        fb_round_rect(&surf, dr, 14, COL_DISPLAY);
        char db[24]; int dn=i64_to_str(display_val(), db);
        i32 tw=text_ui_width(db);
        text_ui(&surf, cw-PAD-16-tw, PAD+34, db, COL_TXT);

        /* botoes */
        for(int r=0;r<ROWS;r++){
            for(int col=0;col<COLS;col++){
                const char* lab=LABELS[r][col];
                if(!lab[0]) continue;
                i32 x = bx0 + col*(bw+GAP);
                i32 y = by0 + r*(bh+GAP);
                i32 wbtn = bw;
                if(r==ROWS-1 && col==0) wbtn = bw*2 + GAP;
                char c=lab[0];
                u32 bg, fg;
                if(c=='='||((c=='+'||c=='-'||c=='*'||c=='/')&&lab[1]==0)){ bg=COL_OP; fg=COL_OP_TXT; }
                else if(c=='C'||c=='%'||(c=='+'&&lab[1]=='/')){ bg=COL_FN; fg=COL_NUM_TXT; }
                else { bg=COL_NUM; fg=COL_NUM_TXT; }
                Rect br={x,y,wbtn,bh};
                fb_round_rect(&surf, br, 12, bg);
                i32 lw=text_ui_width(lab);
                text_ui(&surf, x+wbtn/2-lw/2, y+bh/2-8, lab, fg);
            }
        }
        wm_commit(wm,slot);
        sys_yield();
    }
    return 0;
}
