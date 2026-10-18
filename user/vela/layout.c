/* layout.c — caixas CSS -> display list.
 *
 * Um passo recursivo sobre o DOM ja estilizado. Cada caixa de bloco recebe
 * a largura do bloco que a contem e devolve a sua altura; o conteudo inline
 * vira linhas (LB). Larguras "encolher para caber" (inline-block, floats,
 * celulas de tabela, itens flex) usam medidas min/max-content, obtidas
 * rodando o mesmo layout sem emitir nada (DL == NULL) com largura 0 e
 * "infinita" — com cache por nodo.
 *
 * Floats: cada contexto de formatacao de bloco (html, overflow, flow-root,
 * celulas, itens flex/grid, inline-blocks, os proprios floats) guarda os
 * floats ja colocados; cada linha pergunta a faixa livre na sua altura.
 * Nas medidas min/max-content os floats contam como inline-blocks.
 * Simplificacoes: rowspan conta 1. */
#include "vela.h"

#define BIG 100000
static DList* DL;                 /* NULL = so medindo */
static u32 mgen = 1, lgen = 1;
static i32 VW, VH;
static int fixed_depth = 0, hide_depth = 0;

typedef struct { i32 w, h, extent, base; } Out;     /* base: linha de base da ultima linha (da borda da margem), -1 sem linhas */
static i32 g_base = -0x7FFFFFFF;                     /* linha de base da ultima linha posta (y absoluto) */
static void layout_box(Node* e, i32 x, i32 y, i32 cbw, i32 forced_w, i32 forced_h, i32 cshift, Out* o);

/* filhos para o layout: ::before, os filhos do DOM, ::after */
static Node* kfirst(Node* e){ return e->pse[0] ? e->pse[0] : e->first ? e->first : e->pse[1]; }
static Node* knext(Node* c){
    Node* p = c->parent;
    if(c->flags & NF_PSEUDO){ if(c == p->pse[0]) return p->first ? p->first : p->pse[1]; return NULL; }
    return c->next ? c->next : p ? p->pse[1] : NULL;
}

/* ---------------------------------------------------------------- floats */
typedef struct { i32 x0, x1, y0, y1; u8 side; } Flt;       /* caixa da margem; side 1 esq, 2 dir */
typedef struct { Flt* f; int n, cap; i32 last_top; } FCtx;
static FCtx* FC = NULL;           /* contexto atual (NULL: medindo, floats viram inline-block) */
static int intrinsic = 0;

static void fc_band(i32 y, i32 h, i32 L, i32 R, i32* l, i32* r){
    *l = L; *r = R;
    if(!FC) return;
    if(h < 1) h = 1;
    for(int i = 0; i < FC->n; i++){
        Flt* f = &FC->f[i];
        if(f->y0 >= y + h || f->y1 <= y) continue;
        if(f->side == 1){ if(f->x1 > *l) *l = f->x1; }
        else if(f->x0 < *r) *r = f->x0;
    }
}
/* o fim mais proximo (abaixo de y) dos floats que cruzam [y, y+h) */
static i32 fc_below(i32 y, i32 h){
    i32 best = -1;
    if(!FC) return y;
    if(h < 1) h = 1;
    for(int i = 0; i < FC->n; i++){
        Flt* f = &FC->f[i];
        if(f->y0 >= y + h || f->y1 <= y) continue;
        if(best < 0 || f->y1 < best) best = f->y1;
    }
    return best < 0 ? y : best;
}
static i32 fc_clear(int side, i32 y){
    if(!FC) return y;
    for(int i = 0; i < FC->n; i++) if(FC->f[i].side & side) if(FC->f[i].y1 > y) y = FC->f[i].y1;
    return y;
}
static i32 fc_bottom(void){
    i32 b = 0;
    if(FC) for(int i = 0; i < FC->n; i++) if(FC->f[i].y1 > b) b = FC->f[i].y1;
    return b;
}
/* posicao de um float com caixa de margem w x h, a partir de y, entre L e R */
static void fc_place(int side, i32 w, i32 h, i32 y, i32 L, i32 R, i32* ox, i32* oy){
    if(y < FC->last_top) y = FC->last_top;
    i32 l = L, r = R;
    for(int guard = 0; guard < 256; guard++){
        fc_band(y, h, L, R, &l, &r);
        if(r - l >= w || (l == L && r == R)) break;
        i32 ny = fc_below(y, h);
        if(ny <= y) break;
        y = ny;
    }
    *ox = side == 1 ? l : r - w;
    *oy = y;
    FC->last_top = y;
}
static void fc_add(int side, i32 x0, i32 y0, i32 x1, i32 y1){
    if(FC->n == FC->cap){ FC->cap = FC->cap ? FC->cap * 2 : 16; FC->f = (Flt*)realloc(FC->f, sizeof(Flt) * (size_t)FC->cap); }
    Flt* f = &FC->f[FC->n++];
    f->x0 = x0; f->x1 = x1; f->y0 = y0; f->y1 = y1; f->side = (u8)side;
}
/* a caixa cria um contexto de formatacao de bloco proprio? */
static int makes_bfc(const Node* e){
    const Style* s = e->cs;
    if(s->flow_root || s->floating || s->position == POS_ABSOLUTE || s->position == POS_FIXED) return 1;
    if(s->display != D_BLOCK && s->display != D_LIST_ITEM) return 1;
    const Node* p = e->parent;
    if(!p || p->type != N_ELEM || !p->cs) return 1;
    u8 pd = p->cs->display;
    return pd == D_FLEX || pd == D_INLINE_FLEX || pd == D_GRID || pd == D_INLINE_GRID;
}

/* ----------------------------------------------------------- display list */
static DItem* emit(int kind){
    if(!DL) return NULL;
    if(hide_depth && kind != DL_HIT && kind != DL_CLIP && kind != DL_UNCLIP) return NULL;
    if(DL->n == DL->cap){
        DL->cap = DL->cap ? DL->cap * 2 : 2048;
        DL->it = (DItem*)realloc(DL->it, sizeof(DItem) * (size_t)DL->cap);
    }
    DItem* d = &DL->it[DL->n++];
    memset(d, 0, sizeof *d);
    d->kind = (u8)kind; d->alpha = 255; d->fixed = fixed_depth > 0;
    return d;
}
static int dl_mark(void){ return DL ? DL->n : 0; }
static void dl_shift(int from, i32 dx, i32 dy){
    if(!DL || (!dx && !dy)) return;
    for(int i = from; i < DL->n; i++){
        DItem* d = &DL->it[i];
        d->x += dx; d->y += dy;
        if(d->kind == DL_HIT && d->node && d->node->lgen == lgen){ d->node->bx += dx; d->node->by += dy; }
    }
}
static void node_box(Node* n, i32 x, i32 y, i32 w, i32 h){
    if(!DL || !n) return;
    if(n->lgen != lgen){ n->lgen = lgen; n->bx = x; n->by = y; n->bw = w; n->bh = h; return; }
    i32 x1 = n->bx + n->bw, y1 = n->by + n->bh;
    if(x < n->bx) n->bx = x;
    if(y < n->by) n->by = y;
    if(x + w > x1) x1 = x + w;
    if(y + h > y1) y1 = y + h;
    n->bw = x1 - n->bx; n->bh = y1 - n->by;
}

/* --------------------------------------------------------------- estilos */
/* ladrilho de uma camada de fundo/mascara numa caixa pw x ph */
static void layer_tile(Image* im, u8 szk, Len sw, Len sh, Len px, Len py, i32 pw, i32 ph, i32* ox, i32* oy, i32* tw, i32* th){
    i32 iw = im->w > 0 ? im->w : pw, ih = im->h > 0 ? im->h : ph;
    i32 w = iw, h = ih;
    if(szk == 1 || szk == 2){
        /* cover: a maior escala; contain: a menor */
        int wide = (i64)pw * ih > (i64)ph * iw;
        if((szk == 1) == wide){ w = pw; h = (i32)((i64)ih * pw / iw); }
        else { h = ph; w = (i32)((i64)iw * ph / ih); }
    } else if(szk == 3){
        int hw = sw.kind == L_VAL, hh = sh.kind == L_VAL;
        if(hw) w = len_res(sw, pw);
        if(hh) h = len_res(sh, ph);
        if(hw && !hh) h = iw ? (i32)((i64)ih * w / iw) : h;
        if(hh && !hw) w = ih ? (i32)((i64)iw * h / ih) : w;
    }
    if(w < 1) w = 1;
    if(h < 1) h = 1;
    *tw = w; *th = h;
    *ox = px.px + (i32)((i64)px.pct * (pw - w) / 10000);
    *oy = py.px + (i32)((i64)py.pct * (ph - h) / 10000);
}
static i32 res(Len l, i32 base){ return l.kind == L_VAL ? len_res(l, base) : 0; }
static FontInst* sfont(const Style* s){
    i32 px = s->font_size;
    if(s->mono) px = px * 7 / 8;
    return font_get(s->fam, s->ffb, s->weight, s->italic, px);
}
static i32 line_h(const Style* s, FontInst* f){
    if(s->line_height > 0) return s->line_height;
    if(s->line_height < 0) return s->font_size * (-s->line_height) / 100;
    return font_ascent(f) + font_descent(f) + font_line_gap(f);
}
static int is_blocklevel(const Style* s){
    switch(s->display){
    case D_BLOCK: case D_LIST_ITEM: case D_FLEX: case D_GRID: case D_TABLE:
    case D_ROW_GROUP: case D_ROW: case D_CELL: case D_CAPTION: return 1;
    }
    return 0;
}
static int is_replaced(const Node* e){
    const char* t = e->tag;
    if(!t) return 0;
    switch(t[0]){
    case 'i': return !strcmp(t, "img") || !strcmp(t, "input") || !strcmp(t, "iframe");
    case 's': return !strcmp(t, "select") || !strcmp(t, "svg");
    case 't': return !strcmp(t, "textarea");
    case 'v': return !strcmp(t, "video");
    case 'c': return !strcmp(t, "canvas");
    case 'e': return !strcmp(t, "embed");
    case 'o': return !strcmp(t, "object");
    case 'a': return !strcmp(t, "audio");
    }
    return 0;
}
static const char* input_type(const Node* e){
    const char* t = dom_attr(e, "type");
    return t ? t : "text";
}
static int input_is_button(const Node* e){
    const char* t = input_type(e);
    return str_ieq(t, "submit") || str_ieq(t, "button") || str_ieq(t, "reset") || str_ieq(t, "image");
}
static const char* input_label(const Node* e){
    const char* v = dom_attr(e, "value");
    if(v) return v;
    const char* t = input_type(e);
    return str_ieq(t, "reset") ? "Limpar" : str_ieq(t, "submit") ? "Enviar" : "";
}
static i32 attr_px(const Node* e, const char* name){
    const char* v = dom_attr(e, name);
    if(!v || !*v || v[strlen(v) - 1] == '%') return -1;
    return atoi(v);
}

