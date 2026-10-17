/* font.c — texto das paginas com fontes TrueType/OpenType (stb_truetype).
 *
 * Familias embutidas, escolhidas como o fontconfig de um Linux faria:
 *   sans-serif -> Noto Sans          serif -> Noto Serif     monospace -> Noto Sans Mono
 *   Arial/Helvetica -> Pulsar Metric Sans (metricas de Arial)
 *   Times -> Pulsar Metric Serif (metricas de Times)
 * Fontes web (@font-face: TTF, OTF, WOFF, WOFF2) entram como familias novas;
 * cada face so e baixada quando algum texto a usa. Enquanto nao chega, o
 * texto sai na familia de reserva (a generica da lista do font-family), e a
 * pagina refaz o layout quando ela chegar. Glifos que a fonte nao tem saem
 * da reserva ou dos simbolos do DejaVu.
 *
 * Rasterizar e caro (ponto flutuante por software): avancos e glifos ficam
 * em cache por instancia (familia, estilo, px). So a thread principal usa
 * este modulo (layout e pintura); a decodificacao das fontes web roda nos
 * workers sem tocar no estado daqui. */
#include "vela.h"

#define STBTT_STATIC
#pragma clang diagnostic ignored "-Wunused-function"
#define STB_TRUETYPE_IMPLEMENTATION
#define STBTT_assert(x) ((void)0)
#include <math.h>
#include "stb_truetype.h"

/* gerados pelo Makefile a partir de third_party/fonts (TTF) */
extern const unsigned char font_sans_regular[], font_sans_bold[], font_sans_italic[], font_sans_bolditalic[],
    font_mono_regular[], font_mono_bold[], font_symbols[],
    font_serif_regular[], font_serif_bold[], font_serif_italic[], font_serif_bolditalic[],
    font_msans_regular[], font_msans_bold[], font_msans_italic[], font_msans_bolditalic[],
    font_mserif_regular[], font_mserif_bold[], font_mserif_italic[], font_mserif_bolditalic[];

#define SYMBOLS   6
#define NBUILTIN  19
#define MAXFACES  160
#define MAXFAMS   96
static const unsigned char* const BUILTIN[NBUILTIN] = {
    font_sans_regular, font_sans_bold, font_sans_italic, font_sans_bolditalic, font_mono_regular, font_mono_bold, font_symbols,
    font_serif_regular, font_serif_bold, font_serif_italic, font_serif_bolditalic,
    font_msans_regular, font_msans_bold, font_msans_italic, font_msans_bolditalic,
    font_mserif_regular, font_mserif_bold, font_mserif_italic, font_mserif_bolditalic };

typedef struct {
    stbtt_fontinfo info;
    const u8* data; int len;
    u8  state;                    /* 0 declarada, 1 pronta, 2 falhou, 3 baixando */
    u8  web, var;                 /* var: fonte variavel em peso (instancias por peso) */
    i16 wmin, wdef, wmax;
    int base, inst_w;             /* instancia: face de origem e peso */
    char* url;
    float upem_scale;             /* 1/unidades por em */
    i32 asc, desc, gap;           /* em unidades da fonte */
} Face;
static Face faces[MAXFACES];
static int nfaces = NBUILTIN;

typedef struct { i16 face; u16 wlo, whi; u8 italic; } FVar;
typedef struct { char name[64]; FVar v[24]; int nv; } Family;
static Family fams[MAXFAMS];
static int nfams = FAM_BUILTIN;

static u32 font_ver = 1;          /* muda quando uma fonte web chega */
static int fonts_changed = 0;
static int fonts_inflight = 0;

struct FontInst {
    u8 fam, fb, italic, fake_bold;
    u16 weight;
    int px;
    u32 ver;
    int face, fbface;             /* principal e reserva (embutida) */
    float scale, fbscale, sscale; /* sscale: simbolos */
    i32 asc, desc, gap;           /* px */
    u16 adv[256];                 /* avanco em 1/64 px (0xFFFF = ainda nao medido) */
    struct FontInst* hnext;
};
#define IHASH 512
static FontInst* ihash[IHASH];
static int ninst = 0;

typedef struct { FontInst* f; u32 cp; i16 x0, y0; u16 w, h; u8* bmp; } Glyph2;
#define GCAP 8192
static Glyph2* gcache = NULL;
static int gcount = 0;

