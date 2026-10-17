/* woff.c — fontes web comprimidas -> TrueType/OpenType comum (sfnt), que o
 * stb_truetype entende.
 *
 *   WOFF 1: cada tabela comprimida com zlib (inflate do stb_image).
 *   WOFF 2: todas as tabelas num bloco Brotli; glyf/loca (e as vezes hmtx)
 *           vem "transformadas" e sao reconstruidas aqui (W3C WOFF2, sec. 5).
 *
 * Roda nos workers: so usa malloc/free e nao toca em estado global. */
#include "vela.h"
#include "brotli/decode.h"

extern char* stbi_zlib_decode_malloc_guesssize_headerflag(const char* buf, int len, int initial_size, int* outlen, int parse_header);

static u32 rd32(const u8* p){ return (u32)p[0] << 24 | (u32)p[1] << 16 | (u32)p[2] << 8 | p[3]; }
static u16 rd16(const u8* p){ return (u16)(p[0] << 8 | p[1]); }
static void wr32(u8* p, u32 v){ p[0] = (u8)(v >> 24); p[1] = (u8)(v >> 16); p[2] = (u8)(v >> 8); p[3] = (u8)v; }
static void wr16(u8* p, u32 v){ p[0] = (u8)(v >> 8); p[1] = (u8)v; }

typedef struct { u32 tag; u8* data; u32 len; int own; } Tab;

/* monta o arquivo sfnt a partir das tabelas */
static u8* build_sfnt(u32 flavor, Tab* t, int n, int* outlen){
    size_t size = 12 + 16 * (size_t)n;
    for(int i = 0; i < n; i++) size += (t[i].len + 3) & ~3u;
    u8* o = (u8*)calloc(1, size);
    if(!o) return NULL;
    int es = 0; while((2 << es) <= n) es++;
    wr32(o, flavor); wr16(o + 4, (u32)n);
    wr16(o + 6, (u32)(16 << es)); wr16(o + 8, (u32)es); wr16(o + 10, (u32)(n * 16 - (16 << es)));
    size_t off = 12 + 16 * (size_t)n;
    for(int i = 0; i < n; i++){
        u8* d = o + 12 + 16 * i;
        wr32(d, t[i].tag); wr32(d + 4, 0); wr32(d + 8, (u32)off); wr32(d + 12, t[i].len);
        if(t[i].len) memcpy(o + off, t[i].data, t[i].len);
        off += (t[i].len + 3) & ~3u;
    }
    *outlen = (int)size;
    return o;
}

/* ------------------------------------------------------------------ WOFF 1 */
static u8* woff1(const u8* d, int n, int* outlen){
    if(n < 44) return NULL;
    u32 flavor = rd32(d + 4);
    int nt = rd16(d + 12);
    if(nt <= 0 || nt > 64 || 44 + 20 * nt > n) return NULL;
    Tab t[64]; memset(t, 0, sizeof t);
    u8* res = NULL;
    for(int i = 0; i < nt; i++){
        const u8* e = d + 44 + 20 * i;
        u32 off = rd32(e + 4), clen = rd32(e + 8), olen = rd32(e + 12);
        t[i].tag = rd32(e);
        if((u64)off + clen > (u64)n || olen > 32u << 20) goto out;
        if(clen < olen){
            int got = 0;
            t[i].data = (u8*)stbi_zlib_decode_malloc_guesssize_headerflag((const char*)d + off, (int)clen, (int)olen, &got, 1);
            if(!t[i].data || (u32)got != olen) goto out;
            t[i].own = 1;
        } else t[i].data = (u8*)d + off;
        t[i].len = olen;
    }
    res = build_sfnt(flavor, t, nt, outlen);
out:
    for(int i = 0; i < nt; i++) if(t[i].own) free(t[i].data);
    return res;
}

