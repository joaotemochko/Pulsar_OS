/* image.c — imagens das paginas (PNG, JPEG, GIF, BMP via stb_image).
 * As <img> entram numa fila e sao baixadas uma por vez quando o navegador
 * esta ocioso; quando uma chega, a pagina refaz o layout. */
#include "vela.h"

#define STBI_NO_STDIO
#define STBI_NO_HDR
#define STBI_NO_LINEAR
#define STBI_NO_SIMD
#define STBI_NO_THREAD_LOCALS
#define STBI_ASSERT(x) ((void)0)
#define STB_IMAGE_IMPLEMENTATION
#include "stb_image.h"
#include <math.h>
#define NANOSVG_IMPLEMENTATION
#define NANOSVGRAST_IMPLEMENTATION
#pragma clang diagnostic ignored "-Wunused-function"
#include "nanosvg.h"
#include "nanosvgrast.h"

#define MAX_IMAGES 120
#define MAX_PIXELS (1024 * 1024)                   /* por imagem (reduz se passar) */
#define BUDGET     ((size_t)24 * 1024 * 1024)      /* pixels de todas as imagens */

static Image* images = NULL;
static int nimages = 0;
static size_t used = 0;                 /* bytes de pixels (workers decodificam em paralelo) */
static Mutex budget_lock = MUTEX_INIT;
static int inflight = 0;                /* imagens ainda baixando/decodificando */

/* reserva `bytes` do orcamento; 0 se nao cabe */
static int budget_take(size_t bytes){
    mutex_lock(&budget_lock);
    int ok = used + bytes <= BUDGET;
    if(ok) used += bytes;
    mutex_unlock(&budget_lock);
    return ok;
}

/* ---- carga assincrona: rede (thread de rede) -> decodificacao (worker) ->
 * entrega na thread principal. Imagens de uma pagina que ja foi trocada
 * ficam "orfas" e sao liberadas na entrega (um worker pode estar usando). */
typedef struct { Req rq; Image* im; int ok; } ImgReq;             /* Req primeiro */
typedef struct { Task t; Image* im; char* src; u32 color; int ok; } SvgTask;
static int changed = 0;
static int decode(Image* im, const u8* data, int len);
static int decode_svg(Image* im, const char* data, int len, u32 color);

static void img_finish(Image* im, int ok){
    inflight--;
    if(im->orphan){ free(im->px); free(im); return; }
    im->state = ok ? 1 : 2;
    if(ok) changed = 1;
}
static void img_run(Task* t){
    ImgReq* q = (ImgReq*)t;
    Resp* r = &q->rq.f.r;
    q->ok = r->status == 200 && r->len < 8 * 1024 * 1024 && decode(q->im, (const u8*)r->body, r->len);
    resp_free(r);                       /* o corpo nao e mais necessario */
}
static void img_done(Task* t){
    ImgReq* q = (ImgReq*)t;
    img_finish(q->im, q->ok);
    req_free(&q->rq);
}
static void svg_run(Task* t){
    SvgTask* q = (SvgTask*)t;
    q->ok = decode_svg(q->im, q->src, (int)strlen(q->src), q->color);
}
static void svg_done(Task* t){
    SvgTask* q = (SvgTask*)t;
    img_finish(q->im, q->ok);
    free(q->src); free(q);
}

Image* image_get(const char* url){
    if(!str_istarts(url, "http://") && !str_istarts(url, "https://")) return NULL;
    for(Image* im = images; im; im = im->next) if(!strcmp(im->url, url)) return im;
    if(nimages >= MAX_IMAGES) return NULL;
    Image* im = (Image*)calloc(1, sizeof *im);
    snprintf(im->url, sizeof im->url, "%s", url);
    /* no fim da lista: carrega na ordem do documento */
    Image** pp = &images; while(*pp) pp = &(*pp)->next;
    *pp = im;
    nimages++;
    ImgReq* q = (ImgReq*)calloc(1, sizeof *q);
    Req* r = req_new(url, 1);
    q->rq = *r; free(r);
    q->rq.f.url = q->rq.url;
    q->rq.task.run = img_run; q->rq.task.done = img_done;
    q->im = im;
    inflight++;
    net_submit(&q->rq);
    return im;
}

int image_pending(void){ return inflight > 0; }

/* alguma imagem chegou desde a ultima chamada? (refazer o layout) */
int image_pump(void){ int c = changed; changed = 0; return c; }

/* ------------------------------------------------------------------ SVG */
static int is_svg(const u8* d, int len){
    int i = 0;
    if(len >= 3 && d[0] == 0xEF && d[1] == 0xBB && d[2] == 0xBF) i = 3;
    while(i < len && is_ws(d[i])) i++;
    if(i + 4 < len && !memcmp(d + i, "<svg", 4)) return 1;
    if(i + 5 < len && (!memcmp(d + i, "<?xml", 5) || !memcmp(d + i, "<!--", 4) || !memcmp(d + i, "<!DOC", 5))){
        int lim = len < 4096 ? len : 4096;
        for(int k = i; k + 4 < lim; k++) if(!memcmp(d + k, "<svg", 4)) return 1;
    }
    return 0;
}