/* tamanho do conteudo de um elemento substituido */
static void replaced_size(Node* e, i32 cbw, i32* w, i32* h){
    Style* s = e->cs;
    FontInst* f = sfont(s);
    i32 lh = line_h(s, f);
    i32 cw = s->width.kind == L_VAL ? len_res(s->width, cbw) : -1;
    i32 ch = s->height.kind == L_VAL && !s->height.pct ? s->height.px : -1;
    i32 hpad = res(s->padding[1], cbw) + res(s->padding[3], cbw) + s->border_w[1] + s->border_w[3];
    i32 vpad = res(s->padding[0], cbw) + res(s->padding[2], cbw) + s->border_w[0] + s->border_w[2];
    if(s->box_border){ if(cw >= 0) cw -= hpad; if(ch >= 0) ch -= vpad; if(cw < 0 && s->width.kind == L_VAL) cw = 0; }
    const char* t = e->tag;
    i32 iw = 0, ih = 0;
    if(!strcmp(t, "img") || (!strcmp(t, "input") && str_ieq(input_type(e), "image"))){
        if(e->img && e->img->state == 1){ iw = e->img->w; ih = e->img->h; }
        if(cw < 0) cw = attr_px(e, "width");
        if(ch < 0) ch = attr_px(e, "height");
        if(cw < 0 && ch < 0){
            if(iw){ cw = iw; ch = ih; }
            else {
                const char* alt = dom_attr(e, "alt");
                cw = alt && *alt ? font_width(f, alt, (int)strlen(alt)) + 4 : 0;
                ch = alt && *alt ? lh : 0;
            }
        } else if(cw < 0) cw = iw && ih ? ch * iw / ih : ch;
        else if(ch < 0) ch = iw ? cw * ih / iw : (dom_attr(e, "alt") && *dom_attr(e, "alt") ? lh : 0);
    } else if(!strcmp(t, "input")){
        const char* ty = input_type(e);
        if(str_ieq(ty, "checkbox") || str_ieq(ty, "radio")){ if(cw < 0) cw = 13; if(ch < 0) ch = 13; }
        else if(input_is_button(e)){
            const char* l = input_label(e);
            if(cw < 0) cw = font_width(f, l, (int)strlen(l));
            if(ch < 0) ch = lh;
        } else {
            i32 sz = attr_px(e, "size");
            if(cw < 0) cw = sz > 0 ? sz * font_px(f) / 2 + 8 : font_px(f) * 11;
            if(ch < 0) ch = lh;
        }
    } else if(!strcmp(t, "select")){
        i32 mw = 40;
        for(Node* o = e->first; o; o = dom_next_in_tree(o, e)){
            if(!dom_is(o, "option")) continue;
            Buf b = {0}; dom_text_content(o, &b);
            i32 w2 = b.n ? font_width(f, b.p, b.n) : 0;
            if(w2 > mw) mw = w2;
            buf_free(&b);
        }
        if(cw < 0) cw = mw + 22;
        if(ch < 0) ch = lh;
    } else if(!strcmp(t, "textarea")){
        i32 cols = attr_px(e, "cols"), rows = attr_px(e, "rows");
        if(cw < 0) cw = (cols > 0 ? cols : 20) * font_px(f) / 2;
        if(ch < 0) ch = (rows > 0 ? rows : 2) * lh;
    } else if(!strcmp(t, "svg")){
        if(cw < 0) cw = attr_px(e, "width");
        if(ch < 0) ch = attr_px(e, "height");
        if(cw < 0 && ch < 0 && e->img && e->img->state == 1){ cw = e->img->w; ch = e->img->h; }
        const char* vb = dom_attr(e, "viewbox");
        i32 vw = 0, vh = 0;
        if(vb){ char* p; strtol(vb, &p, 10); strtol(p, &p, 10); vw = (i32)strtol(p, &p, 10); vh = (i32)strtol(p, &p, 10); }
        if(cw < 0 && ch < 0){ cw = 0; ch = 0; }
        else if(cw < 0) cw = vw && vh ? ch * vw / vh : ch;
        else if(ch < 0) ch = vw && vh ? cw * vh / vw : cw;
    } else {
        if(cw < 0) cw = attr_px(e, "width");
        if(ch < 0) ch = attr_px(e, "height");
        if(cw < 0) cw = !strcmp(t, "audio") ? 300 : 300;
        if(ch < 0) ch = !strcmp(t, "audio") ? 32 : 150;
    }
    /* max-width: 100% e companhia, mantendo a proporcao */
    if(s->max_w.kind == L_VAL && cbw < BIG / 2){
        i32 m = len_res(s->max_w, cbw) - (s->box_border ? hpad : 0);
        if(m >= 0 && cw > m){ if(s->height.kind != L_VAL && cw) ch = ch * m / cw; cw = m; }
    }
    if(cw < 0) cw = 0;
    if(ch < 0) ch = 0;
    *w = cw; *h = ch;
}

static void emit_text(i32 x, i32 y, FontInst* f, const char* s, int n, u32 color, Node* node){
    DItem* d = emit(DL_TEXT);
    if(!d) return;
    d->x = x; d->y = y; d->w = font_width(f, s, n); d->h = font_ascent(f) + font_descent(f);
    d->font = f; d->s = s; d->n = n; d->color = color; d->node = node;
}

/* conteudo de controles e imagens */
static void emit_replaced(Node* e, i32 cx, i32 cy, i32 cw, i32 ch){
    if(!DL) return;
    Style* s = e->cs;
    FontInst* f = sfont(s);
    i32 asc = font_ascent(f), fh = asc + font_descent(f);
    i32 ty = cy + (ch - fh) / 2;
    const char* t = e->tag;
    u32 col = s->color;
    if(!s->visibility) return;
    if(!strcmp(t, "svg")){
        if(e->img && e->img->state == 1 && cw > 0 && ch > 0){
            DItem* d = emit(DL_IMAGE);
            if(d){ d->x = cx; d->y = cy; d->w = cw; d->h = ch; d->img = e->img; d->node = e; d->alpha = (u8)s->opacity; }
        }
        return;
    }
    if(!strcmp(t, "img") || (!strcmp(t, "input") && str_ieq(input_type(e), "image"))){
        if(e->img && e->img->state == 1){
            DItem* d = emit(DL_IMAGE);
            if(d){ d->x = cx; d->y = cy; d->w = cw; d->h = ch; d->img = e->img; d->node = e; d->alpha = (u8)s->opacity; }
        } else {
            const char* alt = dom_attr(e, "alt");
            if(alt && *alt) emit_text(cx + 2, ty, f, alt, (int)strlen(alt), 0xFF808080, e);
            else if(cw > 8 && ch > 8){
                DItem* d = emit(DL_BORDER);
                if(d){ d->x = cx; d->y = cy; d->w = cw; d->h = ch; for(int k = 0; k < 4; k++){ d->bw[k] = 1; d->bc[k] = 0xFFD0D0D8; } }
            }
        }
        return;
    }
    if(!strcmp(t, "input")){
        const char* ty2 = input_type(e);
        if(str_ieq(ty2, "checkbox") || str_ieq(ty2, "radio")){
            if(e->checked || (e->checked == 0 && dom_attr(e, "checked") && !e->value)){
                DItem* d = emit(DL_RECT);
                if(d){ d->x = cx + 3; d->y = cy + 3; d->w = cw - 6; d->h = ch - 6; d->color = 0xFF1A73E8; d->radius = str_ieq(ty2, "radio") ? 4 : 1; }
            }
            return;
        }
        const char* txt;
        int pw = 0;
        if(input_is_button(e)) txt = input_label(e);
        else {
            txt = e->value ? e->value : dom_attr(e, "value");
            if(!txt || !*txt){ txt = dom_attr(e, "placeholder"); col = 0xFF8A8A90; }
            else if(str_ieq(ty2, "password")) pw = 1;
        }
        if(txt && *txt){
            if(pw){
                int n = (int)strlen(txt); if(n > 64) n = 64;
                char* dots = (char*)arena_alloc(&frame_arena, (size_t)n * 3 + 1);
                for(int k = 0; k < n; k++) memcpy(dots + k * 3, "\xE2\x80\xA2", 3);
                emit_text(cx, ty, f, dots, n * 3, col, e);
            } else {
                i32 tw = font_width(f, txt, (int)strlen(txt));
                i32 tx = input_is_button(e) ? cx + (cw - tw) / 2 : cx;
                DItem* c = emit(DL_CLIP); if(c){ c->x = cx; c->y = cy; c->w = cw; c->h = ch; }
                emit_text(tx, ty, f, txt, (int)strlen(txt), col, e);
                emit(DL_UNCLIP);
            }
        }
        if(doc && doc->focus == e){
            DItem* d = emit(DL_CARET);
            const char* v = e->value ? e->value : "";
            if(d){ d->x = cx + font_width(f, v, (int)strlen(v)); d->y = ty; d->w = 1; d->h = fh; d->color = 0xFF1A73E8; }
        }
        return;
    }
    if(!strcmp(t, "select")){
        Node* sel = NULL; Node* first = NULL;
        for(Node* o = e->first; o; o = dom_next_in_tree(o, e)){
            if(!dom_is(o, "option")) continue;
            if(!first) first = o;
            if(dom_attr(o, "selected") || o->checked){ sel = o; break; }
        }
        if(!sel) sel = first;
        if(sel){
            Buf b = {0}; dom_text_content(sel, &b);
            if(b.n){ char* txt = arena_strndup(&frame_arena, b.p, (size_t)b.n); emit_text(cx, ty, f, txt, b.n, col, e); }
            buf_free(&b);
        }
        emit_text(cx + cw - font_width(f, "\xE2\x96\xBE", 3), ty, f, "\xE2\x96\xBE", 3, col, e);
        return;
    }
    if(!strcmp(t, "textarea")){
        const char* v = e->value;
        Buf b = {0};
        if(!v){ dom_text_content(e, &b); v = b.p; }
        if(v && *v){
            char* txt = arena_strdup(&frame_arena, v);
            i32 yy = cy;
            DItem* c = emit(DL_CLIP); if(c){ c->x = cx; c->y = cy; c->w = cw; c->h = ch; }
            for(char* p = txt; *p && yy < cy + ch;){
                char* nl = strchr(p, '\n');
                int len = nl ? (int)(nl - p) : (int)strlen(p);
                emit_text(cx, yy, f, p, len, col, e);
                yy += line_h(s, f);
                if(!nl) break;
                p = nl + 1;
            }
            emit(DL_UNCLIP);
        } else if((v = dom_attr(e, "placeholder"))) emit_text(cx, cy, f, v, (int)strlen(v), 0xFF8A8A90, e);
        buf_free(&b);
        return;
    }
    if(!strcmp(t, "iframe") || !strcmp(t, "video") || !strcmp(t, "embed") || !strcmp(t, "object")){
        DItem* d = emit(DL_RECT);
        if(d){ d->x = cx; d->y = cy; d->w = cw; d->h = ch; d->color = !strcmp(t, "iframe") ? 0xFFF4F4F6 : 0xFF202024; }
        const char* m = !strcmp(t, "iframe") ? "(quadro incorporado)" : "(mídia)";
        FontInst* sf = font_get(FAM_SANS, FAM_SANS, 400, 0, 13);
        emit_text(cx + (cw - font_width(sf, m, (int)strlen(m))) / 2, cy + ch / 2 - 8, sf, m, (int)strlen(m),
                  !strcmp(t, "iframe") ? 0xFF8A8A90 : 0xFFB0B0B8, e);
    }
}

/* -------------------------------------------------------------- medidas */
static void measure(Node* e, i32* mn, i32* mx){
    if(e->mgen == mgen && e->mmin >= 0){ *mn = e->mmin; *mx = e->mmax; return; }
    if(e->mgen == mgen && e->mmin == -2){ *mn = 0; *mx = 0; return; }      /* recursao */
    DList* sv = DL; DL = NULL;
    FCtx* sfc = FC; FC = NULL; intrinsic++;
    e->mgen = mgen; e->mmin = -2;
    Out o;
    layout_box(e, 0, 0, 0, -1, -1, 0, &o); i32 a = o.extent;
    layout_box(e, 0, 0, BIG, -1, -1, 0, &o); i32 b = o.extent;
    if(b < a) b = a;
    e->mmin = a; e->mmax = b;
    intrinsic--; FC = sfc;
    DL = sv;
    *mn = a; *mx = b;
}
/* largura de borda "encolher para caber" dentro de `avail` */
static i32 shrink_fit(Node* e, i32 avail){
    Style* s = e->cs;
    i32 mn, mx; measure(e, &mn, &mx);
    i32 ml = res(s->margin[3], avail), mr = res(s->margin[1], avail);
    i32 w = mx < avail ? mx : avail;
    if(w < mn) w = mn;
    w -= ml + mr;
    return w < 0 ? 0 : w;
}

