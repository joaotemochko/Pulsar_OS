/* calc.c — Calculadora do Pulsar OS (C). Ponto fixo com 6 casas decimais,
 * formatacao brasileira (1.234,56), teclado completo e visual no estilo
 * macOS que acompanha o tema claro/escuro. */
#include "pulsar.h"

#define SCALE 1000000L      /* 6 casas decimais */
#define ROWS 5
#define COLS 4

static App app;
static i64 acc = 0;              /* acumulador (ponto fixo) */
static char ent[24]; static int entn = 0, ent_neg = 0;   /* numero sendo digitado */
static i64 shown = 0;            /* valor exibido quando nao ha digitacao */
static char pend = 0;
static int error = 0;
static int pressed = -1; static u64 pressed_at = 0;

static const char* LABELS[ROWS][COLS] = {
    {"AC", "+/-", "%", "÷"},
    {"7", "8", "9", "×"},
    {"4", "5", "6", "-"},
    {"1", "2", "3", "+"},
    {"0", "", ",", "="},
};

static i64 mul(i64 a, i64 b){ return (a / SCALE) * b + (a % SCALE) * b / SCALE; }
static i64 divi(i64 a, i64 b){
    if(b == 0){ error = 1; return 0; }
    i64 q = a / b, r = a % b;
    return q * SCALE + r * SCALE / b;
}

/* valor da entrada digitada */
static i64 entry(void){
    i64 ip = 0, fp = 0, p = SCALE; int frac = 0;
    for(int i = 0; i < entn; i++){
        if(ent[i] == ','){ frac = 1; continue; }
        if(!frac) ip = ip * 10 + (ent[i] - '0');
        else if(p > 1){ p /= 10; fp += (ent[i] - '0') * p; }
    }
    i64 v = ip * SCALE + fp;
    return ent_neg ? -v : v;
}

static void apply(i64 cur){
    switch(pend){
    case '+': acc += cur; break;
    case '-': acc -= cur; break;
    case '*': acc = mul(acc, cur); break;
    case '/': acc = divi(acc, cur); break;
    default: acc = cur;
    }
    shown = acc;
}

static void press(const char* s){
    char c = s[0];
    if(error && c != 'A') return;
    if(c >= '0' && c <= '9'){
        int digits = 0; for(int i = 0; i < entn; i++) if(ent[i] != ',') digits++;
        if(digits >= 15) return;
        if(entn == 1 && ent[0] == '0'){ if(c == '0') return; entn = 0; }
        ent[entn++] = c;
    } else if(c == ','){
        for(int i = 0; i < entn; i++) if(ent[i] == ',') return;
        if(entn == 0) ent[entn++] = '0';
        ent[entn++] = ',';
    } else if(c == 'A'){
        acc = shown = 0; entn = 0; ent_neg = 0; pend = 0; error = 0;
    } else if(c == '+' && s[1] == '/'){
        if(entn) ent_neg = !ent_neg; else { shown = -shown; acc = shown; }
    } else if(c == '%'){
        i64 v = (entn ? entry() : shown) / 100;
        entn = 0; ent_neg = 0; shown = v;
        if(!pend) acc = v; else { apply(v); pend = 0; }
    } else if(c == '='){
        if(entn){ apply(entry()); entn = 0; ent_neg = 0; }
        else if(pend) apply(shown);
        pend = 0;
    } else {
        char op = c == '+' ? '+' : c == '-' ? '-' : ((u8)s[1] == 0x97 ? '*' : '/');
        if(entn){ apply(entry()); entn = 0; ent_neg = 0; }
        else acc = shown;
        pend = op;
    }
}

/* pt-BR: milhar com '.', decimal com ',' */
static void group(const char* digits, int n, char** p){
    for(int i = 0; i < n; i++){
        *(*p)++ = digits[i];
        if(n - 1 - i > 0 && (n - 1 - i) % 3 == 0) *(*p)++ = '.';
    }
}

static void display(char* out){
    char* p = out;
    if(entn){
        if(ent_neg) *p++ = '-';
        int ip = 0; while(ip < entn && ent[ip] != ',') ip++;
        group(ent, ip, &p);
        for(int i = ip; i < entn; i++) *p++ = ent[i];
        *p = 0; return;
    }
    i64 v = shown; if(v < 0){ *p++ = '-'; v = -v; }
    char d[24]; int n = 0; u64 ipart = (u64)v / SCALE, fp = (u64)v % SCALE;
    if(ipart == 0) d[n++] = '0';
    char t[24]; int tn = 0; while(ipart){ t[tn++] = '0' + ipart % 10; ipart /= 10; }
    while(tn) d[n++] = t[--tn];
    group(d, n, &p);
    char fr[6]; for(int i = 5; i >= 0; i--){ fr[i] = '0' + fp % 10; fp /= 10; }
    int last = 5; while(last >= 0 && fr[last] == '0') last--;
    if(last >= 0){ *p++ = ','; for(int i = 0; i <= last; i++) *p++ = fr[i]; }
    *p = 0;
}