/* ------------------------------------------------------------------ WOFF 2 */
static const char KNOWN[63][5] = {
    "cmap","head","hhea","hmtx","maxp","name","OS/2","post","cvt ","fpgm","glyf","loca","prep","CFF ","VORG","EBDT",
    "EBLC","gasp","hdmx","kern","LTSH","PCLT","VDMX","vhea","vmtx","BASE","GDEF","GPOS","GSUB","EBSC","JSTF","MATH",
    "CBDT","CBLC","COLR","CPAL","SVG ","sbix","acnt","avar","bdat","bloc","bsln","cvar","fdsc","feat","fmtx","fvar",
    "gvar","hsty","just","lcar","mort","morx","opbd","prop","trak","Zapf","Silf","Glat","Gloc","Feat","Sill" };
#define TAG(s) ((u32)(s)[0] << 24 | (u32)(s)[1] << 16 | (u32)(s)[2] << 8 | (u32)(s)[3])

typedef struct { const u8* p; u32 n, i; int bad; } Rd;
static u32 r8(Rd* r){ if(r->i + 1 > r->n){ r->bad = 1; return 0; } return r->p[r->i++]; }
static u32 r16(Rd* r){ if(r->i + 2 > r->n){ r->bad = 1; return 0; } u32 v = rd16(r->p + r->i); r->i += 2; return v; }
static u32 r32(Rd* r){ if(r->i + 4 > r->n){ r->bad = 1; return 0; } u32 v = rd32(r->p + r->i); r->i += 4; return v; }
static u32 rb128(Rd* r){
    u32 v = 0;
    for(int k = 0; k < 5; k++){
        u32 b = r8(r);
        if(k == 0 && b == 0x80){ r->bad = 1; return 0; }
        if(v & 0xFE000000u){ r->bad = 1; return 0; }
        v = (v << 7) | (b & 0x7F);
        if(!(b & 0x80)) return v;
    }
    r->bad = 1; return 0;
}
static u32 r255(Rd* r){
    u32 c = r8(r);
    if(c == 253) return r16(r);
    if(c == 255) return r8(r) + 253;
    if(c == 254) return r8(r) + 506;
    return c;
}

/* buffer de saida que cresce */
typedef struct { u8* p; size_t n, cap; int bad; } Out;
static void o_need(Out* o, size_t k){
    if(o->n + k <= o->cap) return;
    size_t c = o->cap ? o->cap * 2 : 65536;
    while(c < o->n + k) c *= 2;
    u8* q = (u8*)realloc(o->p, c);
    if(!q){ o->bad = 1; return; }
    o->p = q; o->cap = c;
}
static void o_put(Out* o, const void* d, size_t k){ o_need(o, k); if(o->bad) return; memcpy(o->p + o->n, d, k); o->n += k; }
static void o_16(Out* o, u32 v){ u8 b[2]; wr16(b, v); o_put(o, b, 2); }
static void o_pad4(Out* o){ static const u8 z[4]; o_put(o, z, (4 - (o->n & 3)) & 3); }

static int with_sign(int flag, int v){ return (flag & 1) ? v : -v; }