/* ------------------------------------------------------- posicionamento */
typedef struct { Node* n; i32 sx, sy; } Pend;
static Pend* pend = NULL; static int npend = 0, cpend = 0;
static struct { i32 x, y, w, h; } cb;

static void queue_abs(Node* n, i32 sx, i32 sy){
    if(!DL) return;
    if(npend == cpend){ cpend = cpend ? cpend * 2 : 64; pend = (Pend*)realloc(pend, sizeof(Pend) * (size_t)cpend); }
    pend[npend].n = n; pend[npend].sx = sx; pend[npend].sy = sy; npend++;
}
static void layout_abs(Pend p){
    Node* n = p.n; Style* s = n->cs;
    i32 cx = cb.x, cy = cb.y, cw = cb.w, ch = cb.h;
    int fixed = s->position == POS_FIXED;
    if(fixed){ cx = 0; cy = 0; cw = VW; ch = VH; fixed_depth++; }
    int hasL = s->inset[3].kind == L_VAL, hasR = s->inset[1].kind == L_VAL;
    int hasT = s->inset[0].kind == L_VAL, hasB = s->inset[2].kind == L_VAL;
    i32 L = res(s->inset[3], cw), R = res(s->inset[1], cw), T = res(s->inset[0], ch), B = res(s->inset[2], ch);
    i32 ml = res(s->margin[3], cw), mr = res(s->margin[1], cw);
    i32 hpad = res(s->padding[1], cw) + res(s->padding[3], cw) + s->border_w[1] + s->border_w[3];
    i32 bw;
    if(s->width.kind == L_VAL) bw = len_res(s->width, cw) + (s->box_border ? 0 : hpad);
    else if(hasL && hasR) bw = cw - L - R - ml - mr;
    else if(is_replaced(n)){ i32 rw, rh; replaced_size(n, cw, &rw, &rh); bw = rw + hpad; }
    else bw = shrink_fit(n, cw - (hasL ? L : 0) - (hasR ? R : 0));
    if(s->max_w.kind == L_VAL){ i32 m = len_res(s->max_w, cw) + (s->box_border ? 0 : hpad); if(bw > m) bw = m; }
    if(bw < 0) bw = 0;
    i32 x = hasL ? cx + L : hasR ? cx + cw - R - bw - ml - mr : p.sx;
    if(hasL && hasR && s->margin[3].kind == L_AUTO && s->margin[1].kind == L_AUTO){
        i32 rest = cw - L - R - bw; if(rest > 0) x += rest / 2 - ml;
    }
    i32 fh = -1;
    if(hasT && hasB && s->height.kind != L_VAL) fh = ch - T - B;
    i32 y;
    if(hasT) y = cy + T;
    else if(hasB){
        Out o; DList* sv = DL; DL = NULL;
        layout_box(n, 0, 0, cw, bw, -1, 0, &o);
        DL = sv;
        y = cy + ch - B - o.h;
    } else y = p.sy;
    Out o;
    layout_box(n, x, y, cw, bw, fh, 0, &o);
    if(fixed) fixed_depth--;
}
/* move os itens [a, n) da display list para a posicao `at` (< a) */
static void dl_move_back(int a, int at){
    if(!DL || at >= a || a >= DL->n) return;
    int cnt = DL->n - a;
    DItem* tmp = (DItem*)malloc(sizeof(DItem) * (size_t)cnt);
    memcpy(tmp, DL->it + a, sizeof(DItem) * (size_t)cnt);
    memmove(DL->it + at + cnt, DL->it + at, sizeof(DItem) * (size_t)(a - at));
    memcpy(DL->it + at, tmp, sizeof(DItem) * (size_t)cnt);
    free(tmp);
}
/* posicionados do bloco: z-index negativo atras do conteudo (`behind` =
 * inicio do conteudo do bloco), os demais na ordem crescente de z-index */
static void process_pending(int from, int behind){
    int n = npend - from;
    Pend* list = (Pend*)malloc(sizeof(Pend) * (size_t)(n > 0 ? n : 1));
    memcpy(list, pend + from, sizeof(Pend) * (size_t)n);
    npend = from;
    for(int i = 1; i < n; i++){                       /* insercao estavel por z-index */
        Pend t = list[i]; int j = i;
        while(j > 0 && list[j - 1].n->cs->z_index > t.n->cs->z_index){ list[j] = list[j - 1]; j--; }
        list[j] = t;
    }
    for(int i = 0; i < n; i++){
        int mark = dl_mark();
        layout_abs(list[i]);
        if(list[i].n->cs->z_index < 0 && behind >= 0){ dl_move_back(mark, behind); behind += dl_mark() - mark; }
    }
    free(list);
}

/* ------------------------------------------------------- linhas (inline) */
typedef struct {
    u8 kind;                       /* 0 texto, 1 caixa atomica */
    u8 ul, st;
    i32 x, w, asc, desc, fasc, shift, ls;
    FontInst* font; const char* s; int n; u32 color;
    Node* node; Node* bg;
    i32 aw;                         /* atomica: largura da borda */
} Frag;
typedef struct {
    i32 x0, y, w, x;
    Frag* f; int nf, cf;
    i32 maxw;
    int space;
    u8 align;
    i32 sasc, sdesc;
    int lines;
    i32 indent;
    Node* bgnode;
    const Style* bs;
    i32 L, R;                      /* bordas do conteudo do bloco (a linha pode ser mais estreita por floats) */
    int band_ok;
    Node* defer[16]; int ndefer;   /* floats que nao couberam na linha atual: vao para a proxima */
} LB;

static void place_float(Node* c, i32 L, i32 R, i32 y);
/* faixa livre para a linha que comeca agora */
static void lb_band(LB* lb){
    if(lb->band_ok) return;
    lb->band_ok = 1;
    if(!FC || !FC->n) return;
    i32 l, r;
    fc_band(lb->y, lb->sasc + lb->sdesc, lb->L, lb->R, &l, &r);
    lb->x0 = l; lb->w = r - l;
}
/* nada coube ao lado dos floats: desce a linha ate o proximo fim de float */
static int lb_drop(LB* lb){
    if(!FC || lb->nf || lb->w >= lb->R - lb->L) return 0;
    i32 ny = fc_below(lb->y, lb->sasc + lb->sdesc);
    if(ny <= lb->y) return 0;
    lb->y = ny; lb->band_ok = 0; lb->x0 = lb->L; lb->w = lb->R - lb->L;
    lb_band(lb);
    return 1;
}

static void lb_begin(LB* lb, const Style* s, i32 x0, i32 y, i32 w){
    memset(lb, 0, sizeof *lb);
    lb->x0 = x0; lb->y = y; lb->w = w; lb->align = s->text_align; lb->bs = s;
    lb->L = x0; lb->R = x0 + w;
    FontInst* f = sfont(s);
    i32 lh = line_h(s, f), a = font_ascent(f), d = font_descent(f);
    i32 hl = (lh - (a + d)) / 2;
    lb->sasc = a + hl; lb->sdesc = lh - lb->sasc;
    lb->indent = s->indent;
    lb->x = lb->indent;
}
static Frag* lb_push(LB* lb){
    if(lb->nf == lb->cf){ lb->cf = lb->cf ? lb->cf * 2 : 32; lb->f = (Frag*)realloc(lb->f, sizeof(Frag) * (size_t)lb->cf); }
    Frag* f = &lb->f[lb->nf++];
    memset(f, 0, sizeof *f);
    return f;
}

/* o elemento que originou o sublinhado (o mais alto com a decoracao) */
static Node* deco_root(Node* n){
    while(n && n->parent && n->parent->type == N_ELEM && n->parent->cs && n->parent->cs->underline == n->cs->underline
          && n->parent->cs->strike == n->cs->strike && (n->cs->underline || n->cs->strike)) n = n->parent;
    return n;
}
static void lb_flush(LB* lb, int force){
    if(!lb->nf && !force) return;
    i32 asc = lb->sasc, desc = lb->sdesc;
    if(lb->nf && !force){
        /* linha so com caixas atomicas: a "strut" nao precisa empurrar */
    }
    for(int i = 0; i < lb->nf; i++){
        Frag* f = &lb->f[i];
        if(f->asc - f->shift > asc) asc = f->asc - f->shift;
        if(f->desc + f->shift > desc) desc = f->desc + f->shift;
    }
    i32 lw = lb->x;
    i32 dx = 0, free_w = lb->w - lw;
    if(free_w > 0 && lb->w < BIG / 2){
        if(lb->align == TA_CENTER) dx = free_w / 2;
        else if(lb->align == TA_RIGHT) dx = free_w;
    }
    i32 base = lb->y + asc;
    if(lb->nf) g_base = base;
    if(DL){
        /* fundos de elementos inline */
        for(int i = 0; i < lb->nf; i++){
            Frag* f = &lb->f[i];
            if(!f->bg) continue;
            int j = i; while(j + 1 < lb->nf && lb->f[j + 1].bg == f->bg) j++;
            Style* bs = f->bg->cs;
            i32 pl = res(bs->padding[3], lb->w) + bs->border_w[3], pr = res(bs->padding[1], lb->w) + bs->border_w[1];
            i32 pt = res(bs->padding[0], lb->w) + bs->border_w[0], pb = res(bs->padding[2], lb->w) + bs->border_w[2];
            FontInst* bf = sfont(bs);
            i32 top = base - font_ascent(bf) - pt, h = font_ascent(bf) + font_descent(bf) + pt + pb;
            i32 x = lb->x0 + dx + f->x - pl, w = lb->f[j].x + lb->f[j].w - f->x + pl + pr;
            if((bs->bg >> 24) && !bs->masked){
                DItem* d = emit(DL_RECT);
                if(d){ d->x = x; d->y = top; d->w = w; d->h = h; d->color = bs->bg; d->radius = bs->radius; d->node = f->bg; }
            }
            if(bs->border_w[0] || bs->border_w[1] || bs->border_w[2] || bs->border_w[3]){
                DItem* d = emit(DL_BORDER);
                if(d){ d->x = x; d->y = top; d->w = w; d->h = h; d->radius = bs->radius;
                    for(int k = 0; k < 4; k++){ d->bw[k] = (u8)bs->border_w[k]; d->bc[k] = bs->border_c[k]; } }
            }
            node_box(f->bg, x, top, w, h);
            i = j;
        }
        for(int i = 0; i < lb->nf; i++){
            Frag* f = &lb->f[i];
            i32 x = lb->x0 + dx + f->x;
            if(f->kind == 0){
                i32 ty = base + f->shift - f->fasc;
                if(f->color >> 24){
                    DItem* d = emit(DL_TEXT);
                    if(d){ d->x = x; d->y = ty; d->w = f->w; d->h = f->fasc + font_descent(f->font);
                        d->font = f->font; d->s = f->s; d->n = f->n; d->color = f->color; d->node = f->node; d->ls = (i16)f->ls; }
                    i32 px = font_px(f->font), th = px >= 20 ? 2 : 1;
                    /* a linha continua pelo espaco ate a proxima palavra decorada da mesma cor */
                    i32 dw = f->w;
                    if(i + 1 < lb->nf){
                        Frag* nx = &lb->f[i + 1];
                        if(nx->kind == 0 && nx->color == f->color && nx->ul == f->ul && nx->st == f->st && nx->x > f->x + f->w
                           && deco_root(nx->node) == deco_root(f->node)) dw = nx->x - f->x;
                    }
                    if(f->ul){ DItem* u = emit(DL_RECT); if(u){ u->x = x; u->y = base + f->shift + (px + 9) / 10; u->w = dw; u->h = th; u->color = f->color; } }
                    if(f->st){ DItem* u = emit(DL_RECT); if(u){ u->x = x; u->y = base + f->shift - font_ascent(f->font) * 3 / 10; u->w = dw; u->h = th; u->color = f->color; } }
                }
                /* caixa do texto para o nodo e seus ancestrais inline */
                i32 hh = f->fasc + font_descent(f->font);
                for(Node* n = f->node; n && n->cs; n = n->parent){
                    node_box(n, x, ty, f->w, hh);
                    if(n->cs->display != D_INLINE) break;
                }
            } else {
                Out o;
                layout_box(f->node, x, base + f->shift - f->asc, lb->w, f->aw, -1, 0, &o);
            }
        }
    }
    if(lw > lb->maxw) lb->maxw = lw;
    lb->y += asc + desc;
    lb->nf = 0; lb->x = 0; lb->space = 0; lb->lines++;
    lb->x0 = lb->L; lb->w = lb->R - lb->L; lb->band_ok = 0;
    if(lb->ndefer){
        int nd = lb->ndefer; lb->ndefer = 0;
        for(int i = 0; i < nd; i++) place_float(lb->defer[i], lb->L, lb->R, lb->y);
    }
}
static void lb_end(LB* lb){
    lb_flush(lb, 0);
    if(lb->ndefer){ int nd = lb->ndefer; lb->ndefer = 0; for(int i = 0; i < nd; i++) place_float(lb->defer[i], lb->L, lb->R, lb->y); }
    free(lb->f); lb->f = NULL; lb->cf = 0;
}