/* rasteriza; `color` substitui currentColor (0 = preto) */
static int decode_svg(Image* im, const char* data, int len, u32 color){
    char* txt = (char*)malloc((size_t)len + 1);
    memcpy(txt, data, (size_t)len); txt[len] = 0;
    char hex[16]; snprintf(hex, sizeof hex, "#%06x     ", color & 0xFFFFFF);   /* 12 caracteres, como "currentColor" */
    for(char* q = txt; (q = strstr(q, "currentColor")); q += 12) memcpy(q, hex, 12);
    NSVGimage* img = nsvgParse(txt, "px", 96.0f);
    free(txt);
    if(!img) return 0;
    float w = img->width, h = img->height;
    if(w <= 0 || h <= 0){ nsvgDelete(img); return 0; }
    /* rasteriza em 2x (nitido quando ampliado), no maximo 1024 px */
    float sc = 2.0f;
    if(w * sc > 1024 || h * sc > 1024) sc = 1024.0f / (w > h ? w : h);
    int pw = (int)(w * sc + 0.5f), ph = (int)(h * sc + 0.5f);
    if(pw < 1) pw = 1;
    if(ph < 1) ph = 1;
    size_t bytes = (size_t)pw * (size_t)ph * 4;
    if(!budget_take(bytes)){ nsvgDelete(img); return 0; }
    u8* rgba = (u8*)calloc(1, bytes);
    NSVGrasterizer* r = nsvgCreateRasterizer();
    if(!rgba || !r){ free(rgba); if(r) nsvgDeleteRasterizer(r); nsvgDelete(img); return 0; }
    nsvgRasterize(r, img, 0, 0, sc, rgba, pw, ph, pw * 4);
    nsvgDeleteRasterizer(r);
    nsvgDelete(img);
    im->px = (u32*)rgba;                    /* converte RGBA -> ARGB no proprio buffer */
    for(int i = 0; i < pw * ph; i++){
        u8* p = rgba + i * 4;
        im->px[i] = ((u32)p[3] << 24) | ((u32)p[0] << 16) | ((u32)p[1] << 8) | p[2];
    }
    im->w = (int)(w + 0.5f); im->h = (int)(h + 0.5f);
    im->pw = pw; im->ph = ph;
    return 1;
}

/* <svg> inline da pagina (ja serializado): rasterizado num worker (com
 * ponto flutuante por software, dezenas de icones atrasariam a pagina) */
Image* image_svg_inline(const char* key, const char* svg, int len, u32 color){
    for(Image* im = images; im; im = im->next) if(!strcmp(im->url, key)) return im;
    if(nimages >= MAX_IMAGES * 2) return NULL;
    Image* im = (Image*)calloc(1, sizeof *im);
    snprintf(im->url, sizeof im->url, "%s", key);
    im->next = images; images = im;
    nimages++;
    SvgTask* q = (SvgTask*)calloc(1, sizeof *q);
    q->t.run = svg_run; q->t.done = svg_done;
    q->im = im; q->src = strndup(svg, (size_t)len); q->color = color;
    inflight++;
    work_hold();
    work_submit(&q->t);
    return im;
}

/* serializa um <svg> do DOM restaurando a caixa (o parser HTML deixa tudo
 * minusculo, e o SVG diferencia: viewBox, linearGradient...) */
static const char* const SVG_NAMES[] = { "viewBox", "preserveAspectRatio", "linearGradient", "radialGradient",
    "gradientUnits", "gradientTransform", "clipPath", "clipPathUnits", "spreadMethod", "stdDeviation",
    "textPath", "foreignObject", "patternUnits", "patternTransform", "maskUnits", "markerWidth", "markerHeight",
    "refX", "refY", "xlink:href", NULL };
static void svg_name(Buf* b, const char* n){
    for(int i = 0; SVG_NAMES[i]; i++) if(str_ieq(n, SVG_NAMES[i])){ buf_puts(b, SVG_NAMES[i]); return; }
    buf_puts(b, n);
}
void svg_serialize(const Node* n, Buf* b){
    if(n->type == N_TEXT){ buf_put(b, n->text, n->tlen); return; }
    if(n->type != N_ELEM) return;
    buf_putc(b, '<'); svg_name(b, n->tag);
    if(dom_is(n, "svg") && !dom_attr(n, "xmlns")) buf_puts(b, " xmlns=\"http://www.w3.org/2000/svg\"");
    /* o NanoSVG nao entende fracoes sem unidade em gradientes objectBoundingBox:
     * x2="1" vira x2="100%" */
    int grad = (dom_is(n, "lineargradient") || dom_is(n, "radialgradient"))
        && !(dom_attr(n, "gradientunits") && str_ieq(dom_attr(n, "gradientunits"), "userSpaceOnUse"));
    for(int i = 0; i < n->nattr; i++){
        const char* an = n->attrs[i].name; const char* v = n->attrs[i].value;
        buf_putc(b, ' '); svg_name(b, an); buf_puts(b, "=\"");
        int coord = grad && (!strcmp(an, "x1") || !strcmp(an, "x2") || !strcmp(an, "y1") || !strcmp(an, "y2")
            || !strcmp(an, "cx") || !strcmp(an, "cy") || !strcmp(an, "r") || !strcmp(an, "fx") || !strcmp(an, "fy"));
        char* end = NULL;
        double d = coord ? strtod(v, &end) : 0;
        if(coord && end && end != v && !*end){ char t[32]; snprintf(t, sizeof t, "%g%%", d * 100); buf_puts(b, t); }
        else for(; *v; v++){ if(*v == '"') buf_puts(b, "&quot;"); else if(*v == '&') buf_puts(b, "&amp;"); else buf_putc(b, *v); }
        buf_putc(b, '"');
    }
    buf_putc(b, '>');
    for(Node* c = n->first; c; c = c->next) svg_serialize(c, b);
    buf_puts(b, "</"); svg_name(b, n->tag); buf_putc(b, '>');
}