static void face_metrics(Face* f){
    int a, d, g;
    stbtt_GetFontVMetrics(&f->info, &a, &d, &g);
    /* OS/2 com USE_TYPO_METRICS: os navegadores usam as metricas "typo" */
    stbtt_uint32 os2 = stbtt__find_table(f->info.data, (stbtt_uint32)f->info.fontstart, "OS/2");
    if(os2 && (ttUSHORT(f->info.data + os2 + 62) & 128)){
        int ta, td, tg;
        if(stbtt_GetFontVMetricsOS2(&f->info, &ta, &td, &tg)){ a = ta; d = td; g = tg; }
    }
    f->asc = a; f->desc = -d; f->gap = g < 0 ? 0 : g;
}
static int face_ok(int i){
    Face* f = &faces[i];
    if(i < NBUILTIN && f->state == 0){
        if(stbtt_InitFont(&f->info, BUILTIN[i], stbtt_GetFontOffsetForIndex(BUILTIN[i], 0))){ f->state = 1; face_metrics(f); }
        else f->state = 2;
    }
    return f->state == 1;
}

static void fam_init(void){
    static int done = 0;
    if(done) return;
    done = 1;
    static const struct { const char* n; i16 f[4]; } B[FAM_BUILTIN] = {
        { "sans-serif", { 0, 1, 2, 3 } }, { "monospace", { 4, 5, 4, 5 } }, { "serif", { 7, 8, 9, 10 } },
        { "arial", { 11, 12, 13, 14 } }, { "times", { 15, 16, 17, 18 } } };
    for(int i = 0; i < FAM_BUILTIN; i++){
        Family* f = &fams[i];
        snprintf(f->name, sizeof f->name, "%s", B[i].n);
        for(int k = 0; k < 4; k++){ f->v[k].face = B[i].f[k]; f->v[k].wlo = f->v[k].whi = (k & 1) ? 700 : 400; f->v[k].italic = (u8)(k >> 1); }
        f->nv = 4;
    }
}

/* ---- familias: nome (minusculo, sem aspas) -> id ---- */
int font_family_id(const char* name){
    fam_init();
    for(int i = FAM_BUILTIN; i < nfams; i++) if(!strcmp(fams[i].name, name)) return i;
    return -1;
}

/* ---- carga das fontes web ---- */
int font_wght_axis(const u8* d, int n, int* mn, int* def, int* mx);
u8* font_instance(const u8* d, int n, int wght, int* outlen);
typedef struct { Req rq; int face; u8* out; int outlen; } FontReq;
u8* font_decode(const u8* d, int n, int* outlen);
static int b64v(int c){
    if(c >= 'A' && c <= 'Z') return c - 'A';
    if(c >= 'a' && c <= 'z') return c - 'a' + 26;
    if(c >= '0' && c <= '9') return c - '0' + 52;
    if(c == '+' || c == '-') return 62;
    if(c == '/' || c == '_') return 63;
    return -1;
}
static void font_run(Task* t){                       /* worker */
    FontReq* q = (FontReq*)t;
    Resp* r = &q->rq.f.r;
    if(r->status == 200 && r->len > 0 && r->len < 16 * 1024 * 1024)
        q->out = font_decode((const u8*)r->body, r->len, &q->outlen);
    resp_free(r);
}
static void face_ready(int i, u8* data, int len){
    Face* f = &faces[i];
    fonts_inflight--;
    if(data && stbtt_InitFont(&f->info, data, stbtt_GetFontOffsetForIndex(data, 0))){
        f->data = data; f->len = len; f->state = 1; face_metrics(f);
        int mn, df, mx;
        if(!f->base && font_wght_axis(data, len, &mn, &df, &mx) && mx > mn){ f->var = 1; f->wmin = (i16)mn; f->wdef = (i16)df; f->wmax = (i16)mx; }
        font_ver++; fonts_changed = 1;
        font_reset_cache();                    /* glifos guardados podem ser da reserva */
    } else { free(data); f->state = 2; }
}
static void font_done(Task* t){                      /* principal */
    FontReq* q = (FontReq*)t;
    face_ready(q->face, q->out, q->outlen);
    req_free(&q->rq);
}
typedef struct { Task t; int face; char* src; u8* out; int outlen; } DataFont;
static void data_run(Task* t){
    DataFont* q = (DataFont*)t;
    const char* c = strchr(q->src, ',');
    if(c && strstr(q->src, ";base64") && strstr(q->src, ";base64") < c){
        size_t n = strlen(c + 1);
        u8* raw = (u8*)malloc(n * 3 / 4 + 4); int k = 0; u32 acc = 0; int bits = 0;
        for(const char* p = c + 1; *p; p++){
            int v = b64v((u8)*p); if(v < 0) continue;
            acc = acc << 6 | (u32)v; bits += 6;
            if(bits >= 8){ bits -= 8; raw[k++] = (u8)(acc >> bits); }
        }
        q->out = font_decode(raw, k, &q->outlen);
        free(raw);
    }
}
static void data_done(Task* t){
    DataFont* q = (DataFont*)t;
    face_ready(q->face, q->out, q->outlen);
    free(q->src); free(q);
}
/* instancia de uma fonte variavel num peso (worker) */
typedef struct { Task t; int face; const u8* src; int srclen, w; u8* out; int outlen; } InstTask;
static void inst_run(Task* t){ InstTask* q = (InstTask*)t; q->out = font_instance(q->src, q->srclen, q->w, &q->outlen); }
static void inst_done(Task* t){ InstTask* q = (InstTask*)t; face_ready(q->face, q->out, q->outlen); free(q); }
static int face_instance(int base, int w){
    for(int i = NBUILTIN; i < nfaces; i++) if(faces[i].base == base && faces[i].inst_w == w) return i;
    if(nfaces >= MAXFACES) return -1;
    int i = nfaces++;
    memset(&faces[i], 0, sizeof faces[i]);
    faces[i].base = base; faces[i].inst_w = w; faces[i].web = 1; faces[i].state = 3;
    fonts_inflight++;
    InstTask* q = (InstTask*)calloc(1, sizeof *q);
    q->face = i; q->src = faces[base].data; q->srclen = faces[base].len; q->w = w;
    q->t.run = inst_run; q->t.done = inst_done;
    work_hold();
    work_submit(&q->t);
    return i;
}