static void text_frag(LB* lb, const Style* st, FontInst* f, const char* s, int n, i32 w, Node* node, i32 sp){
    Frag* fr = lb_push(lb);
    i32 lh = line_h(st, f), a = font_ascent(f), d = font_descent(f);
    i32 hl = (lh - (a + d)) / 2;
    fr->kind = 0; fr->x = lb->x + sp; fr->w = w; fr->asc = a + hl; fr->desc = lh - fr->asc; fr->fasc = a;
    fr->font = f; fr->s = s; fr->n = n; fr->ls = st->letter_spacing;
    fr->color = st->visibility ? st->color : 0;
    if(st->opacity < 255 && (fr->color >> 24)) fr->color = (fr->color & 0xFFFFFF) | ((u32)((fr->color >> 24) * (u32)st->opacity / 255) << 24);
    fr->ul = st->underline; fr->st = st->strike; fr->node = node; fr->bg = lb->bgnode;
    if(st->valign == VA_SUB) fr->shift = st->font_size / 3;
    else if(st->valign == VA_SUPER) fr->shift = -(st->font_size / 2);
    lb->x = fr->x + w;
    lb->space = 0;
}

/* transforma maiusculas/minusculas (ASCII + Latin-1) */
static const char* transform(const Style* st, const char* s, int n, int word_start){
    if(!st->upper && !st->lower && !st->capitalize) return s;
    char* o = (char*)arena_alloc(&frame_arena, (size_t)n + 1);
    memcpy(o, s, (size_t)n);
    for(int i = 0; i < n; i++){
        int up = st->upper || (st->capitalize && i == 0 && word_start);
        u8 c = (u8)o[i];
        if(c == 0xC3 && i + 1 < n){
            u8 d = (u8)o[i + 1];
            if(up && d >= 0xA0 && d <= 0xBE && d != 0xB7) o[i + 1] = (char)(d - 0x20);
            else if(st->lower && d >= 0x80 && d <= 0x9E && d != 0x97) o[i + 1] = (char)(d + 0x20);
            i++; continue;
        }
        if(up && c >= 'a' && c <= 'z') o[i] = (char)(c - 32);
        else if(st->lower && c >= 'A' && c <= 'Z') o[i] = (char)(c + 32);
    }
    return o;
}

static void lb_word(LB* lb, const Style* st, FontInst* f, const char* s, int n, Node* node, int can_wrap){
    s = transform(st, s, n, 1);
    i32 w = font_width(f, s, n);
    if(st->letter_spacing){ int cps = 0; for(int i = 0; i < n; i++) if(((u8)s[i] & 0xC0) != 0x80) cps++; w += st->letter_spacing * cps; }
    lb_band(lb);
    i32 sp = lb->space && lb->nf ? font_width(f, " ", 1) : 0;
    if(lb->x + sp + w > lb->w && lb->nf && can_wrap){ lb_flush(lb, 0); sp = 0; lb_band(lb); }
    while(!lb->nf && lb->x + w > lb->w && lb_drop(lb)){}
    if(st->break_words && w > lb->w - lb->x && lb->w > 0 && n > 1){
        /* quebra a palavra no meio (word-break / overflow-wrap) */
        int i = 0;
        while(i < n){
            int j = i; i32 ww = 0;
            while(j < n){
                int k = j; utf8_dec(s, n, &k);
                i32 cw = font_width(f, s + j, k - j);
                if(lb->x + sp + ww + cw > lb->w && j > i) break;
                ww += cw; j = k;
            }
            text_frag(lb, st, f, s + i, j - i, ww, node, sp);
            sp = 0;
            i = j;
            if(i < n) lb_flush(lb, 0);
        }
        return;
    }
    text_frag(lb, st, f, s, n, w, node, sp);
}

static void lb_text(LB* lb, Node* t, const Style* st, Node* el){
    FontInst* f = sfont(st);
    const char* s = t->text; int n = t->tlen;
    int ws = st->white_space;
    lb_band(lb);
    if(ws == WS_PRE || ws == WS_PRE_WRAP){
        int i = 0;
        while(i <= n){
            int j = i; while(j < n && s[j] != '\n') j++;
            /* tabulacoes -> espacos */
            int tabs = 0; for(int k = i; k < j; k++) if(s[k] == '\t') tabs++;
            const char* seg = s + i; int sl = j - i;
            if(tabs){
                char* o = (char*)arena_alloc(&frame_arena, (size_t)(sl + tabs * 8 + 1)); int on = 0;
                for(int k = i; k < j; k++){ if(s[k] == '\t'){ int c = 8 - on % 8; while(c--) o[on++] = ' '; } else o[on++] = s[k]; }
                seg = o; sl = on;
            }
            if(ws == WS_PRE){
                if(sl){ lb->space = 0; i32 w = font_width(f, seg, sl); text_frag(lb, st, f, seg, sl, w, el, 0); }
            } else {
                int k = 0;
                while(k < sl){
                    int e = k; while(e < sl && seg[e] != ' ') e++;
                    while(e < sl && seg[e] == ' ') e++;
                    i32 w = font_width(f, seg + k, e - k);
                    if(lb->x + w > lb->w && lb->nf) lb_flush(lb, 0);
                    text_frag(lb, st, f, seg + k, e - k, w, el, 0);
                    k = e;
                }
            }
            if(j < n) lb_flush(lb, 1);
            i = j + 1;
            if(j >= n) break;
        }
        return;
    }
    int can_wrap = ws != WS_NOWRAP;
    int i = 0;
    while(i < n){
        u8 c = (u8)s[i];
        if(is_ws(c)){
            if(c == '\n' && ws == WS_PRE_LINE) lb_flush(lb, 1);
            else if(lb->nf || lb->x > lb->indent) lb->space = 1;
            i++; continue;
        }
        int j = i;
        while(j < n && !is_ws((u8)s[j])){
            /* quebra opcional depois de '-' e '/' em palavras longas nao e feita */
            j++;
        }
        lb_word(lb, st, f, s + i, j - i, el, can_wrap);
        i = j;
    }
}

static void lb_node(LB* lb, Node* n, const Style* parent_st);

static void lb_atomic(LB* lb, Node* e){
    Style* s = e->cs;
    lb_band(lb);
    i32 bw;
    if(is_replaced(e) || s->width.kind == L_VAL) bw = -1;
    else bw = shrink_fit(e, lb->w);
    Out o; DList* sv = DL; DL = NULL;
    layout_box(e, 0, 0, lb->w, bw, -1, 0, &o);
    DL = sv;
    i32 ml = res(s->margin[3], lb->w), mr = res(s->margin[1], lb->w);
    i32 adv = o.w + ml + mr;
    i32 sp = lb->space && lb->nf ? font_width(sfont(lb->bs), " ", 1) : 0;
    if(lb->x + sp + adv > lb->w && lb->nf) { lb_flush(lb, 0); sp = 0; lb_band(lb); }
    while(!lb->nf && lb->x + adv > lb->w && lb_drop(lb)){}
    Frag* f = lb_push(lb);
    f->kind = 1; f->x = lb->x + sp + ml; f->w = o.w; f->aw = o.w; f->node = e;
    f->x -= ml;                     /* layout_box recebe a borda da margem */
    i32 h = o.h;
    if(s->valign == VA_MIDDLE){ f->asc = h / 2 + font_px(sfont(lb->bs)) / 4; f->desc = h - f->asc; }
    else if(s->valign == VA_TOP || s->valign == VA_BOTTOM || is_replaced(e) || o.base < 0 || s->overflow_hidden){ f->asc = h; f->desc = 0; }
    else { f->asc = o.base; f->desc = h - o.base; if(f->desc < 0){ f->desc = 0; f->asc = h; } }
    lb->x = f->x + adv;
    lb->space = 0;
}

static void lb_node(LB* lb, Node* n, const Style* parent_st){
    if(n->type == N_TEXT){ lb_text(lb, n, parent_st, n->parent); return; }
    if(n->type != N_ELEM || !n->cs) return;
    Style* s = n->cs;
    if(s->hidden_clip) return;
    if(s->position == POS_ABSOLUTE || s->position == POS_FIXED){ queue_abs(n, lb->x0 + lb->x, lb->y); return; }
    if(dom_is(n, "br")){ lb_flush(lb, 1); if(s->clear && FC){ i32 c2 = fc_clear(s->clear, lb->y); if(c2 > lb->y){ lb->y = c2; lb->band_ok = 0; } } return; }
    if(s->floating && FC){
        lb_band(lb);
        if(!lb->nf){ place_float(n, lb->L, lb->R, lb->y); lb->band_ok = 0; lb->x0 = lb->L; lb->w = lb->R - lb->L; }
        else if(lb->ndefer < 16) lb->defer[lb->ndefer++] = n;
        return;
    }
    if(is_replaced(n) || s->floating || s->display == D_INLINE_BLOCK || s->display == D_INLINE_FLEX
        || s->display == D_INLINE_GRID || s->display == D_INLINE_TABLE){
        lb_atomic(lb, n);
        return;
    }
    if(is_blocklevel(s)){
        /* bloco dentro de inline: termina a linha e ocupa a largura toda */
        lb_flush(lb, 0);
        Out o; layout_box(n, lb->L, lb->y, lb->R - lb->L, -1, -1, 0, &o);
        lb->y += o.h;
        if(o.extent > lb->maxw) lb->maxw = o.extent;
        lb->x = 0; lb->space = 0; lb->band_ok = 0;
        return;
    }
    /* inline comum */
    lb_band(lb);
    i32 ml = res(s->margin[3], lb->w) + res(s->padding[3], lb->w) + s->border_w[3];
    i32 mr = res(s->margin[1], lb->w) + res(s->padding[1], lb->w) + s->border_w[1];
    if(ml){ if(lb->space && lb->nf){ lb->x += font_width(sfont(s), " ", 1); lb->space = 0; } lb->x += ml; }
    Node* saved = lb->bgnode;
    if((s->bg >> 24) || s->border_w[0] || s->border_w[1] || s->border_w[2] || s->border_w[3]) lb->bgnode = n;
    int hidden = s->opacity == 0;
    if(hidden) hide_depth++;
    for(Node* c = kfirst(n); c; c = knext(c)) lb_node(lb, c, s);
    if(hidden) hide_depth--;
    lb->bgnode = saved;
    lb->x += mr;
}