static int decode(Image* im, const u8* data, int len){
    if(is_svg(data, len)){
        /* normaliza pelo DOM (mesmo caminho do SVG inline) */
        Doc* tmp = doc_new("about:blank");
        html_parse_into(tmp, tmp->root, NULL, (const char*)data, len);
        Node* svg = dom_find(tmp->root, "svg");
        int ok = 0;
        if(svg){ Buf b = {0}; svg_serialize(svg, &b); if(b.n) ok = decode_svg(im, b.p, b.n, 0); buf_free(&b); }
        doc_free(tmp);
        return ok;
    }
    int w, h, comp;
    if(!stbi_info_from_memory(data, len, &w, &h, &comp)) return 0;
    if(w <= 0 || h <= 0 || (i64)w * h > 2600 * 2600) return 0;   /* decodificar usa w*h*4 temporarios */
    u8* px = stbi_load_from_memory(data, len, &w, &h, &comp, 4);
    if(!px) return 0;
    /* imagens enormes: reduz (vizinho mais proximo) para poupar memoria */
    int step = 1;
    while((i64)(w / step) * (h / step) > MAX_PIXELS) step++;
    int ow = w / step, oh = h / step;
    if(ow < 1) ow = 1;
    if(oh < 1) oh = 1;
    size_t bytes = sizeof(u32) * (size_t)ow * (size_t)oh;
    if(!budget_take(bytes)){ stbi_image_free(px); return 0; }
    im->px = (u32*)malloc(bytes);
    if(!im->px){ stbi_image_free(px); return 0; }
    for(int y = 0; y < oh; y++){
        const u8* row = px + (size_t)(y * step) * (size_t)w * 4;
        for(int x = 0; x < ow; x++){
            const u8* p = row + (size_t)(x * step) * 4;
            im->px[y * ow + x] = ((u32)p[3] << 24) | ((u32)p[0] << 16) | ((u32)p[1] << 8) | p[2];
        }
    }
    stbi_image_free(px);
    /* tamanho intrinseco continua o original */
    im->w = w; im->h = h;
    im->pw = ow; im->ph = oh;
    return 1;
}

void image_reset(void){
    Image* im = images;
    while(im){
        Image* n = im->next;
        if(im->state == 0) im->orphan = 1;            /* ainda em voo: libera na entrega */
        else { free(im->px); free(im); }
        im = n;
    }
    images = NULL; nimages = 0;
    mutex_lock(&budget_lock); used = 0; mutex_unlock(&budget_lock);
    changed = 0;
}

/* desenha a imagem esticada em dst; cover: recorta para preencher sem distorcer */
void image_draw(Fb* fb, Image* im, Rect dst, u8 alpha, int cover){
    if(!im || im->state != 1 || dst.w <= 0 || dst.h <= 0) return;
    i32 sx0 = 0, sy0 = 0, sw = im->pw, sh = im->ph;
    if(cover){
        /* escala = max(dw/sw, dh/sh): recorta o excesso no centro */
        if((i64)dst.w * sh > (i64)dst.h * sw){ i32 nh = (i32)((i64)sw * dst.h / dst.w); sy0 = (sh - nh) / 2; sh = nh; }
        else { i32 nw = (i32)((i64)sh * dst.w / dst.h); sx0 = (sw - nw) / 2; sw = nw; }
        if(sw < 1) sw = 1;
        if(sh < 1) sh = 1;
    }
    Rect r = rect_and(dst, fb->clip);
    for(i32 y = r.y; y < r.y + r.h; y++){
        i32 sy = sy0 + (i32)((i64)(y - dst.y) * sh / dst.h);
        const u32* srow = im->px + (i64)sy * im->pw;
        u32* drow = fb->base + (i64)y * fb->stride;
        for(i32 x = r.x; x < r.x + r.w; x++){
            u32 p = srow[sx0 + (i32)((i64)(x - dst.x) * sw / dst.w)];
            u32 a = p >> 24;
            if(alpha < 255) a = a * alpha / 255;
            if(!a) continue;
            drow[x] = a >= 255 ? (p & 0xFFFFFF) : mix(drow[x], p & 0xFFFFFF, a);
        }
    }
}