static void face_request(int i){
    Face* f = &faces[i];
    if(f->state != 0) return;
    f->state = 3;
    fonts_inflight++;
    if(str_istarts(f->url, "data:")){
        DataFont* q = (DataFont*)calloc(1, sizeof *q);
        q->face = i; q->src = strdup(f->url);
        q->t.run = data_run; q->t.done = data_done;
        work_hold();
        work_submit(&q->t);
        return;
    }
    FontReq* q = (FontReq*)calloc(1, sizeof *q);
    Req* r = req_new(f->url, 1);
    q->rq = *r; free(r);
    q->rq.f.url = q->rq.url;
    q->rq.task.run = font_run; q->rq.task.done = font_done;
    q->face = i;
    net_submit(&q->rq);
}

/* @font-face: registra a face (baixada so quando for usada) */
void font_face_add(const char* family, const char* url, int wlo, int whi, int italic){
    fam_init();
    if(!*family || !*url) return;
    int fi = font_family_id(family);
    if(fi < 0){
        if(nfams >= MAXFAMS) return;
        fi = nfams++;
        memset(&fams[fi], 0, sizeof fams[fi]);
        snprintf(fams[fi].name, sizeof fams[fi].name, "%s", family);
    }
    Family* fm = &fams[fi];
    int face = -1;
    for(int i = NBUILTIN; i < nfaces; i++) if(faces[i].url && !strcmp(faces[i].url, url)){ face = i; break; }
    if(whi < wlo) whi = wlo;
    for(int k = 0; k < fm->nv; k++)
        if(fm->v[k].wlo == wlo && fm->v[k].whi == whi && fm->v[k].italic == italic) return;   /* a primeira declaracao vale */
    if(face < 0){
        if(nfaces >= MAXFACES) return;
        face = nfaces++;
        memset(&faces[face], 0, sizeof faces[face]);
        faces[face].url = strdup(url); faces[face].web = 1;
    }
    if(fm->nv >= 24) return;
    fm->v[fm->nv].face = (i16)face; fm->v[fm->nv].wlo = (u16)wlo; fm->v[fm->nv].whi = (u16)whi; fm->v[fm->nv].italic = (u8)italic;
    fm->nv++;
}

/* a variante mais proxima (regras de casamento do CSS Fonts, simplificadas);
 * *got = peso que a face escolhida realmente tem */