/* glyf/loca transformados -> glyf/loca normais; xmin[] recebe o xMin de cada glifo (para hmtx) */
static int rebuild_glyf(const u8* src, u32 len, Out* glyf, Out* loca, int* index_fmt, i16** xmin_out, int* nglyphs_out){
    Rd h = { src, len, 0, 0 };
    r16(&h);                                   /* reservado */
    u32 opt = r16(&h);
    u32 ng = r16(&h);
    u32 ifmt = r16(&h);
    u32 sz[7];
    for(int k = 0; k < 7; k++) sz[k] = r32(&h);
    if(h.bad) return 0;
    u32 off = h.i;
    Rd s[7];
    for(int k = 0; k < 7; k++){
        if((u64)off + sz[k] > len) return 0;
        s[k].p = src + off; s[k].n = sz[k]; s[k].i = 0; s[k].bad = 0;
        off += sz[k];
    }
    Rd *nc = &s[0], *np = &s[1], *fl = &s[2], *gl = &s[3], *co = &s[4], *bb = &s[5], *in = &s[6];
    const u8* overlap = NULL;
    if(opt & 1){ if(off + (ng + 7) / 8 > len) return 0; overlap = src + off; }
    u32 bbmap = ((ng + 31) / 32) * 4;
    if(bb->n < bbmap) return 0;
    const u8* bbits = bb->p; bb->i = bbmap;
    i16* xmin = (i16*)calloc(ng ? ng : 1, sizeof(i16));
    u32* offs = (u32*)calloc(ng + 1, sizeof(u32));
    int npts_cap = 0; i32 *xs = NULL, *ys = NULL; u8* on = NULL; u16* ends = NULL; int ends_cap = 0;
    int ok = 1;
    for(u32 g = 0; g < ng && ok; g++){
        offs[g] = (u32)glyf->n;
        int ncont = (i16)r16(nc);
        int has_bb = (bbits[g >> 3] >> (7 - (g & 7))) & 1;
        if(ncont == 0){ if(has_bb) ok = 0; continue; }
        if(ncont < 0){
            /* composto: os componentes sao copiados como estao */
            u32 c0 = co->i; int have_instr = 0;
            for(;;){
                u32 f = r16(co); r16(co);
                co->i += (f & 1) ? 4 : 2;
                if(f & 8) co->i += 2; else if(f & 0x40) co->i += 4; else if(f & 0x80) co->i += 8;
                if(f & 0x100) have_instr = 1;
                if(co->i > co->n){ ok = 0; break; }
                if(!(f & 0x20)) break;
            }
            if(!ok || !has_bb) { ok = 0; break; }
            u8 hd[10]; wr16(hd, 0xFFFF);
            for(int k = 0; k < 4; k++){ u32 v = r16(bb); wr16(hd + 2 + 2 * k, v); }
            xmin[g] = (i16)rd16(hd + 2);
            o_put(glyf, hd, 10);
            o_put(glyf, co->p + c0, co->i - c0);
            if(have_instr){
                u32 il = r255(gl);
                if(in->i + il > in->n){ ok = 0; break; }
                o_16(glyf, il); o_put(glyf, in->p + in->i, il); in->i += il;
            }
            o_pad4(glyf);
            continue;
        }
        /* simples */
        if(ncont > ends_cap){ ends_cap = ncont + 16; ends = (u16*)realloc(ends, sizeof(u16) * (size_t)ends_cap); }
        int npts = 0;
        for(int c = 0; c < ncont; c++){ npts += (int)r255(np); ends[c] = (u16)(npts - 1); }
        if(np->bad || npts > 65535){ ok = 0; break; }
        if(npts > npts_cap){
            npts_cap = npts + 64;
            xs = (i32*)realloc(xs, sizeof(i32) * (size_t)npts_cap); ys = (i32*)realloc(ys, sizeof(i32) * (size_t)npts_cap);
            on = (u8*)realloc(on, (size_t)npts_cap);
        }
        i32 x = 0, y = 0;
        i32 bx0 = 0, by0 = 0, bx1 = 0, by1 = 0;
        for(int i = 0; i < npts; i++){
            int flag = (int)r8(fl);
            on[i] = !(flag >> 7);
            flag &= 0x7F;
            int nb = flag < 84 ? 1 : flag < 120 ? 2 : flag < 124 ? 3 : 4;
            if(gl->i + (u32)nb > gl->n){ ok = 0; break; }
            const u8* t = gl->p + gl->i;
            int dx, dy;
            if(flag < 10){ dx = 0; dy = with_sign(flag, ((flag & 14) << 7) + t[0]); }
            else if(flag < 20){ dx = with_sign(flag, (((flag - 10) & 14) << 7) + t[0]); dy = 0; }
            else if(flag < 84){ int b0 = flag - 20, b1 = t[0]; dx = with_sign(flag, 1 + (b0 & 0x30) + (b1 >> 4)); dy = with_sign(flag >> 1, 1 + ((b0 & 0x0C) << 2) + (b1 & 0x0F)); }
            else if(flag < 120){ int b0 = flag - 84; dx = with_sign(flag, 1 + ((b0 / 12) << 8) + t[0]); dy = with_sign(flag >> 1, 1 + (((b0 % 12) >> 2) << 8) + t[1]); }
            else if(flag < 124){ int b2 = t[1]; dx = with_sign(flag, (t[0] << 4) + (b2 >> 4)); dy = with_sign(flag >> 1, ((b2 & 0x0F) << 8) + t[2]); }
            else { dx = with_sign(flag, (t[0] << 8) + t[1]); dy = with_sign(flag >> 1, (t[2] << 8) + t[3]); }
            gl->i += (u32)nb;
            x += dx; y += dy;
            xs[i] = x; ys[i] = y;
            if(i == 0 || x < bx0) bx0 = x;
            if(i == 0 || x > bx1) bx1 = x;
            if(i == 0 || y < by0) by0 = y;
            if(i == 0 || y > by1) by1 = y;
        }
        if(!ok || fl->bad) { ok = 0; break; }
        u32 il = r255(gl);
        if(gl->bad || in->i + il > in->n){ ok = 0; break; }
        u8 hd[10]; wr16(hd, (u32)ncont);
        if(has_bb){ for(int k = 0; k < 4; k++) wr16(hd + 2 + 2 * k, r16(bb)); }
        else { wr16(hd + 2, (u32)bx0); wr16(hd + 4, (u32)by0); wr16(hd + 6, (u32)bx1); wr16(hd + 8, (u32)by1); }
        xmin[g] = (i16)rd16(hd + 2);
        o_put(glyf, hd, 10);
        for(int c = 0; c < ncont; c++) o_16(glyf, ends[c]);
        o_16(glyf, il); o_put(glyf, in->p + in->i, il); in->i += il;
        /* flags sem compressao; coordenadas sempre em 16 bits */
        o_need(glyf, (size_t)npts * 5);
        if(glyf->bad){ ok = 0; break; }
        for(int i = 0; i < npts; i++){
            u8 f = on[i] ? 1 : 0;
            if(i == 0 && overlap && (overlap[g >> 3] >> (7 - (g & 7))) & 1) f |= 0x40;
            glyf->p[glyf->n++] = f;
        }
        i32 px = 0; for(int i = 0; i < npts; i++){ wr16(glyf->p + glyf->n, (u32)(xs[i] - px)); glyf->n += 2; px = xs[i]; }
        i32 py = 0; for(int i = 0; i < npts; i++){ wr16(glyf->p + glyf->n, (u32)(ys[i] - py)); glyf->n += 2; py = ys[i]; }
        o_pad4(glyf);
    }
    if(ok && !glyf->bad){
        offs[ng] = (u32)glyf->n;
        for(u32 g = 0; g <= ng; g++){
            if(ifmt) { u8 b[4]; wr32(b, offs[g]); o_put(loca, b, 4); }
            else o_16(loca, offs[g] / 2);
        }
    }
    free(offs); free(xs); free(ys); free(on); free(ends);
    if(!ok || glyf->bad || loca->bad){ free(xmin); return 0; }
    *index_fmt = (int)ifmt; *xmin_out = xmin; *nglyphs_out = (int)ng;
    return 1;
}

