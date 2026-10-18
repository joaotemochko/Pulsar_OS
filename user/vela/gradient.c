/* gradient.c — linear-gradient() e radial-gradient() de fundo.
 *
 * O layout monta uma tabela de 256 cores ao longo do gradiente (paradas
 * com posicao em % ou px, angulos, "to right", "to bottom left"...); a
 * pintura projeta cada pixel na linha do gradiente e consulta a tabela,
 * so com inteiros. repeating-* repete a tabela no comprimento da ultima parada. */
#include "vela.h"
#include <math.h>

struct Grad {
    u8  radial, repeat;
    i32 sx, sy;                   /* direcao (Q16) */
    i32 len;                      /* comprimento da linha do gradiente (px) */
    i32 cx, cy;                   /* radial: centro relativo a caixa */
    i32 rx, ry;                   /* radial: raios */
    i32 period;                   /* repeating: fracao da tabela usada (0..255) */
    u32 lut[256];                 /* ARGB */
};

static int parse_angle(const char* s, int n, i32* deg10){
    char t[32]; if(n > 31) n = 31;
    memcpy(t, s, (size_t)n); t[n] = 0;
    char* e; double v = strtod(t, &e);
    if(e == t) return 0;
    double d;
    if(!strncmp(e, "deg", 3)) d = v;
    else if(!strncmp(e, "turn", 4)) d = v * 360;
    else if(!strncmp(e, "rad", 3)) d = v * 57.29577951308232;
    else if(!strncmp(e, "grad", 4)) d = v * 0.9;
    else if(v == 0 && !*e) d = 0;
    else return 0;
    *deg10 = (i32)(d * 10);
    return 1;
}

/* fatias de topo separadas por virgula */
static int split_args(const char* s, int n, const char** a, int* al, int max){
    int k = 0, depth = 0, st = 0;
    for(int i = 0; i <= n && k < max; i++){
        if(i == n || (s[i] == ',' && depth == 0)){
            int a0 = st, a1 = i;
            while(a0 < a1 && is_ws((u8)s[a0])) a0++;
            while(a1 > a0 && is_ws((u8)s[a1 - 1])) a1--;
            a[k] = s + a0; al[k] = a1 - a0; k++;
            st = i + 1;
        } else if(s[i] == '(') depth++;
        else if(s[i] == ')') depth--;
    }
    return k;
}

static u32 lerp(u32 a, u32 b, int t){       /* t 0..256 */
    u32 r = 0;
    for(int sh = 0; sh < 32; sh += 8){
        int ca = (a >> sh) & 255, cb = (b >> sh) & 255;
        r |= (u32)(ca + (cb - ca) * t / 256) << sh;
    }
    return r;
}