static int pick(int fam, int want, int italic, int* ready, int* got){
    Family* f = &fams[fam];
    int best = -1, bs = 1 << 30, bw = 400;
    for(int k = 0; k < f->nv; k++){
        int lo = f->v[k].wlo, hi = f->v[k].whi, s = (f->v[k].italic != italic) * 100000, w;
        if(want >= lo && want <= hi){ w = want; }
        else if(want > 500){ w = hi < want ? hi : lo; s += hi < want ? 1000 + want - hi : lo - want; }      /* mais pesada primeiro */
        else if(want < 400){ w = lo > want ? lo : hi; s += lo > want ? 1000 + lo - want : want - hi; }     /* mais leve primeiro */
        else {
            /* 400..500: ate 500 para cima, depois mais leve, depois mais pesada */
            if(lo > want && lo <= 500){ w = lo; s += lo - want; }
            else if(hi < want){ w = hi; s += 600 + want - hi; }
            else { w = lo; s += 2000 + lo - want; }
        }
        if(s < bs){ bs = s; best = f->v[k].face; bw = w; }
    }
    *got = bw;
    if(best < 0){ *ready = 0; return -1; }
    if(best < NBUILTIN) face_ok(best);
    else if(faces[best].state == 0) face_request(best);
    *ready = faces[best].state == 1;
    if(*ready && faces[best].var){
        /* fonte variavel: a instancia no peso pedido */
        int w = bw < faces[best].wmin ? faces[best].wmin : bw > faces[best].wmax ? faces[best].wmax : bw;
        w = (w + 50) / 100 * 100;
        if(w != faces[best].wdef){
            int in = face_instance(best, w);
            if(in < 0) return best;
            *ready = faces[in].state == 1;
            if(faces[in].state == 2){ *ready = 1; return best; }
            return in;
        }
    }
    return best;
}

static void inst_init(FontInst* f){
    fam_init();
    int rd, got = 400, fgot = 400;
    int fb = pick(f->fb < FAM_BUILTIN ? f->fb : FAM_SANS, f->weight, f->italic, &rd, &fgot);
    int face = f->fam < nfams ? pick(f->fam, f->weight, f->italic, &rd, &got) : -1;
    if(face < 0 || !rd){ face = fb; got = fgot; }
    f->face = face; f->fbface = fb;
    f->fake_bold = f->weight >= 600 && got < 600;          /* negrito sintetico, como os navegadores */
    Face* p = &faces[face];
    f->scale = stbtt_ScaleForMappingEmToPixels(&p->info, (float)f->px);
    f->fbscale = stbtt_ScaleForMappingEmToPixels(&faces[fb].info, (float)f->px);
    f->sscale = face_ok(SYMBOLS) ? stbtt_ScaleForMappingEmToPixels(&faces[SYMBOLS].info, (float)f->px) : 0;
    f->asc = (i32)(p->asc * f->scale + 0.5f);
    f->desc = (i32)(p->desc * f->scale + 0.5f);
    f->gap = (i32)(p->gap * f->scale + 0.5f);
    memset(f->adv, 0xFF, sizeof f->adv);
    f->ver = font_ver;
}

FontInst* font_get(int fam, int fb, int weight, int italic, int px){
    if(px < 6) px = 6;
    if(px > 400) px = 400;
    if(weight < 100) weight = 400;
    italic = !!italic;
    u32 h = ((u32)fam * 131u + (u32)fb * 31u + (u32)weight * 7u + (u32)italic * 3u + (u32)px * 2654435761u) & (IHASH - 1);
    for(FontInst* f = ihash[h]; f; f = f->hnext)
        if(f->fam == fam && f->fb == fb && f->weight == weight && f->italic == italic && f->px == px){
            if(f->ver != font_ver) inst_init(f);
            return f;
        }
    if(ninst >= 4096){                     /* (nunca libera: a display list guarda ponteiros) */
        for(FontInst* f = ihash[h]; f; f = f->hnext) return f;
        return font_get(FAM_SANS, FAM_SANS, 400, 0, 16);
    }
    FontInst* f = (FontInst*)calloc(1, sizeof *f);
    f->fam = (u8)fam; f->fb = (u8)fb; f->weight = (u16)weight; f->italic = (u8)italic; f->px = px;
    inst_init(f);
    f->hnext = ihash[h]; ihash[h] = f;
    ninst++;
    return f;
}

/* fontes web chegaram desde a ultima chamada? (refazer o layout) */
int font_pump(void){
    int c = fonts_changed; fonts_changed = 0;
    return c;
}
int font_pending(void){ return fonts_inflight > 0; }

i32 font_ascent(FontInst* f){ return f->asc; }
i32 font_descent(FontInst* f){ return f->desc; }
i32 font_line_gap(FontInst* f){ return f->gap; }
i32 font_px(FontInst* f){ return f->px; }

static int zero_width(u32 cp){ return cp == 0xAD || (cp >= 0x200B && cp <= 0x200F) || cp == 0xFEFF || cp == 0x2060; }

/* a face que tem o glifo: a principal, a reserva ou a de simbolos */
static int face_for(FontInst* f, u32 cp, float* scale){
    int pf = f->face;
    if((pf < NBUILTIN && cp < 0x2000) || stbtt_FindGlyphIndex(&faces[pf].info, (int)cp)){ *scale = f->scale; return pf; }
    if(f->fbface != pf && stbtt_FindGlyphIndex(&faces[f->fbface].info, (int)cp)){ *scale = f->fbscale; return f->fbface; }
    if(faces[SYMBOLS].state == 1 && stbtt_FindGlyphIndex(&faces[SYMBOLS].info, (int)cp)){ *scale = f->sscale; return SYMBOLS; }
    *scale = f->scale; return pf;
}