static Rect button_rect(int r, int c, i32* wbtn){
    i32 pad = 12, gap = 10, disp = app.h * 30 / 100;
    i32 bw = (app.w - 2 * pad - (COLS - 1) * gap) / COLS;
    i32 bh = (app.h - disp - pad - (ROWS - 1) * gap) / ROWS;
    *wbtn = (r == ROWS - 1 && c == 0) ? bw * 2 + gap : bw;
    return rect(pad + c * (bw + gap), disp + r * (bh + gap), *wbtn, bh);
}

static void draw(i32 mx, i32 my){
    Fb* fb = &app.fb;
    const Theme* th = &app.th;
    int dark = th->dark;
    fb_fill(fb, rect(0, 0, app.w, app.h), dark ? 0x1C1C1E : 0xEDEDF0);
    /* visor */
    char txt[48];
    if(error) fmt_s(txt, "Erro");
    else display(txt);
    const Font* f = &FONT_HUGE;
    if(text_width(f, txt) > app.w - 32) f = &FONT_TITLE;
    i32 disp = app.h * 30 / 100;
    text_right(fb, f, app.w - 18, disp - f->height - 12, txt, dark ? 0xFFFFFF : 0x1D1D1F);
    if(pend){
        char op[4] = {0}; const char* s = pend == '+' ? "+" : pend == '-' ? "-" : pend == '*' ? "×" : "÷";
        fmt_s(op, s);
        text_draw(fb, &FONT_TITLE, 18, disp - 44, op, th->accent);
    }
    /* botoes */
    for(int r = 0; r < ROWS; r++) for(int c = 0; c < COLS; c++){
        const char* lab = LABELS[r][c];
        if(!lab[0]) continue;
        i32 w; Rect br = button_rect(r, c, &w);
        int op = c == COLS - 1, fn = r == 0 && !op;
        u32 bg = op ? 0xFF9F0A : fn ? (dark ? 0xA5A5A5 : 0xD4D4D8) : (dark ? 0x333336 : 0xFFFFFF);
        u32 fg = op ? 0xFFFFFF : fn ? 0x1D1D1F : (dark ? 0xFFFFFF : 0x1D1D1F);
        if(op && pend && !entn && ((pend == '+' && lab[0] == '+') || (pend == '-' && lab[0] == '-')
            || (pend == '*' && (u8)lab[1] == 0x97) || (pend == '/' && (u8)lab[1] == 0xB7))){
            bg = 0xFFFFFF; fg = 0xFF9F0A;
        }
        int id = r * COLS + c;
        if(id == pressed && sys_time_ms() - pressed_at < 160) bg = shade(bg, dark ? 70 : -40);
        else if(rect_has(br, mx, my)) bg = shade(bg, dark ? 30 : -14);
        i32 rad = br.h / 2 < br.w / 2 ? br.h / 2 : br.w / 2;
        if(!dark) fb_shadow(fb, br, rad, 4, 1, 30);
        fb_round_rect(fb, br, rad, bg);
        text_center(fb, &FONT_TITLE, br, lab, fg);
    }
}

static int hit(i32 mx, i32 my){
    for(int r = 0; r < ROWS; r++) for(int c = 0; c < COLS; c++){
        i32 w; if(!LABELS[r][c][0]) continue;
        if(rect_has(button_rect(r, c, &w), mx, my)) return r * COLS + c;
    }
    return -1;
}

static int key_to_button(u32 ch, u16 code){
    if(code == KEY_ENTER || code == KEY_KPENTER || ch == '=') return 4 * COLS + 3;
    if(code == KEY_ESC || code == KEY_DELETE) return 0;
    const char* map = "0123456789";
    for(int d = 0; d < 10; d++) if(ch == (u32)map[d]){
        if(d == 0) return 4 * COLS;
        int r = 3 - (d - 1) / 3, c = (d - 1) % 3;
        return r * COLS + c;
    }
    switch(ch){
    case '+': return 3 * COLS + 3;
    case '-': return 2 * COLS + 3;
    case '*': case 'x': return 1 * COLS + 3;
    case '/': return 3;
    case '%': return 2;
    case ',': case '.': return 4 * COLS + 2;
    }
    return -1;
}

int main(void){
    app_init(&app, 3, "Calculadora", 260, 380);
    i32 lmx = -1, lmy = -1;
    draw(-1, -1);
    WmEvent e = app_commit(&app);
    for(;;){
        if(e.close) sys_exit();
        int dirty = e.changed;
        int b = -1;
        if(e.click) b = hit(e.mx, e.my);
        if(e.key){
            if(e.key == KEY_BACKSPACE && entn){ entn--; dirty = 1; }
            else b = key_to_button(e.ch, e.key);
        }
        if(e.menu == APP_MENU_CLEAR) b = 0;
        if(b >= 0){ press(LABELS[b / COLS][b % COLS]); pressed = b; pressed_at = sys_time_ms(); dirty = 1; }
        if(pressed >= 0 && sys_time_ms() - pressed_at > 170){ pressed = -1; dirty = 1; }
        if(e.mx != lmx || e.my != lmy){
            if(hit(e.mx, e.my) != hit(lmx, lmy)) dirty = 1;
            lmx = e.mx; lmy = e.my;
        }
        if(e.more){ e = app_poll(&app); continue; }
        if(dirty){ draw(e.mx, e.my); e = app_commit(&app); }
        else e = app_poll(&app);
    }
    return 0;
}