/* hmtx transformado: devolve a tabela comum (lsb = xMin do glifo quando omitido) */
static u8* rebuild_hmtx(const u8* src, u32 len, int nhm, int ng, const i16* xmin, u32* outlen){
    Rd r = { src, len, 0, 0 };
    u32 flags = r8(&r);
    if(nhm < 1 || nhm > ng) return NULL;
    u16* adv = (u16*)malloc(sizeof(u16) * (size_t)nhm);
    i16* lsb = (i16*)malloc(sizeof(i16) * (size_t)ng);
    for(int i = 0; i < nhm; i++) adv[i] = (u16)r16(&r);
    for(int i = 0; i < nhm; i++) lsb[i] = (flags & 1) ? xmin[i] : (i16)r16(&r);
    for(int i = nhm; i < ng; i++) lsb[i] = (flags & 2) ? xmin[i] : (i16)r16(&r);
    u8* o = NULL;
    if(!r.bad){
        *outlen = (u32)(4 * nhm + 2 * (ng - nhm));
        o = (u8*)malloc(*outlen);
        u8* p = o;
        for(int i = 0; i < nhm; i++){ wr16(p, adv[i]); wr16(p + 2, (u32)lsb[i]); p += 4; }
        for(int i = nhm; i < ng; i++){ wr16(p, (u32)lsb[i]); p += 2; }
    }
    free(adv); free(lsb);
    return o;
}