Grad* grad_build(Arena* ar, const char* spec, i32 w, i32 h){
    int radial = str_icontains(spec, "radial-gradient") != NULL;
    int repeat = str_istarts(spec, "repeating-");
    const char* p = strchr(spec, '(');
    if(!p || w <= 0 || h <= 0) return NULL;
    p++;
    int n = (int)strlen(p);
    while(n > 0 && p[n - 1] != ')') n--;
    if(n > 0) n--;                      /* o ')' final */
    const char* a[24]; int al[24];
    int k = split_args(p, n, a, al, 24);
    if(k < 2) return NULL;
    Grad* g = (Grad*)arena_alloc(ar, sizeof(Grad));
    memset(g, 0, sizeof *g);
    g->radial = (u8)radial; g->repeat = (u8)repeat;
    int first = 0;
    i32 deg10 = 1800;                   /* padrao: to bottom */
    if(!radial){
        if(parse_angle(a[0], al[0], &deg10)) first = 1;
        else if(str_istarts(a[0], "to ")){
            int top = str_icontains(a[0], "top") && str_icontains(a[0], "top") < a[0] + al[0];
            int bot = str_icontains(a[0], "bottom") && str_icontains(a[0], "bottom") < a[0] + al[0];
            int lef = str_icontains(a[0], "left") && str_icontains(a[0], "left") < a[0] + al[0];
            int rig = str_icontains(a[0], "right") && str_icontains(a[0], "right") < a[0] + al[0];
            double ang = 180;
            if(top && !lef && !rig) ang = 0;
            else if(bot && !lef && !rig) ang = 180;
            else if(rig && !top && !bot) ang = 90;
            else if(lef && !top && !bot) ang = 270;
            else {
                /* cantos: perpendicular a diagonal */
                double base = atan2((double)h, (double)w) * 57.29577951308232;
                if(top && rig) ang = 90 - base;
                else if(bot && rig) ang = 90 + base;
                else if(bot && lef) ang = 270 - base;
                else ang = 270 + base;
            }
            deg10 = (i32)(ang * 10);
            first = 1;
        }
        double r = deg10 / 10.0 / 57.29577951308232;
        double sx = sin(r), sy = -cos(r);
        g->sx = (i32)(sx * 65536); g->sy = (i32)(sy * 65536);
        g->len = (i32)(fabs(w * sx) + fabs(h * sy));
    } else {
        /* radial: forma/tamanho/posicao simplificados (elipse ate o canto mais distante, no centro) */
        g->cx = w / 2; g->cy = h / 2;
        const char* at = NULL;
        u32 dummy_ok; int ok = 0;
        (void)dummy_ok;
        css_parse_color(a[0], al[0], &ok);
        if(!ok){
            first = 1;
            for(int i = 0; i + 3 <= al[0]; i++) if(!strncmp(a[0] + i, "at ", 3)){ at = a[0] + i + 3; break; }
            if(at){
                int top = strstr(at, "top") != NULL, bot = strstr(at, "bottom") != NULL;
                int lef = strstr(at, "left") != NULL, rig = strstr(at, "right") != NULL;
                if(lef) g->cx = 0; if(rig) g->cx = w;
                if(top) g->cy = 0; if(bot) g->cy = h;
                char* e; double v = strtod(at, &e);
                if(e != at && *e == '%'){ g->cx = (i32)(w * v / 100); while(*e && *e != ' ') e++; double v2 = strtod(e, &e); g->cy = (i32)(h * v2 / 100); }
            }
        }
        i32 fx = g->cx > w - g->cx ? g->cx : w - g->cx, fy = g->cy > h - g->cy ? g->cy : h - g->cy;
        int circle = str_icontains(a[0], "circle") && str_icontains(a[0], "circle") < a[0] + al[0];
        /* farthest-corner: a elipse com a mesma proporcao que passa pelo canto */
        g->rx = (i32)(fx * 1.41421356); g->ry = (i32)(fy * 1.41421356);
        if(circle){ i32 r = (i32)sqrt((double)fx * fx + (double)fy * fy); g->rx = g->ry = r; }
        if(g->rx < 1) g->rx = 1;
        if(g->ry < 1) g->ry = 1;
        g->len = g->rx;
    }
    if(g->len < 1) g->len = 1;
    /* paradas */
    u32 col[24]; i32 pos[24]; int ns = 0;
    for(int i = first; i < k && ns < 24; i++){
        const char* s = a[i]; int l = al[i];
        /* cor = ate o primeiro espaco fora de parenteses */
        int d = 0, e = 0;
        while(e < l && !(d == 0 && is_ws((u8)s[e]))){ if(s[e] == '(') d++; else if(s[e] == ')') d--; e++; }
        int ok = 0;
        u32 c = css_parse_color(s, e, &ok);
        if(!ok){
            /* "50%" sozinho: dica de interpolacao (ignorada) */
            continue;
        }
        col[ns] = c; pos[ns] = -1;
        while(e < l && is_ws((u8)s[e])) e++;
        if(e < l){
            char t[32]; int tl = l - e < 31 ? l - e : 31; memcpy(t, s + e, (size_t)tl); t[tl] = 0;
            char* q; double v = strtod(t, &q);
            if(q != t){
                if(*q == '%') pos[ns] = (i32)(v * 100);
                else pos[ns] = (i32)(v * 10000 / g->len);
                /* segunda posicao (parada dupla) */
                while(*q && *q != ' ') q++;
                while(*q == ' ') q++;
                if(*q && ns + 1 < 24){
                    char* q2; double v2 = strtod(q, &q2);
                    if(q2 != q){ ns++; col[ns] = c; pos[ns] = *q2 == '%' ? (i32)(v2 * 100) : (i32)(v2 * 10000 / g->len); }
                }
            }
        }
        ns++;
    }
    if(ns == 0) return NULL;
    if(ns == 1){ col[1] = col[0]; pos[1] = -1; ns = 2; }
    if(pos[0] < 0) pos[0] = 0;
    if(pos[ns - 1] < 0) pos[ns - 1] = 10000;
    for(int i = 1; i < ns; i++) if(pos[i] >= 0 && pos[i] < pos[i - 1]) pos[i] = pos[i - 1];
    /* posicoes que faltam: distribuidas entre as conhecidas */
    for(int i = 1; i < ns; i++){
        if(pos[i] >= 0) continue;
        int j = i; while(pos[j] < 0) j++;
        for(int m = i; m < j; m++) pos[m] = pos[i - 1] + (pos[j] - pos[i - 1]) * (m - i + 1) / (j - i + 1);
        i = j;
    }
    i32 span = 10000;
    if(repeat){ span = pos[ns - 1] - pos[0]; if(span <= 0) span = 10000; }
    g->period = repeat ? span : 10000;
    for(int t = 0; t < 256; t++){
        i32 x = (repeat ? pos[0] : 0) + (i32)((i64)t * (repeat ? span : 10000) / 255);
        u32 c;
        if(x <= pos[0]) c = col[0];
        else if(x >= pos[ns - 1]) c = col[ns - 1];
        else {
            int i = 1; while(i < ns - 1 && pos[i] < x) i++;
            i32 a0 = pos[i - 1], a1 = pos[i];
            c = a1 > a0 ? lerp(col[i - 1], col[i], (int)((i64)(x - a0) * 256 / (a1 - a0))) : col[i];
        }
        g->lut[t] = c;
    }
    return g;
}

