/* varfont.c — instancia uma fonte variavel (TrueType com fvar/gvar) num
 * peso fixo, gerando uma fonte estatica que o stb_truetype desenha.
 *
 * Segue o "Algorithm for interpolation of instance values" do OpenType:
 * coordenadas normalizadas (com avar), escalar de cada tupla, deltas por
 * ponto (com IUP para pontos nao citados) e pontos "fantasmas" para o novo
 * avanco horizontal. So o eixo de peso (wght) muda; os outros ficam no
 * padrao. Aritmetica em ponto fixo (Q16): roda nos workers, sem FPU. */
#include "vela.h"

static u32 rd32(const u8* p){ return (u32)p[0] << 24 | (u32)p[1] << 16 | (u32)p[2] << 8 | p[3]; }
static u16 rd16(const u8* p){ return (u16)(p[0] << 8 | p[1]); }
static void wr16(u8* p, u32 v){ p[0] = (u8)(v >> 8); p[1] = (u8)v; }
static void wr32(u8* p, u32 v){ p[0] = (u8)(v >> 24); p[1] = (u8)(v >> 16); p[2] = (u8)(v >> 8); p[3] = (u8)v; }
#define TAG(s) ((u32)(s)[0] << 24 | (u32)(s)[1] << 16 | (u32)(s)[2] << 8 | (u32)(s)[3])

typedef struct { u32 tag; const u8* p; u32 len; } T;
static int tables(const u8* d, int n, T* t, int max){
    if(n < 12) return 0;
    int nt = rd16(d + 4);
    if(nt > max || 12 + 16 * nt > n) return 0;
    for(int i = 0; i < nt; i++){
        const u8* e = d + 12 + 16 * i;
        u32 off = rd32(e + 8), len = rd32(e + 12);
        if((u64)off + len > (u64)n) return 0;
        t[i].tag = rd32(e); t[i].p = d + off; t[i].len = len;
    }
    return nt;
}
static T* find(T* t, int n, const char* tag){ for(int i = 0; i < n; i++) if(t[i].tag == TAG(tag)) return &t[i]; return NULL; }

/* faixa do eixo wght (min/padrao/max); 0 se a fonte nao e variavel em peso */
int font_wght_axis(const u8* d, int n, int* mn, int* def, int* mx){
    T t[64]; int nt = tables(d, n, t, 64);
    T* fv = find(t, nt, "fvar");
    if(!fv || !find(t, nt, "gvar") || fv->len < 16) return 0;
    int ao = rd16(fv->p + 4), ac = rd16(fv->p + 8), as = rd16(fv->p + 10);
    for(int i = 0; i < ac; i++){
        const u8* a = fv->p + ao + i * as;
        if(a + 20 > fv->p + fv->len) return 0;
        if(rd32(a) == TAG("wght")){
            *mn = (int)((i32)rd32(a + 4) >> 16); *def = (int)((i32)rd32(a + 8) >> 16); *mx = (int)((i32)rd32(a + 12) >> 16);
            return 1;
        }
    }
    return 0;
}

/* ---- glifos ---- */
typedef struct {
    int nc;                         /* contornos (-1 composto) */
    int np;                         /* pontos (simples) ou componentes (composto) */
    u16* ends; u8* on; i32* x; i32* y;
    const u8* instr; int ninstr;
    u8 overlap;
    const u8* comp; int complen;    /* composto: registros originais */
    i32 adv, lsb;
    i32 bx0, by0, bx1, by1;
    int done;
} G;