/* coloca um float: mede, acha o lugar entre L e R a partir de y, faz o layout */
static void place_float(Node* c, i32 L, i32 R, i32 y){
    Style* s = c->cs;
    i32 cw = R - L;
    i32 bw = -1;
    if(!is_replaced(c) && s->width.kind != L_VAL) bw = shrink_fit(c, cw);
    Out o; DList* sv = DL; DL = NULL;
    layout_box(c, 0, 0, cw, bw, -1, 0, &o);
    DL = sv;
    i32 ml = res(s->margin[3], cw), mr = res(s->margin[1], cw);
    i32 mw = o.w + ml + mr;
    if(mw < 0) mw = 0;
    if(s->clear) y = fc_clear(s->clear, y);
    i32 fx, fy;
    fc_place(s->floating, mw, o.h, y, L, R, &fx, &fy);
    Out o2; layout_box(c, fx, fy, cw, o.w, -1, 0, &o2);
    fc_add(s->floating, fx, fy, fx + mw, fy + o2.h);
}

/* --------------------------------------------------------- fluxo de blocos */
static int all_ws(const Node* t){
    for(int i = 0; i < t->tlen; i++) if(!is_ws((u8)t->text[i])) return 0;
    return 1;
}

static i32 layout_flow(Node* e, i32 cx, i32 cy, i32 cw, i32* extent){
    Style* s = e->cs;
    /* "tabela" sem linhas (ex.: <figure> com display:table): a legenda nao
     * alarga a caixa alem do seu min-content */
    int table_like = s->display == D_TABLE || s->display == D_INLINE_TABLE;
    LB lb; int on = 0;
    i32 y = cy, prev_mb = 0;
    for(Node* c = kfirst(e); c; c = knext(c)){
        if(c->type == N_TEXT){
            if(!on){
                if(all_ws(c) && s->white_space != WS_PRE && s->white_space != WS_PRE_WRAP) continue;
                lb_begin(&lb, s, cx, y, cw); on = 1; prev_mb = 0;
            }
            lb_text(&lb, c, s, e);
            continue;
        }
        if(c->type != N_ELEM || !c->cs) continue;
        Style* cs = c->cs;
        if(cs->hidden_clip) continue;
        if(cs->position == POS_ABSOLUTE || cs->position == POS_FIXED){
            queue_abs(c, on ? cx + lb.x : cx, on ? lb.y : y);
            continue;
        }
        if(cs->floating && FC && !on){ place_float(c, cx, cx + cw, y); continue; }
        if(is_blocklevel(cs) && !cs->floating){
            if(on){ lb_end(&lb); y = lb.y; if(lb.maxw > *extent) *extent = lb.maxw; on = 0; prev_mb = 0; }
            i32 mt = res(cs->margin[0], cw);
            i32 col = prev_mb > 0 && mt > 0 ? (prev_mb < mt ? prev_mb : mt) : 0;
            i32 yy = y - col, bx = cx, bcw = cw;
            if(FC && FC->n){
                if(cs->clear){
                    i32 c2 = fc_clear(cs->clear, yy + mt);
                    if(c2 > yy + mt){ yy = c2 - mt; }
                }
                if(makes_bfc(c)){
                    /* caixa com contexto proprio: fica ao lado dos floats, sem cobri-los */
                    i32 need = 0;
                    if(cs->width.kind == L_VAL && !cs->width.pct) need = cs->width.px;
                    for(int guard = 0; guard < 64; guard++){
                        i32 l, r; fc_band(yy + mt, 1, cx, cx + cw, &l, &r);
                        bx = l; bcw = r - l;
                        if((l == cx && r == cx + cw) || (bcw >= need && bcw > 0)) break;
                        i32 ny = fc_below(yy + mt, 1);
                        if(ny <= yy + mt) break;
                        yy = ny - mt;
                    }
                }
            }
            Out o; layout_box(c, bx, yy, bcw, -1, -1, 0, &o);
            y = yy + o.h;
            prev_mb = res(cs->margin[2], cw);
            i32 ext = o.extent;
            if(table_like && cs->display == D_CAPTION){ i32 mn, mx; measure(c, &mn, &mx); ext = mn; }
            if(ext > *extent) *extent = ext;
            continue;
        }
        if(!on){ lb_begin(&lb, s, cx, y, cw); on = 1; prev_mb = 0; }
        lb_node(&lb, c, s);
    }
    if(on){ lb_end(&lb); y = lb.y; if(lb.maxw > *extent) *extent = lb.maxw; }
    return y - cy;
}

/* ------------------------------------------------------------------- flex */
typedef struct {
    Node* n; int text;
    i32 basis, mn, mx, maxw, w, h, ml, mr, mt, mb;
    i32 grow, shrink;
    int ml_auto, mr_auto;
    u8 align;
    i32 order;
} FItem;

static i32 text_item_layout(Node* t, const Style* ps, Node* el, i32 x, i32 y, i32 w, i32* used){
    LB lb; lb_begin(&lb, ps, x, y, w);
    lb.align = TA_LEFT;
    lb_text(&lb, t, ps, el);
    lb_end(&lb);
    if(used) *used = lb.maxw;
    return lb.y - y;
}
static void item_measure(FItem* it, const Style* ps, Node* parent){
    if(it->text){
        DList* sv = DL; DL = NULL;
        text_item_layout(it->n, ps, parent, 0, 0, 0, &it->mn);
        text_item_layout(it->n, ps, parent, 0, 0, BIG, &it->mx);
        DL = sv;
    } else measure(it->n, &it->mn, &it->mx);
}

static int collect_items(Node* e, FItem** out, i32 cw){
    int n = 0, cap = 16;
    FItem* it = (FItem*)malloc(sizeof(FItem) * (size_t)cap);
    for(Node* c = kfirst(e); c; c = knext(c)){
        int text = c->type == N_TEXT && !all_ws(c);
        if(!text && (c->type != N_ELEM || !c->cs || c->cs->hidden_clip)) continue;
        if(!text && (c->cs->position == POS_ABSOLUTE || c->cs->position == POS_FIXED)){ queue_abs(c, 0, 0); continue; }
        if(n == cap){ cap *= 2; it = (FItem*)realloc(it, sizeof(FItem) * (size_t)cap); }
        FItem* f = &it[n++];
        memset(f, 0, sizeof *f);
        f->n = c; f->text = text; f->grow = 0; f->shrink = 100; f->maxw = BIG; f->align = 255;
        if(!text){
            Style* s = c->cs;
            f->ml = res(s->margin[3], cw); f->mr = res(s->margin[1], cw);
            f->mt = res(s->margin[0], cw); f->mb = res(s->margin[2], cw);
            f->ml_auto = s->margin[3].kind == L_AUTO; f->mr_auto = s->margin[1].kind == L_AUTO;
            f->grow = s->flex_grow; f->shrink = s->flex_shrink; f->align = s->align_self; f->order = s->order;
        }
    }
    /* order: insercao estavel */
    for(int i = 1; i < n; i++){
        FItem t = it[i]; int j = i;
        while(j > 0 && it[j - 1].order > t.order){ it[j] = it[j - 1]; j--; }
        it[j] = t;
    }
    *out = it;
    return n;
}

static i32 item_layout(FItem* it, const Style* ps, Node* parent, i32 x, i32 y, i32 cw, i32 w, i32 fh){
    if(it->text) return text_item_layout(it->n, ps, parent, x + it->ml, y, w, NULL);
    Out o;
    layout_box(it->n, x, y, cw, w, fh, 0, &o);
    return o.h - it->mt - it->mb;
}

static i32 layout_flex(Node* e, i32 cx, i32 cy, i32 cw, i32* extent){
    Style* s = e->cs;
    FItem* it; int n = collect_items(e, &it, cw);
    i32 gap_c = res(s->gap_col, cw), gap_r = res(s->gap_row, cw);
    i32 y = cy;
    if(s->flex_dir_col){
        for(int i = 0; i < n; i++){
            FItem* f = &it[i];
            u8 al = f->align != 255 ? f->align : s->align_items;
            i32 w;
            int auto_w = f->text || f->n->cs->width.kind != L_VAL;
            if(f->text){ item_measure(f, s, e); w = f->mx < cw ? f->mx : cw; }
            else if(al == AI_STRETCH && auto_w) w = cw - f->ml - f->mr;
            else if(auto_w && !is_replaced(f->n)) w = shrink_fit(f->n, cw);
            else w = -1;
            i32 x = cx;
            if(w >= 0 && (al == AI_CENTER || al == AI_END)){
                i32 rest = cw - w - f->ml - f->mr;
                if(rest > 0) x += al == AI_CENTER ? rest / 2 : rest;
            }
            if(i) y += gap_r;
            i32 h = item_layout(f, s, e, x, y, cw, w, -1);
            y += h + f->mt + f->mb;
            i32 ext = 0;
            if(!f->text){ i32 mn, mx; measure(f->n, &mn, &mx); ext = cw >= BIG / 2 ? mx : mn; }
            else ext = cw >= BIG / 2 ? f->mx : f->mn;
            if(ext > *extent) *extent = ext;
        }
        free(it);
        return y - cy;
    }
    /* direcao em linha */
    for(int i = 0; i < n; i++){
        FItem* f = &it[i];
        item_measure(f, s, e);
        if(f->text){ f->basis = f->mx; continue; }
        Style* is = f->n->cs;
        i32 hpad = res(is->padding[1], cw) + res(is->padding[3], cw) + is->border_w[1] + is->border_w[3];
        if(is->flex_basis.kind == L_VAL && !(is->flex_basis.pct && cw >= BIG / 2))
            f->basis = len_res(is->flex_basis, cw) + (is->box_border ? 0 : hpad);
        else if(is->width.kind == L_VAL && !(is->width.pct && cw >= BIG / 2))
            f->basis = len_res(is->width, cw) + (is->box_border ? 0 : hpad);
        else f->basis = f->mx - f->ml - f->mr;
        f->mn -= f->ml + f->mr;
        if(is->min_w.kind == L_VAL) f->mn = len_res(is->min_w, cw) + (is->box_border ? 0 : hpad);
        else if(is->overflow_hidden) f->mn = 0;
        if(is->max_w.kind == L_VAL) f->maxw = len_res(is->max_w, cw) + (is->box_border ? 0 : hpad);
        if(f->basis > f->maxw) f->basis = f->maxw;
        if(f->mn > f->maxw) f->mn = f->maxw;
        if(f->basis < 0) f->basis = 0;
    }
    int start = 0;
    while(start < n){
        /* monta uma linha */
        int end = start; i32 used = 0;
        while(end < n){
            i32 need = it[end].basis + it[end].ml + it[end].mr + (end > start ? gap_c : 0);
            if(s->flex_wrap && end > start && used + need > cw) break;
            used += need; end++;
        }
        i32 free_w = cw - used;
        /* encolhe ou cresce */
        if(free_w < 0){
            i64 tot = 0;
            for(int i = start; i < end; i++) tot += (i64)it[i].shrink * it[i].basis;
            for(int i = start; i < end; i++){
                FItem* f = &it[i];
                i32 w = f->basis;
                if(tot > 0) w = f->basis + (i32)((i64)free_w * f->shrink * f->basis / tot);
                if(w < f->mn) w = f->mn;
                f->w = w;
            }
        } else {
            for(int i = start; i < end; i++) it[i].w = it[i].basis < it[i].mn ? it[i].mn : it[i].basis;
        }
        i32 line_ext = 0;
        for(int i = start; i < end; i++) line_ext += it[i].w + it[i].ml + it[i].mr + (i > start ? gap_c : 0);
        if(line_ext > *extent) *extent = line_ext;
        free_w = cw - line_ext;
        i32 grow = 0;
        for(int i = start; i < end; i++) grow += it[i].grow;
        if(free_w > 0 && grow > 0 && cw < BIG / 2){
            i32 left = free_w;
            for(int i = start; i < end; i++){
                if(!it[i].grow) continue;
                i32 add = (i32)((i64)free_w * it[i].grow / grow);
                if(it[i].w + add > it[i].maxw) add = it[i].maxw - it[i].w;
                it[i].w += add; left -= add;
            }
            free_w = left > 0 ? left : 0;
        }
        /* margens auto absorvem o espaco livre */
        int nauto = 0;
        for(int i = start; i < end; i++) nauto += it[i].ml_auto + it[i].mr_auto;
        i32 x = cx, between = 0;
        if(free_w > 0 && cw < BIG / 2){
            if(nauto){
                i32 each = free_w / nauto;
                for(int i = start; i < end; i++){ if(it[i].ml_auto) it[i].ml += each; if(it[i].mr_auto) it[i].mr += each; }
            } else {
                int cnt = end - start;
                switch(s->justify){
                case JC_END: x += free_w; break;
                case JC_CENTER: x += free_w / 2; break;
                case JC_BETWEEN: if(cnt > 1) between = free_w / (cnt - 1); break;
                case JC_AROUND: between = free_w / cnt; x += between / 2; break;
                case JC_EVENLY: between = free_w / (cnt + 1); x += between; break;
                }
            }
        }
        /* alturas (medindo) */
        i32 line_h2 = 0;
        for(int i = start; i < end; i++){
            DList* sv = DL; DL = NULL;
            it[i].h = item_layout(&it[i], s, e, 0, 0, cw, it[i].w, -1);
            DL = sv;
            i32 hh = it[i].h + it[i].mt + it[i].mb;
            if(hh > line_h2) line_h2 = hh;
        }
        if(s->height.kind == L_VAL && !s->height.pct && !s->flex_wrap){
            i32 hh = s->height.px - (s->box_border ? res(s->padding[0], cw) + res(s->padding[2], cw) + s->border_w[0] + s->border_w[2] : 0);
            if(hh > line_h2) line_h2 = hh;
        }
        for(int i = start; i < end; i++){
            FItem* f = &it[i];
            u8 al = f->align != 255 ? f->align : s->align_items;
            i32 yy = y, fh = -1;
            i32 room = line_h2 - f->h - f->mt - f->mb;
            if(al == AI_CENTER) yy += room / 2;
            else if(al == AI_END) yy += room;
            else if(al == AI_STRETCH && !f->text && f->n->cs->height.kind != L_VAL) fh = line_h2 - f->mt - f->mb;
            item_layout(f, s, e, x, yy, cw, f->w, fh);
            x += f->ml + f->w + f->mr + gap_c + between;
        }
        y += line_h2;
        if(end < n) y += gap_r;
        start = end;
    }
    free(it);
    return y - cy;
}