static u8* woff2(const u8* d, int n, int* outlen){
    if(n < 48) return NULL;
    u32 flavor = rd32(d + 4);
    int nt = rd16(d + 12);
    u32 comp = rd32(d + 20);
    if(flavor == TAG("ttcf") || nt <= 0 || nt > 64) return NULL;
    Rd r = { d, (u32)n, 48, 0 };
    struct { u32 tag, olen, tlen; int xf; } e[64];
    size_t total = 0;
    for(int i = 0; i < nt; i++){
        u32 fl = r8(&r);
        u32 tag = (fl & 63) == 63 ? r32(&r) : TAG(KNOWN[fl & 63]);
        int ver = (int)(fl >> 6);
        e[i].tag = tag;
        e[i].olen = rb128(&r);
        int is_gl = tag == TAG("glyf") || tag == TAG("loca");
        e[i].xf = is_gl ? ver == 0 : ver != 0;
        e[i].tlen = e[i].olen;
        if(e[i].xf) e[i].tlen = rb128(&r);
        if(r.bad || e[i].tlen > 64u << 20) return NULL;
        total += e[i].tlen;
    }
    if(r.i + comp > (u32)n || total > 96u << 20) return NULL;
    u8* raw = (u8*)malloc(total ? total : 1);
    if(!raw) return NULL;
    size_t got = total;
    if(BrotliDecoderDecompress(comp, d + r.i, &got, raw) != BROTLI_DECODER_RESULT_SUCCESS || got != total){ free(raw); return NULL; }
    Tab t[64]; memset(t, 0, sizeof t);
    size_t off = 0;
    int gi = -1, li = -1, hi = -1, hheai = -1;
    for(int i = 0; i < nt; i++){
        t[i].tag = e[i].tag; t[i].data = raw + off; t[i].len = e[i].tlen;
        if(e[i].tag == TAG("glyf")) gi = i;
        if(e[i].tag == TAG("loca")) li = i;
        if(e[i].tag == TAG("hmtx")) hi = i;
        if(e[i].tag == TAG("hhea")) hheai = i;
        off += e[i].tlen;
    }
    u8* res = NULL;
    i16* xmin = NULL; int ng = 0;
    Out go = {0}, lo = {0};
    if(gi >= 0 && e[gi].xf){
        int ifmt = 0;
        if(li < 0 || !rebuild_glyf(t[gi].data, t[gi].len, &go, &lo, &ifmt, &xmin, &ng)) goto out;
        t[gi].data = go.p; t[gi].len = (u32)go.n;
        t[li].data = lo.p; t[li].len = (u32)lo.n;
        /* head.indexToLocFormat precisa bater com a loca reconstruida */
        for(int i = 0; i < nt; i++) if(t[i].tag == TAG("head") && t[i].len >= 54) wr16(t[i].data + 50, (u32)ifmt);
    }
    if(hi >= 0 && e[hi].xf){
        if(!xmin || hheai < 0 || t[hheai].len < 36) goto out;
        int nhm = rd16(t[hheai].data + 34);
        u32 hl = 0;
        u8* hm = rebuild_hmtx(t[hi].data, t[hi].len, nhm, ng, xmin, &hl);
        if(!hm) goto out;
        t[hi].data = hm; t[hi].len = hl; t[hi].own = 1;
    }
    res = build_sfnt(flavor, t, nt, outlen);
out:
    for(int i = 0; i < nt; i++) if(t[i].own) free(t[i].data);
    free(go.p); free(lo.p); free(xmin); free(raw);
    return res;
}

/* qualquer formato -> sfnt em memoria nova (NULL se nao e fonte) */
u8* font_decode(const u8* d, int n, int* outlen){
    if(n < 12) return NULL;
    if(!memcmp(d, "wOFF", 4)) return woff1(d, n, outlen);
    if(!memcmp(d, "wOF2", 4)) return woff2(d, n, outlen);
    u32 v = rd32(d);
    if(v == 0x00010000 || v == TAG("OTTO") || v == TAG("true")){
        u8* o = (u8*)malloc((size_t)n);
        if(o){ memcpy(o, d, (size_t)n); *outlen = n; }
        return o;
    }
    return NULL;
}