static int parse_glyph(const u8* g, u32 len, G* o){
    if(len == 0){ o->nc = 0; return 1; }
    if(len < 10) return 0;
    o->nc = (i16)rd16(g);
    o->bx0 = (i16)rd16(g + 2); o->by0 = (i16)rd16(g + 4); o->bx1 = (i16)rd16(g + 6); o->by1 = (i16)rd16(g + 8);
    const u8* p = g + 10; const u8* end = g + len;
    if(o->nc < 0){
        /* composto: um "ponto" por componente (o deslocamento) */
        const u8* c0 = p; int nc = 0; int instr = 0;
        for(;;){
            if(p + 4 > end) return 0;
            u16 f = rd16(p); p += 4;
            p += (f & 1) ? 4 : 2;
            if(f & 8) p += 2; else if(f & 0x40) p += 4; else if(f & 0x80) p += 8;
            if(f & 0x100) instr = 1;
            nc++;
            if(p > end) return 0;
            if(!(f & 0x20)) break;
        }
        o->comp = c0; o->complen = (int)(p - c0);
        if(instr && p + 2 <= end){ o->ninstr = rd16(p); o->instr = p + 2; if(o->instr + o->ninstr > end) o->ninstr = 0; }
        o->np = nc;
        o->x = (i32*)calloc((size_t)nc + 4, sizeof(i32)); o->y = (i32*)calloc((size_t)nc + 4, sizeof(i32));
        p = c0;
        for(int i = 0; i < nc; i++){
            u16 f = rd16(p); p += 4;
            if(f & 1){ o->x[i] = (i16)rd16(p); o->y[i] = (i16)rd16(p + 2); p += 4; }
            else { o->x[i] = (int8_t)p[0]; o->y[i] = (int8_t)p[1]; p += 2; }
            if(f & 8) p += 2; else if(f & 0x40) p += 4; else if(f & 0x80) p += 8;
        }
        return 1;
    }
    if(p + 2 * o->nc + 2 > end) return 0;
    o->ends = (u16*)malloc(sizeof(u16) * (size_t)(o->nc ? o->nc : 1));
    for(int i = 0; i < o->nc; i++){ o->ends[i] = rd16(p); p += 2; }
    int np = o->nc ? o->ends[o->nc - 1] + 1 : 0;
    o->np = np;
    o->ninstr = rd16(p); p += 2; o->instr = p; p += o->ninstr;
    if(p > end) return 0;
    o->on = (u8*)malloc((size_t)np + 1);
    u8* fl = (u8*)malloc((size_t)np + 1);
    o->x = (i32*)calloc((size_t)np + 4, sizeof(i32)); o->y = (i32*)calloc((size_t)np + 4, sizeof(i32));
    for(int i = 0; i < np;){
        if(p >= end){ free(fl); return 0; }
        u8 f = *p++;
        if(i == 0) o->overlap = f & 0x40;
        int rep = 0;
        if(f & 8){ if(p >= end){ free(fl); return 0; } rep = *p++; }
        for(int k = 0; k <= rep && i < np; k++) fl[i++] = f;
    }
    i32 v = 0;
    for(int i = 0; i < np; i++){
        u8 f = fl[i];
        if(f & 2){ if(p >= end){ free(fl); return 0; } v += (f & 16) ? *p : -(i32)*p; p++; }
        else if(!(f & 16)){ if(p + 2 > end){ free(fl); return 0; } v += (i16)rd16(p); p += 2; }
        o->x[i] = v;
    }
    v = 0;
    for(int i = 0; i < np; i++){
        u8 f = fl[i];
        if(f & 4){ if(p >= end){ free(fl); return 0; } v += (f & 32) ? *p : -(i32)*p; p++; }
        else if(!(f & 32)){ if(p + 2 > end){ free(fl); return 0; } v += (i16)rd16(p); p += 2; }
        o->y[i] = v;
        o->on[i] = fl[i] & 1;
    }
    free(fl);
    return 1;
}

/* ---- gvar: numeros de pontos e deltas compactados ---- */
static const u8* read_points(const u8* p, const u8* end, int total, u16** out, int* n){
    if(p >= end) return NULL;
    int cnt = *p++;
    if(cnt & 0x80){ if(p >= end) return NULL; cnt = ((cnt & 0x7F) << 8) | *p++; }
    if(cnt == 0){ *out = NULL; *n = total; return p; }        /* todos os pontos */
    u16* pts = (u16*)malloc(sizeof(u16) * (size_t)cnt);
    int k = 0, last = 0;
    while(k < cnt){
        if(p >= end){ free(pts); return NULL; }
        u8 c = *p++;
        int run = (c & 0x7F) + 1, words = c & 0x80;
        for(int r = 0; r < run && k < cnt; r++){
            int d;
            if(words){ if(p + 2 > end){ free(pts); return NULL; } d = rd16(p); p += 2; }
            else { if(p >= end){ free(pts); return NULL; } d = *p++; }
            last += d; pts[k++] = (u16)last;
        }
    }
    *out = pts; *n = cnt;
    return p;
}
static const u8* read_deltas(const u8* p, const u8* end, int n, i32* out){
    int k = 0;
    while(k < n){
        if(p >= end) return NULL;
        u8 c = *p++;
        int run = (c & 0x3F) + 1;
        for(int r = 0; r < run && k < n; r++){
            if(c & 0x80) out[k++] = 0;
            else if(c & 0x40){ if(p + 2 > end) return NULL; out[k++] = (i16)rd16(p); p += 2; }
            else { if(p >= end) return NULL; out[k++] = (int8_t)*p++; }
        }
    }
    return p;
}