static i32 adv64(FontInst* f, u32 cp){
    if(cp == 0xA0) cp = ' ';
    if(zero_width(cp)) return 0;
    if(cp < 256 && f->adv[cp] != 0xFFFF) return f->adv[cp];
    int aw, lsb; float sc;
    int fc = face_for(f, cp, &sc);
    stbtt_GetCodepointHMetrics(&faces[fc].info, (int)cp, &aw, &lsb);
    i32 v = (i32)(aw * sc * 64.0f + 0.5f);
    if(cp < 256) f->adv[cp] = (u16)(v > 0xFFFE ? 0xFFFE : v);
    return v;
}

i32 font_width(FontInst* f, const char* s, int n){
    if(!f) return 0;
    if(f->ver != font_ver) inst_init(f);
    i32 w = 0;
    for(int i = 0; i < n;){
        u32 cp = (u8)s[i] < 0x80 ? (u8)s[i++] : utf8_dec(s, n, &i);
        w += adv64(f, cp);
    }
    return (w + 32) >> 6;
}

void font_reset_cache(void){
    if(!gcache) return;
    for(int i = 0; i < GCAP; i++) if(gcache[i].f){ free(gcache[i].bmp); gcache[i].f = NULL; }
    gcount = 0;
}

static Glyph2* glyph(FontInst* f, u32 cp){
    if(!gcache) gcache = (Glyph2*)calloc(GCAP, sizeof(Glyph2));
    u32 h = ((u32)(u64)f * 2654435761u) ^ (cp * 40503u);
    for(int probe = 0; probe < GCAP; probe++){
        Glyph2* g = &gcache[(h + (u32)probe) & (GCAP - 1)];
        if(g->f == f && g->cp == cp) return g;
        if(!g->f){
            if(gcount > GCAP * 3 / 4){ font_reset_cache(); return glyph(f, cp); }
            int w = 0, ht = 0, xo = 0, yo = 0; float sc;
            int fc = face_for(f, cp, &sc);
            g->bmp = stbtt_GetCodepointBitmap(&faces[fc].info, 0, sc, (int)cp, &w, &ht, &xo, &yo);
            g->f = f; g->cp = cp; g->w = (u16)w; g->h = (u16)ht; g->x0 = (i16)xo; g->y0 = (i16)yo;
            gcount++;
            return g;
        }
    }
    return NULL;
}

void font_draw(Fb* fb, FontInst* f, i32 x, i32 baseline, const char* s, int n, u32 color, i32 ls){
    if(!f) return;
    if(f->ver != font_ver) inst_init(f);
    u32 ca = color >> 24; if(!ca) return;
    u32 rgb = color & 0xFFFFFF;
    Rect cl = fb->clip;
    if(baseline - f->asc - 4 >= cl.y + cl.h || baseline + f->desc + 4 < cl.y) return;
    i32 pen = x * 64;
    for(int i = 0; i < n;){
        u32 cp = (u8)s[i] < 0x80 ? (u8)s[i++] : utf8_dec(s, n, &i);
        if(cp == 0xA0) cp = ' ';
        i32 a = adv64(f, cp);
        if(cp != ' ' && cp != '\t' && !zero_width(cp)){
            i32 gx0 = (pen + 32) >> 6;
            if(gx0 > cl.x + cl.w) break;
            Glyph2* g = glyph(f, cp);
            if(g && g->bmp){
                i32 gx = gx0 + g->x0, gy = baseline + g->y0;
                for(i32 r = 0; r < g->h; r++){
                    i32 py = gy + r;
                    if(py < cl.y || py >= cl.y + cl.h) continue;
                    u32* row = fb->base + (i64)py * fb->stride;
                    const u8* src = g->bmp + r * g->w;
                    for(i32 k = 0; k < g->w + f->fake_bold; k++){
                        u32 al = k < g->w ? src[k] : 0;
                        if(f->fake_bold && k > 0 && src[k - 1] > al) al = src[k - 1];     /* negrito sintetico: o glifo engordado 1px */
                        if(!al) continue;
                        i32 px = gx + k;
                        if(px < cl.x || px >= cl.x + cl.w) continue;
                        if(ca < 255) al = al * ca / 255;
                        row[px] = mix(row[px], rgb, al);
                    }
                }
            }
        }
        pen += a + ls * 64;
    }
}