/* ------------------------------------------------------------------- grid */
static i32 layout_grid(Node* e, i32 cx, i32 cy, i32 cw, i32* extent){
    Style* s = e->cs;
    FItem* it; int n = collect_items(e, &it, cw);
    i32 gap_c = res(s->gap_col, cw), gap_r = res(s->gap_row, cw);
    /* trilhas: px, %, fr (auto conta como 1fr) */
    i32 fixed_w[32]; i32 fr[32]; int nt = 0;
    const char* g = s->grid_cols;
    if(g && !str_istarts(g, "none")){
        const char* p = g;
        while(*p && nt < 32){
            while(*p == ' ' || *p == ',') p++;
            if(!*p) break;
            if(*p == '['){ while(*p && *p != ']') p++; if(*p) p++; continue; }       /* nomes de linhas */
            if(str_istarts(p, "repeat(")){
                p += 7;
                int count = 0, autofill = 0;
                i32 minpx = 0;
                if(str_istarts(p, "auto-fill") || str_istarts(p, "auto-fit")) autofill = 1;
                else count = atoi(p);
                while(*p && *p != ',') p++;
                if(*p) p++;
                const char* inner = p; int depth = 1;
                while(*p && depth){ if(*p == '(') depth++; else if(*p == ')') depth--; if(depth) p++; }
                int il = (int)(p - inner);
                if(*p) p++;
                const char* mm = NULL;
                for(int k = 0; k + 7 <= il; k++) if(str_istarts(inner + k, "minmax(")){ mm = inner + k + 7; break; }
                i32 tw = -1; int isfr = 0;
                const char* v = mm ? mm : inner;
                while(*v == ' ') v++;
                if(str_istarts(v, "min(")) v += 4;
                i32 num = (i32)atoi(v);
                const char* u = v; while((*u >= '0' && *u <= '9') || *u == '.') u++;
                if(str_istarts(u, "fr")) isfr = 1;
                else if(str_istarts(u, "rem") || str_istarts(u, "em")) tw = num * 16;
                else if(*u == '%') tw = num * cw / 100;
                else if(str_istarts(u, "px")) tw = num;
                minpx = tw;
                if(mm){ isfr = 1; }
                if(autofill){
                    i32 m = minpx > 0 ? minpx : 200;
                    count = (cw + gap_c) / (m + gap_c);
                    if(count < 1) count = 1;
                    if(cw >= BIG / 2) count = n > 0 ? n : 1;
                }
                if(count > 32) count = 32;
                for(int k = 0; k < count && nt < 32; k++){
                    fixed_w[nt] = isfr ? 0 : (tw < 0 ? 0 : tw); fr[nt] = isfr || tw < 0 ? 1 : 0; nt++;
                }
                continue;
            }
            const char* q = p; int depth = 0;
            while(*q && (depth || (*q != ' '))){ if(*q == '(') depth++; else if(*q == ')') depth--; q++; }
            i32 num = (i32)atoi(p);
            const char* u = p; while((*u >= '0' && *u <= '9') || *u == '.') u++;
            if(str_istarts(u, "fr")){ fixed_w[nt] = 0; fr[nt] = num > 0 ? num : 1; }
            else if(str_istarts(u, "px")){ fixed_w[nt] = num; fr[nt] = 0; }
            else if(str_istarts(u, "rem") || str_istarts(u, "em")){ fixed_w[nt] = num * 16; fr[nt] = 0; }
            else if(*u == '%'){ fixed_w[nt] = num * cw / 100; fr[nt] = 0; }
            else if(str_istarts(p, "minmax(")){
                const char* c2 = strchr(p, ',');
                int frr = c2 && strstr(c2, "fr") && strstr(c2, "fr") < q;
                fixed_w[nt] = frr ? 0 : atoi(p + 7); fr[nt] = frr ? 1 : 0;
            }
            else { fixed_w[nt] = 0; fr[nt] = 1; }              /* auto, min-content... */
            nt++;
            p = q;
        }
    }
    if(nt == 0){ nt = 1; fixed_w[0] = 0; fr[0] = 1; }
    i32 colw[32], tot_fixed = 0, tot_fr = 0;
    for(int k = 0; k < nt; k++){ tot_fixed += fixed_w[k]; tot_fr += fr[k]; }
    i32 avail = cw - tot_fixed - gap_c * (nt - 1);
    for(int k = 0; k < nt; k++){
        colw[k] = fixed_w[k];
        if(fr[k] && tot_fr) colw[k] += avail > 0 ? (i32)((i64)avail * fr[k] / tot_fr) : 0;
    }
    i32 y = cy;
    int i = 0;
    while(i < n){
        /* uma linha da grade */
        int col = 0, start = i;
        i32 xs[64]; i32 ws[64]; int cnt = 0;
        i32 x = cx;
        while(i < n && col < nt && cnt < 64){
            FItem* f = &it[i];
            int span = 1;
            if(!f->text){ i16 gs = f->n->cs->grid_span; if(gs == -1) span = nt; else if(gs > 1) span = gs; }
            if(span > nt) span = nt;
            if(col + span > nt && col > 0) break;
            i32 w = 0;
            for(int k = 0; k < span; k++) w += colw[col + k] + (k ? gap_c : 0);
            xs[cnt] = x; ws[cnt] = w; cnt++;
            x += w + gap_c;
            col += span; i++;
        }
        i32 rh = 0, row_ext = 0;
        for(int k = 0; k < cnt; k++){
            FItem* f = &it[start + k];
            DList* sv = DL; DL = NULL;
            f->h = item_layout(f, s, e, 0, 0, ws[k], ws[k] - f->ml - f->mr, -1);
            DL = sv;
            i32 hh = f->h + f->mt + f->mb;
            if(hh > rh) rh = hh;
            i32 mn, mx;
            if(f->text){ item_measure(f, s, e); mn = f->mn; mx = f->mx; } else measure(f->n, &mn, &mx);
            row_ext += (cw >= BIG / 2 ? mx : mn) + (k ? gap_c : 0);
        }
        /* trilhas fixas contam pelo tamanho delas */
        if(tot_fixed + gap_c * (nt - 1) > row_ext && !tot_fr) row_ext = tot_fixed + gap_c * (nt - 1);
        if(row_ext > *extent) *extent = row_ext;
        for(int k = 0; k < cnt; k++){
            FItem* f = &it[start + k];
            u8 al = f->align != 255 ? f->align : s->align_items;
            i32 fh = al == AI_STRETCH && !f->text && f->n->cs->height.kind != L_VAL ? rh - f->mt - f->mb : -1;
            i32 yy = y;
            if(al == AI_CENTER) yy += (rh - f->h - f->mt - f->mb) / 2;
            item_layout(f, s, e, xs[k], yy, ws[k], ws[k] - f->ml - f->mr, fh);
        }
        y += rh;
        if(i < n) y += gap_r;
    }
    free(it);
    return y - cy;
}

/* ----------------------------------------------------------------- tabela */
#define MAXCOLS 64
typedef struct { Node* tr; Node* cell[MAXCOLS]; int span[MAXCOLS]; int n; } TRow;

static void add_row(TRow** rows, int* nr, int* cr, Node* tr){
    if(*nr == *cr){ *cr = *cr ? *cr * 2 : 16; *rows = (TRow*)realloc(*rows, sizeof(TRow) * (size_t)*cr); }
    TRow* r = &(*rows)[(*nr)++];
    r->tr = tr; r->n = 0;
    if(!tr) return;
    for(Node* c = tr->first; c && r->n < MAXCOLS; c = c->next){
        if(c->type != N_ELEM || !c->cs || c->cs->display == D_NONE) continue;
        int sp = 1; const char* cs = dom_attr(c, "colspan");
        if(cs){ sp = atoi(cs); if(sp < 1) sp = 1; if(sp > MAXCOLS) sp = MAXCOLS; }
        r->cell[r->n] = c; r->span[r->n] = sp; r->n++;
    }
}