/* escalar de uma tupla (Q16) */
static i64 tuple_scalar(int na, const i32* coord, const i32* peak, const i32* st, const i32* en, int inter){
    i64 s = 65536;
    for(int a = 0; a < na; a++){
        i32 p = peak[a], c = coord[a];
        if(p == 0 || c == p) continue;
        if(c == 0) return 0;
        i32 lo = inter ? st[a] : (p < 0 ? p : 0), hi = inter ? en[a] : (p > 0 ? p : 0);
        if(c < lo || c > hi) return 0;
        if(c < p) s = s * (c - lo) / (p - lo);
        else s = s * (hi - c) / (hi - p);
    }
    return s;
}

/* IUP: deltas dos pontos nao citados, interpolando entre os vizinhos citados */
static void iup(const G* g, const u8* touched, i64* dx, i64* dy){
    int start = 0;
    for(int c = 0; c < g->nc; c++){
        int end = g->ends[c];
        int first = -1;
        for(int i = start; i <= end; i++) if(touched[i]){ first = i; break; }
        if(first >= 0){
            for(int pass = 0; pass < 2; pass++){
                const i32* org = pass ? g->y : g->x; i64* d = pass ? dy : dx;
                int i = first;
                do {
                    int j = i + 1 > end ? start : i + 1;
                    while(!touched[j]) j = j + 1 > end ? start : j + 1;
                    /* pontos entre i e j (exclusivos) */
                    int k = i + 1 > end ? start : i + 1;
                    while(k != j){
                        i32 a = org[i], b = org[j], v = org[k];
                        i64 da = d[i], db = d[j];
                        if(a == b) d[k] = da == db ? da : 0;
                        else {
                            i32 lo = a < b ? a : b, hi = a < b ? b : a;
                            i64 dlo = a < b ? da : db, dhi = a < b ? db : da;
                            if(v <= lo) d[k] = dlo;
                            else if(v >= hi) d[k] = dhi;
                            else d[k] = dlo + (dhi - dlo) * (v - lo) / (hi - lo);
                        }
                        k = k + 1 > end ? start : k + 1;
                    }
                    i = j;
                } while(i != first);
            }
        }
        start = end + 1;
    }
}

typedef struct { u8* p; size_t n, cap; } Ob;
static void ob_put(Ob* o, const void* d, size_t k){
    if(o->n + k > o->cap){ size_t c = o->cap ? o->cap * 2 : 65536; while(c < o->n + k) c *= 2; o->p = (u8*)realloc(o->p, c); o->cap = c; }
    memcpy(o->p + o->n, d, k); o->n += k;
}
static void ob_16(Ob* o, u32 v){ u8 b[2]; wr16(b, v); ob_put(o, b, 2); }
static void ob_pad4(Ob* o){ static const u8 z[4]; ob_put(o, z, (4 - (o->n & 3)) & 3); }

static i32 rnd(i64 q16){ return (i32)((q16 >= 0 ? q16 + 32768 : q16 - 32767) >> 16); }