/* dentro do retangulo arredondado? */
static int in_round(i32 x, i32 y, i32 w, i32 h, i32 r){
    if(r <= 0) return 1;
    i32 cx = x < r ? r : x >= w - r ? w - r - 1 : x;
    i32 cy = y < r ? r : y >= h - r ? h - r - 1 : y;
    i32 dx = x - cx, dy = y - cy;
    return dx * dx + dy * dy <= r * r;
}

void grad_draw(Fb* fb, Grad* g, Rect box, i32 radius, u8 alpha){
    if(!g) return;
    Rect r = rect_and(box, fb->clip);
    if(radius > box.h / 2) radius = box.h / 2;
    if(radius > box.w / 2) radius = box.w / 2;
    i32 w = box.w, h = box.h;
    for(i32 y = r.y; y < r.y + r.h; y++){
        u32* row = fb->base + (i64)y * fb->stride;
        i32 ly = y - box.y;
        for(i32 x = r.x; x < r.x + r.w; x++){
            i32 lx = x - box.x;
            if(radius && !in_round(lx, ly, w, h, radius)) continue;
            i64 t;
            if(!g->radial){
                i64 d = ((i64)(2 * lx + 1 - w) * g->sx + (i64)(2 * ly + 1 - h) * g->sy) / 2;      /* Q16, a partir do centro */
                t = 128 * 65536 + d * 256 / g->len;
                t >>= 16;
            } else {
                i64 dx = (i64)(lx - g->cx) * 65536 / g->rx, dy = (i64)(ly - g->cy) * 65536 / g->ry;
                u64 dd = (u64)(dx * dx + dy * dy);
                /* raiz inteira de dd (Q32) -> Q16 */
                u64 s = 0, bit = 1ULL << 62;
                while(bit > dd) bit >>= 2;
                while(bit){ if(dd >= s + bit){ dd -= s + bit; s = (s >> 1) + bit; } else s >>= 1; bit >>= 2; }
                t = (i64)(s * 255 >> 16);
            }
            if(g->repeat){ t = t * 10000 / g->period; t %= 256; if(t < 0) t += 256; }
            else if(t < 0) t = 0;
            else if(t > 255) t = 255;
            u32 c = g->lut[t];
            u32 a = c >> 24;
            if(alpha < 255) a = a * alpha / 255;
            if(!a) continue;
            row[x] = a >= 255 ? (c & 0xFFFFFF) : mix(row[x], c & 0xFFFFFF, a);
        }
    }
}