static i32 layout_table(Node* t, i32 cx, i32 cy, i32 cw, i32* extent){
    Style* s = t->cs;
    TRow* rows = NULL; int nr = 0, cr = 0;
    i32 y = cy;
    for(Node* c = t->first; c; c = c->next){
        if(c->type != N_ELEM || !c->cs) continue;
        int d = c->cs->display;
        if(d == D_ROW) add_row(&rows, &nr, &cr, c);
        else if(d == D_ROW_GROUP){
            for(Node* r = c->first; r; r = r->next)
                if(r->type == N_ELEM && r->cs && r->cs->display == D_ROW) add_row(&rows, &nr, &cr, r);
        } else if(d == D_CELL){
            /* celulas soltas: uma linha anonima */
            add_row(&rows, &nr, &cr, NULL);
            TRow* r = &rows[nr - 1];
            Node* k = c;
            for(; k && r->n < MAXCOLS; k = k->next){
                if(k->type != N_ELEM || !k->cs) continue;
                if(k->cs->display != D_CELL) break;
                r->cell[r->n] = k; r->span[r->n] = 1; r->n++;
            }
            if(!k) break;
            c = k->prev ? k->prev : c;
        } else if(d == D_CAPTION || is_blocklevel(c->cs)){
            Out o; layout_box(c, cx, y, cw, -1, -1, 0, &o);
            y += o.h;
        }
    }
    int ncol = 0;
    for(int r = 0; r < nr; r++){ int k = 0; for(int i = 0; i < rows[r].n; i++) k += rows[r].span[i]; if(k > ncol) ncol = k; }
    if(ncol > MAXCOLS) ncol = MAXCOLS;
    i32 sp = res(s->gap_col, cw);
    i32 mn[MAXCOLS] = {0}, mx[MAXCOLS] = {0}, pct[MAXCOLS] = {0};
    for(int pass = 0; pass < 2; pass++){          /* 1o celulas simples, depois as com colspan */
        for(int r = 0; r < nr; r++){
            int col = 0;
            for(int i = 0; i < rows[r].n && col < ncol; i++){
                Node* c = rows[r].cell[i]; int span = rows[r].span[i];
                if(col + span > ncol) span = ncol - col;
                if((span == 1) == (pass == 0)){
                    i32 a, b; measure(c, &a, &b);
                    if(c->cs->width.kind == L_VAL && c->cs->width.pct && span == 1 && c->cs->width.pct > pct[col]) pct[col] = c->cs->width.pct;
                    if(span == 1){ if(a > mn[col]) mn[col] = a; if(b > mx[col]) mx[col] = b; }
                    else {
                        i32 cm = 0, cx2 = 0;
                        for(int k = 0; k < span; k++){ cm += mn[col + k]; cx2 += mx[col + k]; }
                        cm += sp * (span - 1); cx2 += sp * (span - 1);
                        if(a > cm) for(int k = 0; k < span; k++) mn[col + k] += (a - cm) / span;
                        if(b > cx2) for(int k = 0; k < span; k++) mx[col + k] += (b - cx2) / span;
                    }
                }
                col += span;
            }
        }
    }
    for(int k = 0; k < ncol; k++) if(mx[k] < mn[k]) mx[k] = mn[k];
    i32 avail = cw - sp * (ncol + 1);
    if(avail < 0) avail = 0;
    i32 w[MAXCOLS];
    i32 smin = 0, smax = 0;
    i32 rest = avail;
    for(int k = 0; k < ncol; k++){
        if(pct[k] && cw < BIG / 2){
            w[k] = (i32)((i64)avail * pct[k] / 10000);
            if(w[k] < mn[k]) w[k] = mn[k];
            rest -= w[k];
        } else { smin += mn[k]; smax += mx[k]; }
    }
    if(rest < 0) rest = 0;
    for(int k = 0; k < ncol; k++){
        if(pct[k] && cw < BIG / 2) continue;
        if(smax <= rest){
            w[k] = mx[k];
            if(smax > 0 && cw < BIG / 2) w[k] += (i32)((i64)(rest - smax) * mx[k] / smax);
        } else if(smin >= rest) w[k] = mn[k];
        else w[k] = mn[k] + (i32)((i64)(rest - smin) * (mx[k] - mn[k]) / (smax - smin > 0 ? smax - smin : 1));
    }
    if(smax == 0 && rest > 0 && cw < BIG / 2){
        int free_cols = 0; for(int k = 0; k < ncol; k++) if(!(pct[k])) free_cols++;
        for(int k = 0; k < ncol; k++) if(!pct[k] && free_cols) w[k] = rest / free_cols;
    }
    i32 total = sp;
    for(int k = 0; k < ncol; k++) total += w[k] + sp;
    if(total > *extent) *extent = total;
    if(nr) y += sp;
    for(int r = 0; r < nr; r++){
        TRow* row = &rows[r];
        i32 rh = 0;
        i32 hs[MAXCOLS];
        int col = 0;
        if(row->tr && row->tr->cs->height.kind == L_VAL && !row->tr->cs->height.pct) rh = row->tr->cs->height.px;
        for(int i = 0; i < row->n && col < ncol; i++){
            int span = row->span[i]; if(col + span > ncol) span = ncol - col;
            i32 cwid = 0; for(int k = 0; k < span; k++) cwid += w[col + k] + (k ? sp : 0);
            Out o; DList* sv = DL; DL = NULL;
            layout_box(row->cell[i], 0, 0, cwid, cwid, -1, 0, &o);
            DL = sv;
            hs[i] = o.h;
            if(o.h > rh) rh = o.h;
            col += span;
        }
        if(row->tr && DL){
            Style* rs = row->tr->cs;
            if((rs->bg >> 24) && rs->visibility){
                DItem* d = emit(DL_RECT);
                if(d){ d->x = cx; d->y = y; d->w = total - 2 * sp; d->h = rh; d->color = rs->bg; d->x += sp; }
            }
            node_box(row->tr, cx + sp, y, total - 2 * sp, rh);
        }
        i32 x = cx + sp;
        col = 0;
        for(int i = 0; i < row->n && col < ncol; i++){
            int span = row->span[i]; if(col + span > ncol) span = ncol - col;
            i32 cwid = 0; for(int k = 0; k < span; k++) cwid += w[col + k] + (k ? sp : 0);
            Node* c = row->cell[i];
            u8 va = c->cs->valign;
            if(va == VA_BASELINE && row->tr && row->tr->cs->valign != VA_BASELINE) va = row->tr->cs->valign;
            i32 shift = va == VA_TOP ? 0 : va == VA_BOTTOM ? rh - hs[i] : (va == VA_MIDDLE ? (rh - hs[i]) / 2 : 0);
            Out o;
            layout_box(c, x, y, cwid, cwid, rh, shift, &o);
            x += cwid + sp;
            col += span;
        }
        y += rh + sp;
    }
    free(rows);
    return y - cy;
}

static int table_has_rows(Node* t){
    for(Node* c = t->first; c; c = c->next)
        if(c->type == N_ELEM && c->cs && (c->cs->display == D_ROW || c->cs->display == D_ROW_GROUP || c->cs->display == D_CELL)) return 1;
    return 0;
}

/* ------------------------------------------------------------------ caixa */
static void list_marker(Node* e, i32 cx, i32 cy){
    Style* s = e->cs;
    if(!DL || s->list_style == LS_NONE || !s->visibility) return;
    FontInst* f = sfont(s);
    char buf[24];
    if(s->list_style == LS_DECIMAL || s->list_style == LS_ALPHA || s->list_style == LS_ROMAN){
        int idx = 1;
        Node* par = e->parent;
        const char* st = par ? dom_attr(par, "start") : NULL;
        if(st) idx = atoi(st);
        for(Node* p = e->prev; p; p = p->prev) if(p->type == N_ELEM && p->cs && p->cs->display == D_LIST_ITEM) idx++;
        const char* v = dom_attr(e, "value"); if(v) idx = atoi(v);
        if(s->list_style == LS_ALPHA) snprintf(buf, sizeof buf, "%c.", 'a' + (idx - 1) % 26);
        else if(s->list_style == LS_ROMAN){
            static const char* R[] = {"i","ii","iii","iv","v","vi","vii","viii","ix","x","xi","xii"};
            snprintf(buf, sizeof buf, "%s.", idx >= 1 && idx <= 12 ? R[idx - 1] : "?");
        } else snprintf(buf, sizeof buf, "%d.", idx);
    } else snprintf(buf, sizeof buf, "%s", s->list_style == LS_CIRCLE ? "\xE2\x97\xA6" : s->list_style == LS_SQUARE ? "\xE2\x96\xAA" : "\xE2\x80\xA2");
    char* txt = arena_strdup(&frame_arena, buf);
    i32 lh = line_h(s, f), a = font_ascent(f), d = font_descent(f);
    i32 top = cy + (lh - (a + d)) / 2;
    i32 w = font_width(f, txt, (int)strlen(txt));
    emit_text(cx - w - font_px(f) / 2, top, f, txt, (int)strlen(txt), s->color, e);
}