static void comp_bbox(G* gs, int ng, int gi, int depth){
    G* g = &gs[gi];
    if(g->done || depth > 8) return;
    g->done = 1;
    if(g->nc >= 0) return;
    const u8* p = g->comp;
    int any = 0;
    for(int i = 0; i < g->np; i++){
        u16 f = rd16(p); u16 idx = rd16(p + 2); p += 4;
        p += (f & 1) ? 4 : 2;
        if(f & 8) p += 2; else if(f & 0x40) p += 4; else if(f & 0x80) p += 8;
        if(idx >= ng) continue;
        comp_bbox(gs, ng, idx, depth + 1);
        G* c = &gs[idx];
        if(!c->nc) continue;
        i32 ox = (f & 2) ? g->x[i] : 0, oy = (f & 2) ? g->y[i] : 0;
        i32 x0 = c->bx0 + ox, y0 = c->by0 + oy, x1 = c->bx1 + ox, y1 = c->by1 + oy;
        if(!any || x0 < g->bx0) g->bx0 = x0;
        if(!any || y0 < g->by0) g->by0 = y0;
        if(!any || x1 > g->bx1) g->bx1 = x1;
        if(!any || y1 > g->by1) g->by1 = y1;
        any = 1;
    }
}

/* a fonte no peso `wght`; NULL se nao deu */
u8* font_instance(const u8* d, int n, int wght, int* outlen){
    T t[64]; int nt = tables(d, n, t, 64);
    T *fv = find(t, nt, "fvar"), *gv = find(t, nt, "gvar"), *av = find(t, nt, "avar");
    T *gl = find(t, nt, "glyf"), *lo = find(t, nt, "loca"), *hd = find(t, nt, "head"), *hh = find(t, nt, "hhea");
    T *hm = find(t, nt, "hmtx"), *mp = find(t, nt, "maxp");
    if(!fv || !gv || !gl || !lo || !hd || !hh || !hm || !mp || hd->len < 54 || hh->len < 36 || mp->len < 6) return NULL;
    int ng = rd16(mp->p + 4), lfmt = (i16)rd16(hd->p + 50), nhm = rd16(hh->p + 34);
    /* coordenadas normalizadas (Q16, depois F2Dot14 do avar) */
    int ao = rd16(fv->p + 4), na = rd16(fv->p + 8), as = rd16(fv->p + 10);
    if(na <= 0 || na > 16) return NULL;
    i32 coord[16];
    for(int a = 0; a < na; a++){
        const u8* ax = fv->p + ao + a * as;
        i32 mn = (i32)rd32(ax + 4), df = (i32)rd32(ax + 8), mx = (i32)rd32(ax + 12);
        i32 v = df;
        if(rd32(ax) == TAG("wght")) v = wght << 16;
        if(v < mn) v = mn;
        if(v > mx) v = mx;
        i64 c = 0;
        if(v < df && df > mn) c = -(i64)(df - v) * 65536 / (df - mn);
        else if(v > df && mx > df) c = (i64)(v - df) * 65536 / (mx - df);
        coord[a] = (i32)c;
    }
    if(av && av->len >= 8){
        const u8* p = av->p + 8; const u8* end = av->p + av->len;
        for(int a = 0; a < na && p + 2 <= end; a++){
            int pc = rd16(p); p += 2;
            if(p + 4 * pc > end) break;
            i32 c = coord[a];
            for(int k = 1; k < pc; k++){
                i32 f0 = (i16)rd16(p + 4 * (k - 1)) * 4, t0 = (i16)rd16(p + 4 * (k - 1) + 2) * 4;
                i32 f1 = (i16)rd16(p + 4 * k) * 4, t1 = (i16)rd16(p + 4 * k + 2) * 4;
                if(c >= f0 && c <= f1){ c = f1 == f0 ? t0 : t0 + (i32)((i64)(t1 - t0) * (c - f0) / (f1 - f0)); break; }
            }
            coord[a] = c;
            p += 4 * pc;
        }
    }
    /* glifos originais */
    G* gs = (G*)calloc((size_t)ng, sizeof(G));
    u8* res = NULL;
    Ob go = {0}, lo2 = {0};
    u8* touched = NULL; i64 *dx = NULL, *dy = NULL; i32* tmp = NULL;
    for(int i = 0; i < ng; i++){
        u32 o0 = lfmt ? rd32(lo->p + 4 * i) : rd16(lo->p + 2 * i) * 2u;
        u32 o1 = lfmt ? rd32(lo->p + 4 * i + 4) : rd16(lo->p + 2 * i + 2) * 2u;
        if(o1 < o0 || o1 > gl->len) goto out;
        if(!parse_glyph(gl->p + o0, o1 - o0, &gs[i])) goto out;
        int k = i < nhm ? i : nhm - 1;
        gs[i].adv = rd16(hm->p + 4 * k);
        gs[i].lsb = i < nhm ? (i16)rd16(hm->p + 4 * i + 2) : (i16)rd16(hm->p + 4 * nhm + 2 * (i - nhm));
    }
    /* deltas do gvar */
    const u8* G0 = gv->p; u32 GL = gv->len;
    if(GL < 20) goto out;
    int gna = rd16(G0 + 4), nshared = rd16(G0 + 6);
    u32 shoff = rd32(G0 + 8);
    int gflags = rd16(G0 + 14);
    u32 dataoff = rd32(G0 + 16);
    if(gna != na) goto out;
    touched = (u8*)malloc(65540); dx = (i64*)malloc(sizeof(i64) * 65540); dy = (i64*)malloc(sizeof(i64) * 65540);
    tmp = (i32*)malloc(sizeof(i32) * 2 * 65540);
    for(int gi = 0; gi < ng; gi++){
        G* g = &gs[gi];
        u32 v0 = (gflags & 1) ? rd32(G0 + 20 + 4 * gi) : rd16(G0 + 20 + 2 * gi) * 2u;
        u32 v1 = (gflags & 1) ? rd32(G0 + 24 + 4 * gi) : rd16(G0 + 22 + 2 * gi) * 2u;
        int npt = (g->nc ? g->np : 0) + 4;
        /* pontos fantasmas: origem e avanco */
        if(g->nc){ g->x[g->np] = g->bx0 - g->lsb; g->x[g->np + 1] = g->x[g->np] + g->adv; }
        i64 ax0 = 0, ax1 = 0;
        if(v1 <= v0 || dataoff + v1 > GL){ goto apply; }
        {
            const u8* vd = G0 + dataoff + v0; const u8* vend = G0 + dataoff + v1;
            int tvc = rd16(vd), doff = rd16(vd + 2);
            int count = tvc & 0x0FFF;
            const u8* hdr = vd + 4;
            const u8* ser = vd + doff;
            u16* shared_pts = NULL; int nshared_pts = 0;
            if(tvc & 0x8000){ ser = read_points(ser, vend, npt, &shared_pts, &nshared_pts); if(!ser) goto apply; }
            memset(dx, 0, sizeof(i64) * (size_t)npt); memset(dy, 0, sizeof(i64) * (size_t)npt);
            for(int tv = 0; tv < count; tv++){
                if(hdr + 4 > vend) break;
                int dsz = rd16(hdr), ti = rd16(hdr + 2); hdr += 4;
                i32 peak[16], st[16], en[16];
                if(ti & 0x8000){ for(int a = 0; a < na; a++){ peak[a] = (i16)rd16(hdr) * 4; hdr += 2; } }
                else {
                    int si = ti & 0x0FFF;
                    if(si >= nshared || shoff + (u32)(si + 1) * na * 2 > GL) break;
                    for(int a = 0; a < na; a++) peak[a] = (i16)rd16(G0 + shoff + (si * na + a) * 2) * 4;
                }
                int inter = (ti & 0x4000) != 0;
                if(inter){ for(int a = 0; a < na; a++){ st[a] = (i16)rd16(hdr) * 4; hdr += 2; } for(int a = 0; a < na; a++){ en[a] = (i16)rd16(hdr) * 4; hdr += 2; } }
                const u8* tdata = ser; ser += dsz;
                i64 sc = tuple_scalar(na, coord, peak, st, en, inter);
                if(!sc) continue;
                u16* pts = shared_pts; int npts = nshared_pts;
                int priv = (ti & 0x2000) != 0;
                if(priv){ tdata = read_points(tdata, vend, npt, &pts, &npts); if(!tdata) continue; }
                else if(!(tvc & 0x8000)){ pts = NULL; npts = npt; }
                i32* ddx = tmp; i32* ddy = tmp + 65540;
                if(npts > 65535 || !(tdata = read_deltas(tdata, vend, npts, ddx)) || !read_deltas(tdata, vend, npts, ddy)){ if(priv) free(pts); continue; }
                if(!pts){
                    for(int k = 0; k < npts && k < npt; k++){ dx[k] += ddx[k] * sc; dy[k] += ddy[k] * sc; }
                } else if(g->nc > 0){
                    /* pontos citados + IUP no resto (so contornos; fantasmas sem delta) */
                    i64* tx = (i64*)calloc((size_t)npt, sizeof(i64)); i64* ty = (i64*)calloc((size_t)npt, sizeof(i64));
                    memset(touched, 0, (size_t)npt);
                    for(int k = 0; k < npts; k++) if(pts[k] < npt){ tx[pts[k]] = ddx[k] * sc; ty[pts[k]] = ddy[k] * sc; touched[pts[k]] = 1; }
                    iup(g, touched, tx, ty);
                    for(int k = 0; k < npt; k++){ dx[k] += tx[k]; dy[k] += ty[k]; }
                    free(tx); free(ty);
                } else {
                    for(int k = 0; k < npts; k++) if(pts[k] < npt){ dx[pts[k]] += ddx[k] * sc; dy[pts[k]] += ddy[k] * sc; }
                }
                if(priv) free(pts);
            }
            free(shared_pts);
            if(g->nc){
                int np = g->np;
                for(int k = 0; k < np; k++){ g->x[k] = rnd(((i64)g->x[k] << 16) + dx[k]); g->y[k] = rnd(((i64)g->y[k] << 16) + dy[k]); }
                ax0 = dx[np]; ax1 = dx[np + 1];
            } else { ax0 = dx[0]; ax1 = dx[1]; }
        }
apply:
        if(g->nc){
            i32 o0 = rnd(((i64)g->x[g->np] << 16) + ax0), o1 = rnd(((i64)g->x[g->np + 1] << 16) + ax1);
            g->adv = o1 - o0;
            if(g->adv < 0) g->adv = 0;
            if(g->nc > 0){
                for(int k = 0; k < g->np; k++){
                    if(!k || g->x[k] < g->bx0) g->bx0 = g->x[k];
                    if(!k || g->x[k] > g->bx1) g->bx1 = g->x[k];
                    if(!k || g->y[k] < g->by0) g->by0 = g->y[k];
                    if(!k || g->y[k] > g->by1) g->by1 = g->y[k];
                }
                g->lsb = g->bx0 - o0;
                g->done = 1;
            } else g->lsb = -o0;                /* acerto depois do bbox do composto */
        } else g->adv = rnd(((i64)g->adv << 16) + ax1 - ax0);
    }
    for(int i = 0; i < ng; i++) if(gs[i].nc < 0){ i32 org = -gs[i].lsb; comp_bbox(gs, ng, i, 0); gs[i].lsb = gs[i].bx0 - org; }
    /* glyf/loca novos (loca longa) */
    for(int i = 0; i < ng; i++){
        G* g = &gs[i];
        u8 b[4]; wr32(b, (u32)go.n); ob_put(&lo2, b, 4);
        if(!g->nc) continue;
        u8 hdr[10]; wr16(hdr, (u32)g->nc); wr16(hdr + 2, (u32)g->bx0); wr16(hdr + 4, (u32)g->by0); wr16(hdr + 6, (u32)g->bx1); wr16(hdr + 8, (u32)g->by1);
        ob_put(&go, hdr, 10);
        if(g->nc < 0){
            const u8* p = g->comp;
            for(int c = 0; c < g->np; c++){
                u16 f = rd16(p); u16 idx = rd16(p + 2);
                const u8* a = p + 4;
                int alen = (f & 1) ? 4 : 2;
                const u8* rest = a + alen;
                int rlen = (f & 8) ? 2 : (f & 0x40) ? 4 : (f & 0x80) ? 8 : 0;
                if(f & 2){
                    ob_16(&go, (u32)(f | 1)); ob_16(&go, idx); ob_16(&go, (u32)g->x[c]); ob_16(&go, (u32)g->y[c]);
                } else { ob_16(&go, f); ob_16(&go, idx); ob_put(&go, a, (size_t)alen); }
                ob_put(&go, rest, (size_t)rlen);
                p = rest + rlen;
            }
            if(g->instr && g->ninstr){ ob_16(&go, (u32)g->ninstr); ob_put(&go, g->instr, (size_t)g->ninstr); }
        } else {
            for(int c = 0; c < g->nc; c++) ob_16(&go, g->ends[c]);
            ob_16(&go, (u32)g->ninstr); ob_put(&go, g->instr, (size_t)g->ninstr);
            for(int k = 0; k < g->np; k++){ u8 f = (u8)(g->on[k] | (k == 0 ? g->overlap : 0)); ob_put(&go, &f, 1); }
            i32 px = 0; for(int k = 0; k < g->np; k++){ ob_16(&go, (u32)(g->x[k] - px)); px = g->x[k]; }
            i32 py = 0; for(int k = 0; k < g->np; k++){ ob_16(&go, (u32)(g->y[k] - py)); py = g->y[k]; }
        }
        ob_pad4(&go);
    }
    { u8 b[4]; wr32(b, (u32)go.n); ob_put(&lo2, b, 4); }
    {
        /* hmtx completo (um par por glifo) */
        u8* hmx = (u8*)malloc((size_t)ng * 4);
        for(int i = 0; i < ng; i++){ wr16(hmx + 4 * i, (u32)gs[i].adv); wr16(hmx + 4 * i + 2, (u32)gs[i].lsb); }
        u8 head[64], hhea[64];
        memcpy(head, hd->p, hd->len < 64 ? hd->len : 64); wr16(head + 50, 1);
        memcpy(hhea, hh->p, hh->len < 64 ? hh->len : 64); wr16(hhea + 34, (u32)ng);
        /* monta o sfnt: o resto das tabelas como esta, sem as de variacao */
        T outt[64]; int no = 0;
        for(int i = 0; i < nt; i++){
            u32 tg = t[i].tag;
            if(tg == TAG("fvar") || tg == TAG("gvar") || tg == TAG("avar") || tg == TAG("HVAR") || tg == TAG("MVAR")
               || tg == TAG("STAT") || tg == TAG("cvar") || tg == TAG("VVAR")) continue;
            outt[no] = t[i];
            if(tg == TAG("glyf")){ outt[no].p = go.p; outt[no].len = (u32)go.n; }
            else if(tg == TAG("loca")){ outt[no].p = lo2.p; outt[no].len = (u32)lo2.n; }
            else if(tg == TAG("hmtx")){ outt[no].p = hmx; outt[no].len = (u32)ng * 4; }
            else if(tg == TAG("head")){ outt[no].p = head; outt[no].len = hd->len < 64 ? hd->len : 64; }
            else if(tg == TAG("hhea")){ outt[no].p = hhea; outt[no].len = hh->len < 64 ? hh->len : 64; }
            no++;
        }
        size_t size = 12 + 16 * (size_t)no;
        for(int i = 0; i < no; i++) size += (outt[i].len + 3) & ~3u;
        res = (u8*)calloc(1, size);
        if(res){
            int es = 0; while((2 << es) <= no) es++;
            wr32(res, rd32(d)); wr16(res + 4, (u32)no); wr16(res + 6, (u32)(16 << es)); wr16(res + 8, (u32)es); wr16(res + 10, (u32)(no * 16 - (16 << es)));
            size_t off = 12 + 16 * (size_t)no;
            for(int i = 0; i < no; i++){
                u8* e = res + 12 + 16 * i;
                wr32(e, outt[i].tag); wr32(e + 4, 0); wr32(e + 8, (u32)off); wr32(e + 12, outt[i].len);
                memcpy(res + off, outt[i].p, outt[i].len);
                off += (outt[i].len + 3) & ~3u;
            }
            *outlen = (int)size;
        }
        free(hmx);
    }
out:
    for(int i = 0; i < ng; i++){ free(gs[i].ends); free(gs[i].on); free(gs[i].x); free(gs[i].y); }
    free(gs); free(go.p); free(lo2.p); free(touched); free(dx); free(dy); free(tmp);
    return res;
}