static void layout_box(Node* e, i32 x, i32 y, i32 cbw, i32 forced_w, i32 forced_h, i32 cshift, Out* o){
    Style* s = e->cs;
    i32 mt = res(s->margin[0], cbw), mr = res(s->margin[1], cbw), mb = res(s->margin[2], cbw), ml = res(s->margin[3], cbw);
    i32 pt = res(s->padding[0], cbw), pr = res(s->padding[1], cbw), pb = res(s->padding[2], cbw), pl = res(s->padding[3], cbw);
    if(pt < 0) pt = 0; if(pr < 0) pr = 0; if(pb < 0) pb = 0; if(pl < 0) pl = 0;
    i32 bt = s->border_w[0], br = s->border_w[1], bb = s->border_w[2], bl = s->border_w[3];
    int ml_auto = s->margin[3].kind == L_AUTO, mr_auto = s->margin[1].kind == L_AUTO;
    i32 hpad = pl + pr + bl + br, vpad = pt + pb + bt + bb;
    int replaced = is_replaced(e);
    int px_width = 0;
    i32 bw;
    i32 rw = 0, rh = 0;
    if(replaced) replaced_size(e, cbw, &rw, &rh);
    int is_table = s->display == D_TABLE || s->display == D_INLINE_TABLE;
    if(forced_w >= 0) bw = forced_w;
    else if(replaced) bw = rw + hpad;
    else if(s->width.kind == L_VAL && !(s->width.pct && cbw >= BIG / 2)){
        bw = len_res(s->width, cbw) + (s->box_border ? 0 : hpad);
        px_width = !s->width.pct;
    }
    else if(is_table && !(e->mgen == mgen && e->mmin == -2)) bw = shrink_fit(e, cbw);
    else bw = cbw - ml - mr;
    if(forced_w < 0 && !replaced){
        if(s->max_w.kind == L_VAL && !(s->max_w.pct && cbw >= BIG / 2)){ i32 m = len_res(s->max_w, cbw) + (s->box_border ? 0 : hpad); if(bw > m){ bw = m; } }
        if(s->min_w.kind == L_VAL && !(s->min_w.pct && cbw >= BIG / 2)){ i32 m = len_res(s->min_w, cbw) + (s->box_border ? 0 : hpad); if(bw < m) bw = m; }
    }
    if(bw < hpad) bw = hpad;
    i32 oml = ml_auto ? 0 : ml, omr = mr_auto ? 0 : mr;
    if(forced_w < 0 && cbw < BIG / 2){
        i32 rest = cbw - bw - ml - mr;
        if(rest > 0){
            if(ml_auto && mr_auto){ ml += rest / 2; mr += rest - rest / 2; }
            else if(ml_auto) ml += rest;
            else if(mr_auto) mr += rest;
        }
    }
    i32 bx = x + ml, by = y + mt;
    int start = dl_mark();
    int hidden = s->opacity == 0;
    if(hidden) hide_depth++;
    DItem* d;
    int hit_i = -1, bg_i = -1, bd_i = -1, clip_i = -1;
    if((d = emit(DL_HIT))){ hit_i = DL->n - 1; d->node = e; }
    int canvas = DL && DL->bg_node == e;
    if((s->bg >> 24) && s->visibility && !canvas && !s->masked && (d = emit(DL_RECT))){ bg_i = DL->n - 1; d->color = s->bg; d->radius = s->radius; d->node = e; }
    if(s->visibility && doc){
        /* imagens de fundo e de mascara: baixadas como as <img> (a URL ja e absoluta) */
        if(s->bg_url && !s->bg_img) s->bg_img = image_get(s->bg_url);
        if(s->masked && s->mask_url && !s->mask_img) s->mask_img = image_get(s->mask_url);
    }
    if((bt || br || bb || bl) && s->visibility && (d = emit(DL_BORDER))){
        bd_i = DL->n - 1; d->radius = s->radius;
        d->bw[0] = (u8)bt; d->bw[1] = (u8)br; d->bw[2] = (u8)bb; d->bw[3] = (u8)bl;
        for(int k = 0; k < 4; k++) d->bc[k] = s->border_c[k];
    }
    if(s->overflow_hidden && (d = emit(DL_CLIP))) clip_i = DL->n - 1;
    i32 cx = bx + bl + pl, cy = by + bt + pt, cw = bw - hpad;
    /* bloco que contem os absolutos */
    typeof(cb) saved_cb = cb;
    int pmark = npend;
    if(s->position != POS_STATIC){ cb.x = bx + bl; cb.y = by + bt; cb.w = bw - bl - br; cb.h = 0; }
    int content = dl_mark();
    i32 extent = 0, ch;
    i32 saved_base = g_base; g_base = -0x7FFFFFFF;
    /* contexto de floats proprio */
    FCtx own_fc; FCtx* saved_fc = FC; int has_fc = 0;
    if(!intrinsic && !replaced && (!FC || makes_bfc(e))){ memset(&own_fc, 0, sizeof own_fc); FC = &own_fc; has_fc = 1; }
    if(replaced){ ch = rh; emit_replaced(e, cx, cy, cw, ch); extent = rw; }
    else switch(s->display){
        case D_FLEX: case D_INLINE_FLEX: ch = layout_flex(e, cx, cy, cw, &extent); break;
        case D_GRID: case D_INLINE_GRID: ch = layout_grid(e, cx, cy, cw, &extent); break;
        case D_TABLE: case D_INLINE_TABLE:
            ch = table_has_rows(e) ? layout_table(e, cx, cy, cw, &extent) : layout_flow(e, cx, cy, cw, &extent);
            break;
        default: ch = layout_flow(e, cx, cy, cw, &extent); break;
    }
    if(has_fc){
        /* o contexto contem os seus floats */
        if(own_fc.n){ i32 fb = fc_bottom() - cy; if(fb > ch) ch = fb; }
        free(own_fc.f);
        FC = saved_fc;
    }
    if(s->display == D_LIST_ITEM) list_marker(e, cx, cy);
    if(!replaced && s->height.kind == L_VAL && !s->height.pct){
        ch = s->height.px - (s->box_border ? vpad : 0);
    }
    if(s->min_h.kind == L_VAL && !s->min_h.pct){ i32 m = s->min_h.px - (s->box_border ? vpad : 0); if(ch < m) ch = m; }
    else if(s->min_h.kind == L_VAL && s->min_h.pct >= 10000 && s->display != D_INLINE && e->parent && e->parent->type == N_ELEM && dom_is(e->parent, "html")) { if(ch < VH - vpad) ch = VH - vpad; }
    if(s->max_h.kind == L_VAL && !s->max_h.pct){ i32 m = s->max_h.px - (s->box_border ? vpad : 0); if(ch > m) ch = m; }
    if(ch < 0) ch = 0;
    i32 bh = ch + vpad;
    if(forced_h > bh) bh = forced_h;
    if(cshift) dl_shift(content, 0, cshift);
    if(DL){
        DItem* it = DL->it;
        if(hit_i >= 0){ it[hit_i].x = bx; it[hit_i].y = by; it[hit_i].w = bw; it[hit_i].h = bh; }
        if(bg_i >= 0){ it[bg_i].x = bx; it[bg_i].y = by; it[bg_i].w = bw; it[bg_i].h = bh; }
        if(bd_i >= 0){ it[bd_i].x = bx; it[bd_i].y = by; it[bd_i].w = bw; it[bd_i].h = bh; }
        if(clip_i >= 0){ it[clip_i].x = bx + bl; it[clip_i].y = by + bt; it[clip_i].w = bw - bl - br; it[clip_i].h = bh - bt - bb; }
        /* gradiente de fundo: logo depois da cor */
        if(s->bg_grad && s->visibility && !hide_depth && bw > 0 && bh > 0 && !s->masked){
            Grad* g = grad_build(&frame_arena, s->bg_grad, bw, bh);
            if(g && emit(DL_GRAD)){
                DItem tmp = DL->it[DL->n - 1];
                tmp.x = bx; tmp.y = by; tmp.w = bw; tmp.h = bh; tmp.grad = g; tmp.radius = s->radius; tmp.alpha = (u8)s->opacity; tmp.node = e;
                int at = bg_i >= 0 ? bg_i + 1 : (hit_i >= 0 ? hit_i + 1 : start);
                memmove(&DL->it[at + 1], &DL->it[at], sizeof(DItem) * (size_t)(DL->n - 1 - at));
                DL->it[at] = tmp;
                if(clip_i >= at) clip_i++;
                if(bd_i >= at) bd_i++;
                bg_i = at;
            }
        }
        /* fundo (imagem) e icone por mascara: logo depois da cor, antes do conteudo */
        for(int layer = 0; layer < 2; layer++){
            int mk = layer == 1;
            Image* im = mk ? (s->masked && (s->bg >> 24) ? s->mask_img : NULL) : (s->masked && s->mask_url ? NULL : s->bg_img);
            if(!im || im->state != 1 || !s->visibility) continue;
            i32 px0 = bx + bl, py0 = by + bt, pw = bw - bl - br, ph = bh - bt - bb;      /* caixa do padding */
            if(pw <= 0 || ph <= 0) continue;
            DItem tmp; memset(&tmp, 0, sizeof tmp);
            tmp.kind = mk ? DL_MASK : DL_BGIMAGE; tmp.x = px0; tmp.y = py0; tmp.w = pw; tmp.h = ph; tmp.img = im; tmp.alpha = (u8)s->opacity;
            tmp.fixed = fixed_depth > 0; tmp.color = s->bg; tmp.node = e;
            layer_tile(im, mk ? s->mask_szk : s->bg_szk, mk ? s->mask_sw : s->bg_sw, mk ? s->mask_sh : s->bg_sh,
                       mk ? s->mask_px : s->bg_px, mk ? s->mask_py : s->bg_py, pw, ph, &tmp.ix, &tmp.iy, &tmp.iw, &tmp.ih);
            tmp.rep = mk ? s->mask_rep : s->bg_rep;
            int at = bg_i >= 0 ? bg_i + 1 : (hit_i >= 0 ? hit_i + 1 : start);
            if(!hide_depth && emit(DL_BGIMAGE)){
                memmove(&DL->it[at + 1], &DL->it[at], sizeof(DItem) * (size_t)(DL->n - 1 - at));
                DL->it[at] = tmp;
                if(clip_i >= at) clip_i++;
                if(bd_i >= at) bd_i++;
                bg_i = at;
            }
        }
        if(clip_i >= 0) emit(DL_UNCLIP);
        e->lgen = lgen; e->bx = bx; e->by = by; e->bw = bw; e->bh = bh;
    }
    if(s->position != POS_STATIC){
        cb.h = bh - bt - bb;
        process_pending(pmark, content);
        cb = saved_cb;
    }
    if(DL && (s->tx.px || s->tx.pct || s->ty.px || s->ty.pct)){
        dl_shift(start, len_res(s->tx, bw), len_res(s->ty, bh));
    }
    if(s->position == POS_RELATIVE || s->position == POS_STICKY){
        i32 dx = s->inset[3].kind == L_VAL ? res(s->inset[3], cbw) : s->inset[1].kind == L_VAL ? -res(s->inset[1], cbw) : 0;
        i32 dy = s->position == POS_STICKY ? 0 : s->inset[0].kind == L_VAL ? res(s->inset[0], cbw) : s->inset[2].kind == L_VAL ? -res(s->inset[2], cbw) : 0;
        if(DL){ dl_shift(start, dx, dy); }
    }
    if(hidden) hide_depth--;
    o->base = g_base == -0x7FFFFFFF ? -1 : g_base - y;
    if(g_base == -0x7FFFFFFF || s->floating || s->position == POS_ABSOLUTE || s->position == POS_FIXED) g_base = saved_base;
    o->w = bw;
    o->h = mt + bh + mb;
    o->extent = (px_width ? bw : extent + hpad) + oml + omr;
    if(replaced) o->extent = bw + oml + omr;
}

/* ---------------------------------------------------------------- entrada */
void layout_doc(Doc* d, DList* dl, i32 vw, i32 vh){
    DL = dl; dl->n = 0;
    VW = vw; VH = vh;
    mgen++; lgen++;
    npend = 0; fixed_depth = 0; hide_depth = 0; FC = NULL; intrinsic = 0;
    Node* html = NULL;
    for(Node* c = d->root->first; c; c = c->next) if(c->type == N_ELEM){ html = c; break; }
    dl->bg = 0xFFFFFFFF; dl->bg_node = NULL;
    if(!html || !html->cs){ dl->width = vw; dl->height = 0; return; }
    /* fundo do canvas: do <html>, ou propagado do <body> */
    Node* body = dom_body(d);
    if(html->cs->bg >> 24){ dl->bg = html->cs->bg; dl->bg_node = html; }
    else if(body && body->cs && (body->cs->bg >> 24)){ dl->bg = body->cs->bg; dl->bg_node = body; }
    if((dl->bg >> 24) < 255) dl->bg = mix(0xFFFFFF, dl->bg & 0xFFFFFF, dl->bg >> 24) | 0xFF000000;
    cb.x = 0; cb.y = 0; cb.w = vw; cb.h = vh;
    Out o;
    layout_box(html, 0, 0, vw, -1, vh, 0, &o);
    cb.x = 0; cb.y = 0; cb.w = vw; cb.h = o.h > vh ? o.h : vh;
    process_pending(0, 0);
    i32 maxy = o.h;
    for(int i = 0; i < dl->n; i++){
        DItem* it = &dl->it[i];
        if(it->fixed || it->kind == DL_HIT) continue;
        if(it->y + it->h > maxy && it->kind != DL_CLIP) maxy = it->y + it->h;
    }
    /* recortes nao limitam a altura do documento */
    dl->width = vw; dl->height = maxy;
    DL = NULL;
}

Node* layout_hit(DList* dl, i32 x, i32 y, i32 scroll){
    Node* best = NULL;
    /* os fixos sao pintados por cima: testados primeiro */
    for(int pass = 1; pass >= 0 && !best; pass--)
    for(int i = dl->n - 1; i >= 0; i--){
        DItem* d = &dl->it[i];
        if(d->fixed != pass) continue;
        if(!d->node || (d->kind != DL_HIT && d->kind != DL_TEXT && d->kind != DL_IMAGE)) continue;
        i32 yy = d->fixed ? y : y + scroll;
        if(x >= d->x && x < d->x + d->w && yy >= d->y && yy < d->y + d->h){
            Node* n = d->node;
            while(n && (n->flags & NF_PSEUDO)) n = n->parent;       /* ::before/::after: o elemento */
            if(!n) continue;
            if(n->cs && n->cs->visibility == 0 && d->kind == DL_HIT) continue;
            best = n;
            break;
        }
    }
    return best;
}
