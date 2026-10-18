/* css.c — CSS: folhas de estilo, seletores, cascata e estilo computado.
 *
 * - Folhas sao analisadas uma vez (regras + declaracoes com o id da
 *   propriedade ja resolvido). @media e avaliado contra a janela; @supports,
 *   @layer e @container entram como blocos normais.
 * - Seletores: tipo, *, #id, .classe, [atributo op valor], combinadores
 *   (espaco > + ~) e pseudo-classes estruturais (:nth-child, :not, :is...).
 *   Pseudo-elementos (::before) descartam a regra.
 * - Cascata: origem (navegador < dicas de atributos < autor < style=""),
 *   !important, especificidade e ordem. Propriedades --custom com var().
 * Os estilos computados vivem em frame_arena e sao refeitos por inteiro
 * quando o DOM muda. */
#include "vela.h"
#include <math.h>

Arena frame_arena;
i32 css_vw = 1024, css_vh = 768;

/* ======================================================== propriedades */
enum {
    P_UNKNOWN, P_CUSTOM,
    P_ALIGN_CONTENT, P_ALIGN_ITEMS, P_ALIGN_SELF, P_ALL, P_ASPECT_RATIO, P_BACKGROUND, P_BACKGROUND_COLOR,
    P_BACKGROUND_IMAGE, P_BACKGROUND_POSITION, P_BACKGROUND_REPEAT, P_BACKGROUND_SIZE, P_BORDER,
    P_BORDER_BOTTOM, P_BORDER_BOTTOM_COLOR, P_BORDER_BOTTOM_STYLE, P_BORDER_BOTTOM_WIDTH, P_BORDER_COLOR,
    P_BORDER_LEFT, P_BORDER_LEFT_COLOR, P_BORDER_LEFT_STYLE, P_BORDER_LEFT_WIDTH, P_BORDER_RADIUS,
    P_BORDER_RIGHT, P_BORDER_RIGHT_COLOR, P_BORDER_RIGHT_STYLE, P_BORDER_RIGHT_WIDTH, P_BORDER_SPACING,
    P_BORDER_STYLE, P_BORDER_TOP, P_BORDER_TOP_COLOR, P_BORDER_TOP_STYLE, P_BORDER_TOP_WIDTH, P_BORDER_WIDTH,
    P_BOTTOM, P_BOX_SHADOW, P_BOX_SIZING, P_CLEAR, P_CLIP, P_CLIP_PATH, P_COLOR, P_COLUMN_GAP, P_CONTENT,
    P_CURSOR, P_DISPLAY, P_FLEX, P_FLEX_BASIS, P_FLEX_DIRECTION, P_FLEX_FLOW, P_FLEX_GROW, P_FLEX_SHRINK,
    P_FLEX_WRAP, P_FLOAT, P_FONT, P_FONT_FAMILY, P_FONT_SIZE, P_FONT_STYLE, P_FONT_WEIGHT, P_GAP,
    P_GRID_AREA, P_GRID_AUTO_COLUMNS, P_GRID_AUTO_FLOW, P_GRID_AUTO_ROWS, P_GRID_COLUMN, P_GRID_COLUMN_END,
    P_GRID_COLUMN_START, P_GRID_ROW, P_GRID_ROW_END, P_GRID_ROW_START, P_GRID_TEMPLATE,
    P_GRID_TEMPLATE_AREAS, P_GRID_TEMPLATE_COLUMNS, P_GRID_TEMPLATE_ROWS, P_HEIGHT, P_INSET,
    P_JUSTIFY_CONTENT, P_JUSTIFY_ITEMS, P_JUSTIFY_SELF, P_LEFT, P_LETTER_SPACING, P_LINE_HEIGHT,
    P_LIST_STYLE, P_LIST_STYLE_IMAGE, P_LIST_STYLE_TYPE, P_MARGIN, P_MARGIN_BLOCK, P_MARGIN_BLOCK_END,
    P_MARGIN_BLOCK_START, P_MARGIN_BOTTOM, P_MARGIN_INLINE, P_MARGIN_INLINE_END, P_MARGIN_INLINE_START,
    P_MARGIN_LEFT, P_MARGIN_RIGHT, P_MARGIN_TOP, P_MASK, P_MASK_IMAGE, P_MASK_POSITION, P_MASK_REPEAT,
    P_MASK_SIZE, P_MAX_HEIGHT, P_MAX_WIDTH, P_MIN_HEIGHT, P_MIN_WIDTH, P_OBJECT_FIT, P_OPACITY, P_ORDER,
    P_OVERFLOW, P_OVERFLOW_CLIP_MARGIN, P_OVERFLOW_WRAP, P_OVERFLOW_X, P_OVERFLOW_Y, P_PADDING,
    P_PADDING_BLOCK, P_PADDING_BLOCK_END, P_PADDING_BLOCK_START, P_PADDING_BOTTOM, P_PADDING_INLINE,
    P_PADDING_INLINE_END, P_PADDING_INLINE_START, P_PADDING_LEFT, P_PADDING_RIGHT, P_PADDING_TOP,
    P_PLACE_CONTENT, P_PLACE_ITEMS, P_PLACE_SELF, P_POSITION, P_RIGHT, P_ROW_GAP, P_TEXT_ALIGN,
    P_TEXT_DECORATION, P_TEXT_DECORATION_COLOR, P_TEXT_DECORATION_LINE, P_TEXT_INDENT, P_TEXT_OVERFLOW,
    P_TEXT_SHADOW, P_TEXT_TRANSFORM, P_TOP, P_TRANSFORM, P_TRANSLATE, P_VERTICAL_ALIGN, P_VISIBILITY,
    P_WHITE_SPACE, P_WIDTH, P_WORD_BREAK, P_WORD_WRAP, P_Z_INDEX,
    P_COUNT
};
/* em ordem alfabetica: busca binaria */
static const char* const PROP_NAMES[] = {
    "align-content", "align-items", "align-self", "all", "aspect-ratio", "background", "background-color",
    "background-image", "background-position", "background-repeat", "background-size", "border",
    "border-bottom", "border-bottom-color", "border-bottom-style", "border-bottom-width", "border-color",
    "border-left", "border-left-color", "border-left-style", "border-left-width", "border-radius",
    "border-right", "border-right-color", "border-right-style", "border-right-width", "border-spacing",
    "border-style", "border-top", "border-top-color", "border-top-style", "border-top-width", "border-width",
    "bottom", "box-shadow", "box-sizing", "clear", "clip", "clip-path", "color", "column-gap", "content",
    "cursor", "display", "flex", "flex-basis", "flex-direction", "flex-flow", "flex-grow", "flex-shrink",
    "flex-wrap", "float", "font", "font-family", "font-size", "font-style", "font-weight", "gap",
    "grid-area", "grid-auto-columns", "grid-auto-flow", "grid-auto-rows", "grid-column", "grid-column-end",
    "grid-column-start", "grid-row", "grid-row-end", "grid-row-start", "grid-template",
    "grid-template-areas", "grid-template-columns", "grid-template-rows", "height", "inset",
    "justify-content", "justify-items", "justify-self", "left", "letter-spacing", "line-height",
    "list-style", "list-style-image", "list-style-type", "margin", "margin-block", "margin-block-end",
    "margin-block-start", "margin-bottom", "margin-inline", "margin-inline-end", "margin-inline-start",
    "margin-left", "margin-right", "margin-top", "mask", "mask-image", "mask-position", "mask-repeat",
    "mask-size", "max-height", "max-width", "min-height", "min-width", "object-fit", "opacity", "order",
    "overflow", "overflow-clip-margin", "overflow-wrap", "overflow-x", "overflow-y", "padding",
    "padding-block", "padding-block-end", "padding-block-start", "padding-bottom", "padding-inline",
    "padding-inline-end", "padding-inline-start", "padding-left", "padding-right", "padding-top",
    "place-content", "place-items", "place-self", "position", "right", "row-gap", "text-align",
    "text-decoration", "text-decoration-color", "text-decoration-line", "text-indent", "text-overflow",
    "text-shadow", "text-transform", "top", "transform", "translate", "vertical-align", "visibility",
    "white-space", "width", "word-break", "word-wrap", "z-index",
};

static int prop_id(const char* s, int n){
    if(n > 2 && s[0] == '-' && s[1] == '-') return P_CUSTOM;
    if(n > 8 && str_istarts(s, "-webkit-mask")){ s += 8; n -= 8; }
    char k[40];
    if(n >= 40) return P_UNKNOWN;
    for(int i = 0; i < n; i++) k[i] = (char)lower((u8)s[i]);
    k[n] = 0;
    int lo = 0, hi = (int)(sizeof PROP_NAMES / sizeof PROP_NAMES[0]) - 1;
    while(lo <= hi){
        int mid = (lo + hi) / 2;
        int c = strcmp(k, PROP_NAMES[mid]);
        if(c == 0) return mid + P_ALIGN_CONTENT;
        if(c < 0) hi = mid - 1; else lo = mid + 1;
    }
    return P_UNKNOWN;
}

/* ============================================================ seletores */
enum { AT_EXISTS, AT_EQ, AT_INCL, AT_DASH, AT_PREFIX, AT_SUFFIX, AT_SUBSTR };
enum { PS_FIRST = 1, PS_LAST = 2, PS_ONLY = 4, PS_ROOT = 8, PS_EMPTY = 16, PS_LINK = 32,
       PS_FIRST_T = 64, PS_LAST_T = 128, PS_ONLY_T = 256, PS_CHECKED = 512, PS_DISABLED = 1024,
       PS_ENABLED = 2048, PS_NEVER = 4096, PS_HOVER = 8192 };
typedef struct { const char* name; const char* val; u8 op, icase; } ASel;
typedef struct Sel Sel;
typedef struct {
    const char* tag; const char* id;
    const char* cls[8]; int ncls;
    ASel* at; int nat;
    u16 ps;
    u8 nth_kind; i16 nth_a, nth_b;
    Sel* nots[4]; int nnot;
    Sel* is_;
    u8 pe;                           /* pseudo-elemento: 1 ::before, 2 ::after */
    char comb;                       /* combinador com o composto a esquerda */
} Comp;
struct Sel { Comp* c; int n; u32 spec; Sel* next; u8 pe; };

typedef struct { const char* s; int n, i; Arena* a; } Rd;

static int ident_char(int c){ return (c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z') || (c >= '0' && c <= '9') || c == '-' || c == '_' || c >= 0x80 || c == '\\'; }

/* le um identificador (com escapes \: e \31 ) */
static const char* read_ident(Rd* r){
    char buf[256]; int k = 0;
    while(r->i < r->n && ident_char((u8)r->s[r->i])){
        char c = r->s[r->i];
        if(c == '\\' && r->i + 1 < r->n){
            r->i++;
            int hex = 0, hn = 0;
            while(hn < 6 && r->i < r->n){
                int h = lower((u8)r->s[r->i]);
                if(h >= '0' && h <= '9') hex = hex * 16 + h - '0';
                else if(h >= 'a' && h <= 'f') hex = hex * 16 + h - 'a' + 10;
                else break;
                hn++; r->i++;
            }
            if(hn){
                if(r->i < r->n && r->s[r->i] == ' ') r->i++;
                if(k < 250) k += utf8_encode((u32)hex, buf + k);
            } else if(k < 255) buf[k++] = r->s[r->i++];
            continue;
        }
        if(k < 255) buf[k++] = c;
        r->i++;
    }
    if(!k) return NULL;
    return arena_strndup(r->a, buf, (size_t)k);
}

static Sel* parse_sel_list(Rd* r, char end);

static void parse_nth(const char* s, int n, i16* a, i16* b){
    char t[64]; int k = 0;
    for(int i = 0; i < n && k < 63; i++) if(!is_ws((u8)s[i])) t[k++] = (char)lower((u8)s[i]);
    t[k] = 0;
    if(!strcmp(t, "odd")){ *a = 2; *b = 1; return; }
    if(!strcmp(t, "even")){ *a = 2; *b = 0; return; }
    char* np = strchr(t, 'n');
    if(!np){ *a = 0; *b = (i16)atoi(t); return; }
    *np = 0;
    if(!t[0] || !strcmp(t, "+")) *a = 1;
    else if(!strcmp(t, "-")) *a = -1;
    else *a = (i16)atoi(t);
    *b = (i16)atoi(np + 1);
}

/* um composto; retorna 0 se invalido (ex.: pseudo-elemento) */
static int parse_comp(Rd* r, Comp* c, u32* spec){
    memset(c, 0, sizeof *c);
    int any = 0;
    if(r->i < r->n && r->s[r->i] == '*'){ r->i++; any = 1; }
    else if(r->i < r->n && ident_char((u8)r->s[r->i])
            && !(r->s[r->i] >= '0' && r->s[r->i] <= '9')
            && (r->s[r->i] != '-' || (r->i + 1 < r->n && ident_char((u8)r->s[r->i + 1])))){
        const char* t = read_ident(r);
        if(t){
            char* lt = (char*)t; for(int k = 0; lt[k]; k++) lt[k] = (char)lower((u8)lt[k]);
            c->tag = t; *spec += 1; any = 1;
        }
    }
    if(r->i < r->n && r->s[r->i] == '|'){       /* namespace: svg|rect */
        r->i++;
        if(r->i < r->n && r->s[r->i] == '*'){ r->i++; c->tag = NULL; }
        else { const char* t = read_ident(r); c->tag = t; }
    }
    for(;;){
        if(r->i >= r->n) break;
        char ch = r->s[r->i];
        if(ch == '#'){
            r->i++; c->id = read_ident(r); if(!c->id) return 0;
            *spec += 1 << 16; any = 1;
        } else if(ch == '.'){
            r->i++; const char* cl = read_ident(r); if(!cl) return 0;
            if(c->ncls < 8) c->cls[c->ncls++] = cl;
            *spec += 1 << 8; any = 1;
        } else if(ch == '['){
            r->i++;
            while(r->i < r->n && is_ws((u8)r->s[r->i])) r->i++;
            const char* name = read_ident(r);
            if(!name) return 0;
            char* ln = (char*)name; for(int k = 0; ln[k]; k++) ln[k] = (char)lower((u8)ln[k]);
            while(r->i < r->n && is_ws((u8)r->s[r->i])) r->i++;
            ASel as = { name, NULL, AT_EXISTS, 0 };
            if(r->i < r->n && r->s[r->i] != ']'){
                char o = r->s[r->i];
                if(o == '='){ as.op = AT_EQ; r->i++; }
                else if(r->i + 1 < r->n && r->s[r->i + 1] == '='){
                    as.op = o == '~' ? AT_INCL : o == '|' ? AT_DASH : o == '^' ? AT_PREFIX : o == '$' ? AT_SUFFIX : o == '*' ? AT_SUBSTR : 255;
                    if(as.op == 255) return 0;
                    r->i += 2;
                } else return 0;
                while(r->i < r->n && is_ws((u8)r->s[r->i])) r->i++;
                if(r->i < r->n && (r->s[r->i] == '"' || r->s[r->i] == '\'')){
                    char q = r->s[r->i++]; int v0 = r->i;
                    while(r->i < r->n && r->s[r->i] != q) r->i++;
                    as.val = arena_strndup(r->a, r->s + v0, (size_t)(r->i - v0));
                    r->i++;
                } else {
                    as.val = read_ident(r);
                    if(!as.val) as.val = "";
                }
                while(r->i < r->n && is_ws((u8)r->s[r->i])) r->i++;
                if(r->i < r->n && (r->s[r->i] == 'i' || r->s[r->i] == 'I')){ as.icase = 1; r->i++; }
                else if(r->i < r->n && (r->s[r->i] == 's' || r->s[r->i] == 'S')) r->i++;
                while(r->i < r->n && is_ws((u8)r->s[r->i])) r->i++;
            }
            if(r->i >= r->n || r->s[r->i] != ']') return 0;
            r->i++;
            ASel* na = (ASel*)arena_alloc(r->a, sizeof(ASel) * (size_t)(c->nat + 1));
            if(c->nat) memcpy(na, c->at, sizeof(ASel) * (size_t)c->nat);
            na[c->nat++] = as; c->at = na;
            *spec += 1 << 8; any = 1;
        } else if(ch == ':'){
            r->i++;
            int dbl = r->i < r->n && r->s[r->i] == ':';
            if(dbl) r->i++;
            const char* ps = read_ident(r);
            if(!ps) return 0;
            char p[48]; snprintf(p, sizeof p, "%s", ps);
            for(int k = 0; p[k]; k++) p[k] = (char)lower((u8)p[k]);
            any = 1;
            if(!strcmp(p, "before") || !strcmp(p, "after")){
                if(c->pe) return 0;
                c->pe = p[0] == 'b' ? 1 : 2; *spec += 1;
                continue;
            }
            if(dbl || c->pe) return 0;       /* outros pseudo-elementos (::marker, ::placeholder...) */
            if(r->i < r->n && r->s[r->i] == '('){
                r->i++;
                int a0 = r->i, depth = 1;
                if(!strcmp(p, "not") || !strcmp(p, "is") || !strcmp(p, "where") || !strcmp(p, "matches")
                    || !strcmp(p, "-webkit-any") || !strcmp(p, "-moz-any")){
                    Sel* inner = parse_sel_list(r, ')');
                    if(r->i < r->n && r->s[r->i] == ')') r->i++;
                    if(!inner) return 0;
                    u32 mx = 0; for(Sel* s = inner; s; s = s->next) if(s->spec > mx) mx = s->spec;
                    if(!strcmp(p, "not")){ if(c->nnot < 4) c->nots[c->nnot++] = inner; *spec += mx; }
                    else { c->is_ = inner; if(strcmp(p, "where")) *spec += mx; }
                    continue;
                }
                while(r->i < r->n && depth){ if(r->s[r->i] == '(') depth++; else if(r->s[r->i] == ')') depth--; r->i++; }
                int a1 = r->i - 1;
                if(!strcmp(p, "nth-child") || !strcmp(p, "nth-last-child") || !strcmp(p, "nth-of-type") || !strcmp(p, "nth-last-of-type")){
                    c->nth_kind = !strcmp(p, "nth-child") ? 1 : !strcmp(p, "nth-last-child") ? 2 : !strcmp(p, "nth-of-type") ? 3 : 4;
                    int e = a0; while(e < a1 && !(r->s[e] == ' ' && r->s[e + 1] == 'o' && r->s[e + 2] == 'f')) e++;
                    parse_nth(r->s + a0, e - a0, &c->nth_a, &c->nth_b);
                } else if(!strcmp(p, "lang") || !strcmp(p, "dir") || !strcmp(p, "has") || !strcmp(p, "host")
                    || !strcmp(p, "host-context") || !strcmp(p, "state")) c->ps |= PS_NEVER;
                else return 0;
                *spec += 1 << 8;
                continue;
            }
            *spec += 1 << 8;
            if(!strcmp(p, "first-child")) c->ps |= PS_FIRST;
            else if(!strcmp(p, "last-child")) c->ps |= PS_LAST;
            else if(!strcmp(p, "only-child")) c->ps |= PS_ONLY;
            else if(!strcmp(p, "first-of-type")) c->ps |= PS_FIRST_T;
            else if(!strcmp(p, "last-of-type")) c->ps |= PS_LAST_T;
            else if(!strcmp(p, "only-of-type")) c->ps |= PS_ONLY_T;
            else if(!strcmp(p, "root")) c->ps |= PS_ROOT;
            else if(!strcmp(p, "empty")) c->ps |= PS_EMPTY;
            else if(!strcmp(p, "link") || !strcmp(p, "any-link")) c->ps |= PS_LINK;
            else if(!strcmp(p, "checked")) c->ps |= PS_CHECKED;
            else if(!strcmp(p, "disabled")) c->ps |= PS_DISABLED;
            else if(!strcmp(p, "enabled")) c->ps |= PS_ENABLED;
            else if(!strcmp(p, "hover")) c->ps |= PS_HOVER;
            else if(!strcmp(p, "visited") || !strcmp(p, "active") || !strcmp(p, "focus") || !strcmp(p, "focus-within")
                || !strcmp(p, "focus-visible") || !strcmp(p, "target") || !strcmp(p, "placeholder-shown")
                || !strcmp(p, "invalid") || !strcmp(p, "valid") || !strcmp(p, "required") || !strcmp(p, "optional")
                || !strcmp(p, "read-only") || !strcmp(p, "read-write") || !strcmp(p, "indeterminate")
                || !strcmp(p, "default") || !strcmp(p, "defined") || !strcmp(p, "fullscreen") || !strcmp(p, "modal")
                || !strcmp(p, "autofill") || !strcmp(p, "-webkit-autofill") || !strcmp(p, "popover-open")
                || !strcmp(p, "user-invalid") || !strcmp(p, "open") || !strcmp(p, "scope"))
                c->ps |= PS_NEVER;
            else return 0;            /* :before, :after, desconhecidas */
        } else break;
    }
    return any;
}

/* uma lista de seletores separada por virgulas, ate `end` (0 = fim) */
static Sel* parse_sel_list(Rd* r, char end){
    Sel* head = NULL; Sel** tail = &head;
    for(;;){
        while(r->i < r->n && is_ws((u8)r->s[r->i])) r->i++;
        Comp comps[24]; int nc = 0; u32 spec = 0; int ok = 1;
        char comb = 0;
        for(;;){
            while(r->i < r->n && is_ws((u8)r->s[r->i])){ r->i++; if(nc && !comb) comb = ' '; }
            if(r->i >= r->n) break;
            char ch = r->s[r->i];
            if(ch == ',' || (end && ch == end)) break;
            if(ch == '>' || ch == '+' || ch == '~'){
                if(!nc){ ok = 0; }
                comb = ch; r->i++; continue;
            }
            if(nc == 24){ ok = 0; break; }
            if(nc && comps[nc - 1].pe){ ok = 0; break; }      /* pseudo-elemento so no fim */
            if(!parse_comp(r, &comps[nc], &spec)){ ok = 0; break; }
            comps[nc].comb = nc ? (comb ? comb : ' ') : 0;
            nc++; comb = 0;
        }
        if(!ok){
            /* pula o resto deste seletor (respeitando parenteses) */
            int depth = 0;
            while(r->i < r->n){
                char ch = r->s[r->i];
                if(ch == '(' || ch == '[') depth++;
                else if(ch == ')' || ch == ']'){ if(!depth && end == ')') break; depth--; }
                else if(!depth && (ch == ',' || (end && ch == end))) break;
                r->i++;
            }
        } else if(nc){
            Sel* s = (Sel*)arena_alloc(r->a, sizeof(Sel));
            s->c = (Comp*)arena_alloc(r->a, sizeof(Comp) * (size_t)nc);
            memcpy(s->c, comps, sizeof(Comp) * (size_t)nc);
            s->n = nc; s->spec = spec; s->pe = comps[nc - 1].pe; s->next = NULL;
            *tail = s; tail = &s->next;
        }
        if(r->i < r->n && r->s[r->i] == ','){ r->i++; continue; }
        break;
    }
    return head;
}

/* ---- casamento ---- */
static Node* prev_elem(Node* n){ for(n = n->prev; n; n = n->prev) if(n->type == N_ELEM) return n; return NULL; }
static Node* next_elem(Node* n){ for(n = n->next; n; n = n->next) if(n->type == N_ELEM) return n; return NULL; }
static Node* parent_elem(Node* n){ n = n->parent; return n && n->type == N_ELEM ? n : NULL; }

static int has_class(const Node* n, const char* cls){
    const char* v = dom_attr(n, "class");
    if(!v) return 0;
    int cl = (int)strlen(cls);
    for(const char* p = v; *p;){
        while(*p && is_ws((u8)*p)) p++;
        const char* s = p; while(*p && !is_ws((u8)*p)) p++;
        if(p - s == cl && memcmp(s, cls, (size_t)cl) == 0) return 1;
    }
    return 0;
}

static int attr_match(const Node* n, const ASel* a){
    const char* v = dom_attr(n, a->name);
    if(!v) return 0;
    if(a->op == AT_EXISTS) return 1;
    int vl = (int)strlen(v), al = (int)strlen(a->val);
    int (*cmp)(const char*, const char*, size_t) = a->icase ? strncasecmp : strncmp;
    switch(a->op){
    case AT_EQ: return vl == al && cmp(v, a->val, (size_t)al) == 0;
    case AT_PREFIX: return al && vl >= al && cmp(v, a->val, (size_t)al) == 0;
    case AT_SUFFIX: return al && vl >= al && cmp(v + vl - al, a->val, (size_t)al) == 0;
    case AT_SUBSTR: if(!al) return 0; for(int i = 0; i + al <= vl; i++) if(cmp(v + i, a->val, (size_t)al) == 0) return 1; return 0;
    case AT_DASH: return (vl == al || (vl > al && v[al] == '-')) && cmp(v, a->val, (size_t)al) == 0;
    case AT_INCL:
        for(const char* p = v; *p;){
            while(*p && is_ws((u8)*p)) p++;
            const char* s = p; while(*p && !is_ws((u8)*p)) p++;
            if(p - s == al && cmp(s, a->val, (size_t)al) == 0) return 1;
        }
        return 0;
    }
    return 0;
}

static int nth_ok(int a, int b, int idx){         /* idx comeca em 1 */
    if(a == 0) return idx == b;
    int d = idx - b;
    if(d % a) return 0;
    return d / a >= 0;
}

static int match_list(Node* e, Sel* list);

static int match_comp(Node* e, const Comp* c){
    if(c->tag && strcmp(e->tag, c->tag) != 0) return 0;
    if(c->id){ const char* v = dom_attr(e, "id"); if(!v || strcmp(v, c->id)) return 0; }
    for(int k = 0; k < c->ncls; k++) if(!has_class(e, c->cls[k])) return 0;
    for(int k = 0; k < c->nat; k++) if(!attr_match(e, &c->at[k])) return 0;
    if(c->ps){
        u16 ps = c->ps;
        if(ps & PS_NEVER) return 0;
        if(ps & PS_HOVER){ if(!e->hover) return 0; }
        if(ps & (PS_FIRST | PS_ONLY)) if(prev_elem(e) || !parent_elem(e)) return 0;
        if(ps & (PS_LAST | PS_ONLY)) if(next_elem(e) || !parent_elem(e)) return 0;
        if(ps & PS_ROOT) if(!e->parent || e->parent->type != N_DOC) return 0;
        if(ps & PS_EMPTY) for(Node* k = e->first; k; k = k->next) if(k->type == N_ELEM || (k->type == N_TEXT && k->tlen)) return 0;
        if(ps & PS_LINK) if(!(dom_is(e, "a") || dom_is(e, "area")) || !dom_attr(e, "href")) return 0;
        if(ps & (PS_FIRST_T | PS_ONLY_T)) for(Node* k = prev_elem(e); k; k = prev_elem(k)) if(!strcmp(k->tag, e->tag)) return 0;
        if(ps & (PS_LAST_T | PS_ONLY_T)) for(Node* k = next_elem(e); k; k = next_elem(k)) if(!strcmp(k->tag, e->tag)) return 0;
        if(ps & PS_CHECKED) if(!(e->checked || (dom_is(e, "option") && dom_attr(e, "selected")))) return 0;
        if(ps & PS_DISABLED) if(!dom_attr(e, "disabled")) return 0;
        if(ps & PS_ENABLED) if(dom_attr(e, "disabled")) return 0;
    }
    if(c->nth_kind){
        int idx = 1;
        if(c->nth_kind == 1) for(Node* k = prev_elem(e); k; k = prev_elem(k)) idx++;
        else if(c->nth_kind == 2) for(Node* k = next_elem(e); k; k = next_elem(k)) idx++;
        else if(c->nth_kind == 3){ for(Node* k = prev_elem(e); k; k = prev_elem(k)) if(!strcmp(k->tag, e->tag)) idx++; }
        else for(Node* k = next_elem(e); k; k = next_elem(k)) if(!strcmp(k->tag, e->tag)) idx++;
        if(!nth_ok(c->nth_a, c->nth_b, idx)) return 0;
    }
    for(int k = 0; k < c->nnot; k++) if(match_list(e, c->nots[k])) return 0;
    if(c->is_ && !match_list(e, c->is_)) return 0;
    return 1;
}

static int match_sel(Node* e, const Sel* s, int idx){
    if(!match_comp(e, &s->c[idx])) return 0;
    if(idx == 0) return 1;
    switch(s->c[idx].comb){
    case '>': { Node* p = parent_elem(e); return p && match_sel(p, s, idx - 1); }
    case '+': { Node* p = prev_elem(e); return p && match_sel(p, s, idx - 1); }
    case '~': for(Node* p = prev_elem(e); p; p = prev_elem(p)) if(match_sel(p, s, idx - 1)) return 1; return 0;
    default:  for(Node* p = parent_elem(e); p; p = parent_elem(p)) if(match_sel(p, s, idx - 1)) return 1; return 0;
    }
}
static int match_list(Node* e, Sel* list){
    for(Sel* s = list; s; s = s->next) if(!s->pe && match_sel(e, s, s->n - 1)) return 1;
    return 0;
}

static Arena query_arena;
int css_match_list(Node* el, const char* selector){
    Rd r = { selector, (int)strlen(selector), 0, &query_arena };
    Sel* l = parse_sel_list(&r, 0);
    int res = l ? match_list(el, l) : -1;
    arena_reset(&query_arena);
    return res;
}
int css_query(Node* root, const char* selector, Node** out, int max){
    Rd r = { selector, (int)strlen(selector), 0, &query_arena };
    Sel* l = parse_sel_list(&r, 0);
    if(!l){ arena_reset(&query_arena); return -1; }
    int n = 0;
    for(Node* e = root->first ? root->first : NULL; e && n < max; e = dom_next_in_tree(e, root))
        if(e->type == N_ELEM && match_list(e, l)) out[n++] = e;
    arena_reset(&query_arena);
    return n;
}

/* ============================================================== folhas */
typedef struct { u16 prop; u8 important; const char* name; const char* val; int vlen; } Decl;
/* bloco de declaracoes guardado cru: so e analisado quando alguma regra
 * dele casa com um elemento (a maioria das regras de um site nunca casa) */
typedef struct { const char* src; int len; Arena* a; Decl* d; int nd; u8 parsed; u8 uses_attr; struct Sheet* sheet; } Block;
typedef struct Rule { Sel* sel; u32 spec, order; u8 origin; Block* b; u32 anc[4]; u8 nanc; u8 anc_ready; u8 multi; } Rule;
typedef struct FFace { const char* family; const char* src; int wlo, whi, italic, latin; } FFace;
struct Sheet { Rule** rules; int n, cap; Sheet* next; FFace* faces; int nfaces, cfaces; const char* base; int faces_done; };
static Sheet* ua_sheet = NULL;
static Arena ua_arena;
static int sheets_gen = 0;

static void sheet_add(Sheet* sh, Rule* r){
    if(sh->n == sh->cap){
        sh->cap = sh->cap ? sh->cap * 2 : 64;
        sh->rules = (Rule**)realloc(sh->rules, sizeof(Rule*) * (size_t)sh->cap);
    }
    sh->rules[sh->n++] = r;
}

/* "a: b; c: d !important" -> Decl[] */
/* tira os comentarios de um bloco de declaracoes (fora de strings) */
static const char* strip_comments(Arena* a, const char* s, int* n){
    int has = 0;
    for(int i = 0; i + 1 < *n; i++) if(s[i] == '/' && s[i + 1] == '*'){ has = 1; break; }
    if(!has) return s;
    char* o = (char*)arena_alloc(a, (size_t)*n + 1); int k = 0; char q = 0;
    for(int i = 0; i < *n; i++){
        char c = s[i];
        if(q){ o[k++] = c; if(c == '\\' && i + 1 < *n) o[k++] = s[++i]; else if(c == q) q = 0; continue; }
        if(c == '"' || c == '\''){ q = c; o[k++] = c; continue; }
        if(c == '/' && i + 1 < *n && s[i + 1] == '*'){
            i += 2;
            while(i + 1 < *n && !(s[i] == '*' && s[i + 1] == '/')) i++;
            i++;
            o[k++] = ' ';
            continue;
        }
        o[k++] = c;
    }
    o[k] = 0; *n = k;
    return o;
}

static int parse_decls(Arena* a, const char* s, int n, Decl** out){
    s = strip_comments(a, s, &n);
    int cap = 8, nd = 0;
    Decl* d = (Decl*)arena_alloc(a, sizeof(Decl) * (size_t)cap);
    int i = 0;
    while(i < n){
        while(i < n && (is_ws((u8)s[i]) || s[i] == ';')) i++;
        int n0 = i;
        while(i < n && s[i] != ':' && s[i] != ';') i++;
        if(i >= n || s[i] != ':'){ while(i < n && s[i] != ';') i++; continue; }
        int n1 = i;
        while(n1 > n0 && is_ws((u8)s[n1 - 1])) n1--;
        i++;
        int v0 = i, depth = 0; char q = 0;
        while(i < n){
            char c = s[i];
            if(q){ if(c == '\\') i++; else if(c == q) q = 0; }
            else if(c == '"' || c == '\'') q = c;
            else if(c == '(' || c == '[' || c == '{') depth++;
            else if(c == ')' || c == ']' || c == '}') depth--;
            else if(c == ';' && depth <= 0) break;
            i++;
        }
        int v1 = i;
        while(v0 < v1 && is_ws((u8)s[v0])) v0++;
        while(v1 > v0 && is_ws((u8)s[v1 - 1])) v1--;
        int imp = 0;
        for(int k = v1 - 1; k > v0; k--){
            if(s[k] == '!'){
                if(str_istarts(s + k + 1, "important")){ imp = 1; v1 = k; while(v1 > v0 && is_ws((u8)s[v1 - 1])) v1--; }
                break;
            }
            if(!(s[k] >= 'a' && s[k] <= 'z') && !(s[k] >= 'A' && s[k] <= 'Z') && !is_ws((u8)s[k])) break;
        }
        if(n1 <= n0) continue;
        int pid = prop_id(s + n0, n1 - n0);
        if(pid == P_UNKNOWN) continue;
        if(nd == cap){
            Decl* nd2 = (Decl*)arena_alloc(a, sizeof(Decl) * (size_t)cap * 2);
            memcpy(nd2, d, sizeof(Decl) * (size_t)nd); d = nd2; cap *= 2;
        }
        d[nd].prop = (u16)pid; d[nd].important = (u8)imp;
        d[nd].name = pid == P_CUSTOM ? arena_strndup(a, s + n0, (size_t)(n1 - n0)) : NULL;
        d[nd].val = arena_strndup(a, s + v0, (size_t)(v1 - v0)); d[nd].vlen = v1 - v0;
        nd++;
    }
    *out = d;
    return nd;
}

/* avanca ate o '}' que fecha o bloco aberto em s[i-1] */
static int skip_block(const char* s, int n, int i){
    int depth = 1; char q = 0;
    while(i < n && depth){
        char c = s[i];
        if(q){ if(c == '\\') i++; else if(c == q) q = 0; }
        else if(c == '"' || c == '\'') q = c;
        else if(c == '/' && i + 1 < n && s[i + 1] == '*'){ i += 2; while(i + 1 < n && !(s[i] == '*' && s[i + 1] == '/')) i++; i++; }
        else if(c == '{') depth++;
        else if(c == '}') depth--;
        i++;
    }
    return i;
}

static int display_of(const char* v, int n);
static int supports_ok(const char* s, int n);
/* (propriedade: valor) e suportado? */
static int supports_decl(const char* s, int n){
    int c = 0; while(c < n && s[c] != ':') c++;
    if(c >= n) return 0;
    int a = 0; while(a < c && is_ws((u8)s[a])) a++;
    int b = c; while(b > a && is_ws((u8)s[b - 1])) b--;
    int pid = prop_id(s + a, b - a);
    if(pid == P_UNKNOWN) return 0;
    const char* v = s + c + 1; int vl = n - c - 1;
    while(vl > 0 && is_ws((u8)*v)){ v++; vl--; }
    while(vl > 0 && is_ws((u8)v[vl - 1])) vl--;
    if(pid == P_DISPLAY) return display_of(v, vl) >= 0;
    static const char* const NO[] = { "anchor(", "round(", "mod(", "rem(", "sibling-", "if(", "attr(", "env(", "calc-size(",
        "subgrid", "masonry", "contrast-color(", "from ", "anchor-size(", "stretch", "-webkit-fill-available", "fit-content(", NULL };
    for(int k = 0; NO[k]; k++){
        int l = (int)strlen(NO[k]);
        for(int i = 0; i + l <= vl; i++) if(!strncasecmp(v + i, NO[k], (size_t)l)) return 0;
    }
    return 1;
}
static int supports_ok(const char* s, int n){
    while(n > 0 && is_ws((u8)*s)){ s++; n--; }
    while(n > 0 && is_ws((u8)s[n - 1])) n--;
    if(n <= 0) return 0;
    if(n > 3 && str_istarts(s, "not") && (is_ws((u8)s[3]) || s[3] == '(')) return !supports_ok(s + 3, n - 3);
    /* separa operandos de topo por "and"/"or" */
    int res = -1, op = 0;   /* 1 and, 2 or */
    int i = 0;
    while(i < n){
        while(i < n && is_ws((u8)s[i])) i++;
        if(i >= n) break;
        int t0 = i, val;
        if(s[i] == '('){
            int d = 0;
            for(; i < n; i++){ if(s[i] == '(') d++; else if(s[i] == ')' && --d == 0){ i++; break; } }
            const char* in = s + t0 + 1; int il = i - t0 - 2;
            int k = 0; while(k < il && is_ws((u8)in[k])) k++;
            if(k < il && (in[k] == '(' || str_istarts(in + k, "not"))) val = supports_ok(in, il);
            else val = supports_decl(in, il);
        } else if(str_istarts(s + i, "selector(")){
            int d = 0; for(; i < n; i++){ if(s[i] == '(') d++; else if(s[i] == ')' && --d == 0){ i++; break; } }
            val = !str_icontains(s + t0, ":has(") || str_icontains(s + t0, ":has(") > s + i;
        } else if(str_istarts(s + i, "and") && i + 3 < n && is_ws((u8)s[i + 3])){ op = 1; i += 3; continue; }
        else if(str_istarts(s + i, "or") && i + 2 < n && is_ws((u8)s[i + 2])){ op = 2; i += 2; continue; }
        else { while(i < n && !is_ws((u8)s[i]) && s[i] != '(') i++; if(i < n && s[i] == '('){ int d = 0; for(; i < n; i++){ if(s[i] == '(') d++; else if(s[i] == ')' && --d == 0){ i++; break; } } } val = 0; }
        if(res < 0) res = val;
        else if(op == 2) res = res || val;
        else res = res && val;
    }
    return res > 0;
}

/* @font-face: guarda family/src/peso/estilo; o registro (e a URL absoluta)
 * fica para a thread principal, quando a folha entra no documento */
static int is_hex(int c){ return (c >= '0' && c <= '9') || (c >= 'a' && c <= 'f') || (c >= 'A' && c <= 'F'); }
static int urange_has_latin(const char* v, int n){
    int i = 0;
    while(i < n){
        while(i < n && (is_ws((u8)v[i]) || v[i] == ',')) i++;
        if(i + 2 >= n || lower((u8)v[i]) != 'u' || v[i + 1] != '+') { while(i < n && v[i] != ',') i++; continue; }
        i += 2;
        u32 lo = 0, hi; int wild = 0;
        while(i < n && (is_hex((u8)v[i]) || v[i] == '?')){
            int c = lower((u8)v[i]); lo = lo * 16 + (u32)(c == '?' ? 0 : c <= '9' ? c - '0' : c - 'a' + 10);
            if(c == '?') wild = wild * 16 + 15; i++;
        }
        hi = lo + (u32)wild;
        if(i < n && v[i] == '-'){
            i++; hi = 0;
            while(i < n && is_hex((u8)v[i])){ int c = lower((u8)v[i]); hi = hi * 16 + (u32)(c <= '9' ? c - '0' : c - 'a' + 10); i++; }
        }
        if(lo <= 0x61 && hi >= 0x61) return 1;         /* 'a' */
    }
    return 0;
}
static void font_face_rule(Sheet* sh, Arena* a, const char* s, int n){
    s = strip_comments(a, s, &n);
    FFace f; memset(&f, 0, sizeof f); f.wlo = f.whi = 400; f.latin = 1;
    int i = 0;
    while(i < n){
        while(i < n && (is_ws((u8)s[i]) || s[i] == ';')) i++;
        int n0 = i; while(i < n && s[i] != ':' && s[i] != ';') i++;
        if(i >= n || s[i] != ':') continue;
        int n1 = i; while(n1 > n0 && is_ws((u8)s[n1 - 1])) n1--;
        i++;
        int v0 = i; char q = 0; int depth = 0;
        while(i < n){ char c = s[i]; if(q){ if(c == q) q = 0; } else if(c == '"' || c == '\'') q = c; else if(c == '(') depth++; else if(c == ')') depth--; else if(c == ';' && !depth) break; i++; }
        int v1 = i;
        while(v0 < v1 && is_ws((u8)s[v0])) v0++;
        while(v1 > v0 && is_ws((u8)s[v1 - 1])) v1--;
        const char* nm = s + n0; int nl = n1 - n0; const char* v = s + v0; int vl = v1 - v0;
        if(str_ieqn(nm, "font-family", nl)){
            char t[64]; int k = 0;
            for(int j = 0; j < vl && k < 63; j++) if(v[j] != '"' && v[j] != '\'') t[k++] = (char)lower((u8)v[j]);
            while(k && is_ws((u8)t[k - 1])) k--;
            t[k] = 0;
            f.family = arena_strdup(a, t);
        } else if(str_ieqn(nm, "src", nl)) f.src = arena_strndup(a, v, (size_t)vl);
        else if(str_ieqn(nm, "font-weight", nl)){
            if(str_istarts(v, "bold")) f.wlo = f.whi = 700;
            else if(v[0] >= '1' && v[0] <= '9'){
                f.wlo = f.whi = atoi(v);
                const char* sp = v; while(sp < v + vl && !is_ws((u8)*sp)) sp++;
                while(sp < v + vl && is_ws((u8)*sp)) sp++;
                if(sp < v + vl && *sp >= '1' && *sp <= '9') f.whi = atoi(sp);     /* "100 900": fonte variavel */
            }
        } else if(str_ieqn(nm, "font-style", nl)) f.italic = str_istarts(v, "italic") || str_istarts(v, "oblique");
        else if(str_ieqn(nm, "unicode-range", nl)) f.latin = urange_has_latin(v, vl);
    }
    if(!f.family || !f.src || !f.latin) return;
    if(sh->nfaces == sh->cfaces){
        int nc = sh->cfaces ? sh->cfaces * 2 : 8;
        FFace* nf = (FFace*)arena_alloc(a, sizeof(FFace) * (size_t)nc);
        if(sh->nfaces) memcpy(nf, sh->faces, sizeof(FFace) * (size_t)sh->nfaces);
        sh->faces = nf; sh->cfaces = nc;
    }
    sh->faces[sh->nfaces++] = f;
}

/* escolhe a melhor url() do src (woff2 > woff > ttf/otf) e registra */
static void register_faces(Sheet* sh){
    sh->faces_done = 1;
    for(int k = 0; k < sh->nfaces; k++){
        FFace* f = &sh->faces[k];
        const char* p = f->src;
        char best[URLMAX]; best[0] = 0; int bscore = -1;
        while((p = strstr(p, "url("))){
            p += 4;
            while(is_ws((u8)*p)) p++;
            char q = (*p == '"' || *p == '\'') ? *p++ : 0;
            const char* u0 = p;
            while(*p && (q ? *p != q : (*p != ')' && !is_ws((u8)*p)))) p++;
            int ul = (int)(p - u0);
            const char* fmt = strstr(p, "format(");
            const char* next = strstr(p, "url(");
            if(fmt && next && fmt > next) fmt = NULL;
            int sc = 1;
            if(fmt){
                if(str_istarts(fmt + 7, "\"woff2") || str_istarts(fmt + 7, "'woff2") || str_istarts(fmt + 7, "woff2")) sc = 4;
                else if(strstr(fmt, "woff") && strstr(fmt, "woff") < fmt + 16) sc = 3;
                else if(strstr(fmt, "truetype") || strstr(fmt, "opentype")) sc = 2;
                else sc = -1;                                  /* svg, embedded-opentype... */
            } else {
                if(ul > 6 && str_icontains(u0, ".woff2") && str_icontains(u0, ".woff2") < u0 + ul) sc = 4;
                else if(ul > 5 && str_icontains(u0, ".eot") && str_icontains(u0, ".eot") < u0 + ul) sc = -1;
            }
            if(sc > bscore && ul > 0 && ul < URLMAX - 1){
                char rel[URLMAX]; memcpy(rel, u0, (size_t)ul); rel[ul] = 0;
                if(str_istarts(rel, "data:")) snprintf(best, sizeof best, "%s", rel);
                else if(!url_resolve(sh->base ? sh->base : (doc ? doc->url : ""), rel, best, sizeof best)) continue;
                bscore = sc;
            }
        }
        if(best[0] && bscore > 0) font_face_add(f->family, best, f->wlo, f->whi, f->italic);
    }
}

void css_sheet_set_base(Sheet* sh, Arena* a, const char* url){ if(sh) sh->base = arena_strdup(a, url); }

static void parse_rules(Sheet* sh, Arena* a, const char* s, int n, int origin){
    int i = 0;
    while(i < n){
        while(i < n && (is_ws((u8)s[i]) || s[i] == ';' || s[i] == '}')) i++;
        if(i + 1 < n && s[i] == '/' && s[i + 1] == '*'){
            i += 2; while(i + 1 < n && !(s[i] == '*' && s[i + 1] == '/')) i++;
            i += 2; continue;
        }
        if(i + 3 < n && !memcmp(s + i, "<!--", 4)){ i += 4; continue; }
        if(i + 2 < n && !memcmp(s + i, "-->", 3)){ i += 3; continue; }
        if(i >= n) break;
        /* preludio ate '{' ou ';' */
        int p0 = i; char q = 0; int paren = 0;
        while(i < n){
            char c = s[i];
            if(q){ if(c == '\\') i++; else if(c == q) q = 0; }
            else if(c == '"' || c == '\'') q = c;
            else if(c == '(') paren++;
            else if(c == ')') paren--;
            else if(c == '/' && i + 1 < n && s[i + 1] == '*'){ i += 2; while(i + 1 < n && !(s[i] == '*' && s[i + 1] == '/')) i++; i++; }
            else if(paren <= 0 && (c == '{' || c == ';')) break;
            i++;
        }
        int p1 = i;
        if(i >= n) break;
        if(s[p0] == '@'){
            if(s[i] == ';'){ i++; continue; }                   /* @import, @charset */
            int b0 = i + 1;
            int b1 = skip_block(s, n, b0);
            i = b1;
            const char* at = s + p0 + 1;
            if(str_istarts(at, "media")){
                char q2[512]; int ql = p1 - p0 - 6; if(ql > 511) ql = 511; if(ql < 0) ql = 0;
                memcpy(q2, s + p0 + 6, (size_t)ql); q2[ql] = 0;
                if(css_media_ok(q2, ql)) parse_rules(sh, a, s + b0, b1 - 1 - b0, origin);
            } else if(str_istarts(at, "supports")){
                if(supports_ok(s + p0 + 9, p1 - p0 - 9)) parse_rules(sh, a, s + b0, b1 - 1 - b0, origin);
            } else if(str_istarts(at, "font-face")){
                font_face_rule(sh, a, s + b0, b1 - 1 - b0);
            } else if(str_istarts(at, "layer") || str_istarts(at, "container") || str_istarts(at, "scope")
                || str_istarts(at, "document") || str_istarts(at, "-moz-document")){
                parse_rules(sh, a, s + b0, b1 - 1 - b0, origin);
            }
            continue;                                           /* @font-face, @keyframes, @page... */
        }
        if(s[i] == ';'){ i++; continue; }
        int b0 = i + 1;
        int b1 = skip_block(s, n, b0);
        i = b1;
        Rd r = { s + p0, p1 - p0, 0, a };
        Sel* list = parse_sel_list(&r, 0);
        if(!list) continue;
        int bl = b1 - 1 - b0;
        int empty = 1;
        for(int k = 0; k < bl; k++) if(!is_ws((u8)s[b0 + k])){ empty = 0; break; }
        if(empty) continue;
        Block* blk = (Block*)arena_alloc(a, sizeof(Block));
        blk->src = arena_strndup(a, s + b0, (size_t)bl); blk->len = bl; blk->a = a; blk->sheet = sh;
        for(Sel* sl = list; sl; sl = sl->next){
            Rule* ru = (Rule*)arena_alloc(a, sizeof(Rule));
            ru->sel = sl; ru->spec = sl->spec; ru->origin = (u8)origin;   /* order: na montagem do indice */
            ru->b = blk;
            sheet_add(sh, ru);
        }
    }
}

Sheet* css_parse_sheet(Arena* a, const char* src, int len, int origin){
    Sheet* sh = (Sheet*)arena_alloc(a, sizeof(Sheet));
    parse_rules(sh, a, src, len, origin);
    return sh;
}
void css_add_sheet(Doc* d, Sheet* s){
    s->next = NULL;
    Sheet** pp = &d->sheets;
    while(*pp) pp = &(*pp)->next;
    *pp = s;
    sheets_gen++;
}

int css_set_sheets(Doc* d, Sheet** list, int n){
    Sheet* cur = d->sheets; int same = 1;
    for(int i = 0; i < n; i++, cur = cur ? cur->next : NULL) if(cur != list[i]){ same = 0; break; }
    if(same && !cur) return 0;
    d->sheets = NULL;
    for(int i = 0; i < n; i++) if(!list[i]->faces_done) register_faces(list[i]);
    for(int i = 0; i < n; i++) list[i]->next = i + 1 < n ? list[i + 1] : NULL;
    d->sheets = n ? list[0] : NULL;
    sheets_gen++;
    return 1;
}

/* ================================================================ media */
/* sintaxe de intervalo: "width >= 600px", "400px <= width < 700px" */
static int media_range(const char* f){
    char opd[3][48]; int nopd = 0; char ops[2][3]; int nops = 0;
    const char* p = f;
    while(*p && nopd < 3){
        while(is_ws((u8)*p)) p++;
        int k = 0;
        while(*p && *p != '<' && *p != '>' && *p != '='){ if(!is_ws((u8)*p) && k < 47) opd[nopd][k++] = *p; p++; }
        opd[nopd++][k] = 0;
        if(!*p || nops == 2) break;
        int o = 0; while((*p == '<' || *p == '>' || *p == '=') && o < 2) ops[nops][o++] = *p++;
        ops[nops++][o] = 0;
    }
    if(nops < 1 || nopd != nops + 1) return 0;
    int res = 1;
    for(int i = 0; i < nops; i++){
        const char* a = opd[i]; const char* b = opd[i + 1];
        int dim_left = !strcmp(a, "width") || !strcmp(a, "height");
        const char* feat = dim_left ? a : b;
        const char* val = dim_left ? b : a;
        i32 dim = !strcmp(feat, "height") ? css_vh : css_vw;
        i32 v = (i32)atoi(val);
        if(strstr(val, "em")) v *= 16;
        const char* op = ops[i];
        int lt = op[0] == '<', gt = op[0] == '>', eq = op[0] == '=' || op[1] == '=';
        /* normaliza para "dim OP v" */
        if(!dim_left){ int t = lt; lt = gt; gt = t; }
        int r = lt ? (eq ? dim <= v : dim < v) : gt ? (eq ? dim >= v : dim > v) : dim == v;
        if(!r) res = 0;
    }
    return res;
}
static int media_one(const char* s, int n){
    /* "screen and (min-width: 600px)", "not print", "(width >= 40em)" */
    int neg = 0, ok = 1, i = 0;
    while(i < n){
        while(i < n && is_ws((u8)s[i])) i++;
        if(i >= n) break;
        if(s[i] == '('){
            int j = i + 1, depth = 1;
            while(j < n && depth){ if(s[j] == '(') depth++; else if(s[j] == ')') depth--; j++; }
            char f[128]; int fl = j - i - 2; if(fl > 127) fl = 127; if(fl < 0) fl = 0;
            memcpy(f, s + i + 1, (size_t)fl); f[fl] = 0;
            for(int k = 0; f[k]; k++) f[k] = (char)lower((u8)f[k]);
            int res = 0;
            char* colon = strchr(f, ':');
            const char* ops = NULL;
            for(char* p = f; *p; p++) if(*p == '<' || *p == '>' || (*p == '=' && !colon)){ ops = p; break; }
            if(colon){
                *colon = 0;
                char* name = f; while(is_ws((u8)*name)) name++;
                char* e = colon - 1; while(e > name && is_ws((u8)*e)) *e-- = 0;
                char* v = colon + 1; while(is_ws((u8)*v)) v++;
                i32 px = (i32)atoi(v);
                if(strstr(v, "em")) px *= 16;
                if(!strcmp(name, "min-width")) res = css_vw >= px;
                else if(!strcmp(name, "max-width")) res = css_vw <= px;
                else if(!strcmp(name, "width")) res = css_vw == px;
                else if(!strcmp(name, "min-height")) res = css_vh >= px;
                else if(!strcmp(name, "max-height")) res = css_vh <= px;
                else if(!strcmp(name, "orientation")) res = str_istarts(v, "landscape") ? css_vw >= css_vh : css_vw < css_vh;
                else if(!strcmp(name, "prefers-color-scheme")) res = str_istarts(v, "light");
                else if(!strcmp(name, "prefers-reduced-motion")) res = str_istarts(v, "reduce");
                else if(!strcmp(name, "hover") || !strcmp(name, "any-hover")) res = str_istarts(v, "hover");
                else if(!strcmp(name, "pointer") || !strcmp(name, "any-pointer")) res = str_istarts(v, "fine");
                else if(strstr(name, "device-pixel-ratio") || strstr(name, "resolution")) res = !strncmp(name, "max", 3) || atoi(v) <= 1;
                else if(!strcmp(name, "scripting")) res = str_istarts(v, "enabled");
                else if(!strcmp(name, "display-mode")) res = str_istarts(v, "browser");
                else res = 0;
            } else if(ops){
                res = media_range(f);
            } else {
                char* name = f; while(is_ws((u8)*name)) name++;
                res = !strcmp(name, "color") || !strcmp(name, "hover") || !strcmp(name, "grid");
            }
            if(!res) ok = 0;
            i = j;
            continue;
        }
        int j = i;
        while(j < n && !is_ws((u8)s[j]) && s[j] != '(') j++;
        int wl = j - i;
        if(str_ieqn(s + i, "not", wl)) neg = 1;
        else if(str_ieqn(s + i, "only", wl) || str_ieqn(s + i, "and", wl)){}
        else if(str_ieqn(s + i, "screen", wl) || str_ieqn(s + i, "all", wl)){}
        else ok = 0;                                    /* print, speech, ... */
        i = j;
    }
    return neg ? !ok : ok;
}
int css_media_ok(const char* q, int n){
    int i = 0, any = 0;
    while(i < n && is_ws((u8)q[i])) i++;
    if(i >= n) return 1;
    while(i < n){
        int j = i, depth = 0;
        while(j < n && !(q[j] == ',' && depth == 0)){ if(q[j] == '(') depth++; else if(q[j] == ')') depth--; j++; }
        if(media_one(q + i, j - i)) any = 1;
        i = j + 1;
    }
    return any;
}

/* ================================================================ cores */
static const struct { const char* n; u32 c; } COLORS[] = {
    {"aliceblue",0xF0F8FF},{"antiquewhite",0xFAEBD7},{"aqua",0x00FFFF},{"aquamarine",0x7FFFD4},{"azure",0xF0FFFF},
    {"beige",0xF5F5DC},{"bisque",0xFFE4C4},{"black",0x000000},{"blanchedalmond",0xFFEBCD},{"blue",0x0000FF},
    {"blueviolet",0x8A2BE2},{"brown",0xA52A2A},{"burlywood",0xDEB887},{"cadetblue",0x5F9EA0},{"chartreuse",0x7FFF00},
    {"chocolate",0xD2691E},{"coral",0xFF7F50},{"cornflowerblue",0x6495ED},{"cornsilk",0xFFF8DC},{"crimson",0xDC143C},
    {"cyan",0x00FFFF},{"darkblue",0x00008B},{"darkcyan",0x008B8B},{"darkgoldenrod",0xB8860B},{"darkgray",0xA9A9A9},
    {"darkgreen",0x006400},{"darkgrey",0xA9A9A9},{"darkkhaki",0xBDB76B},{"darkmagenta",0x8B008B},{"darkolivegreen",0x556B2F},
    {"darkorange",0xFF8C00},{"darkorchid",0x9932CC},{"darkred",0x8B0000},{"darksalmon",0xE9967A},{"darkseagreen",0x8FBC8F},
    {"darkslateblue",0x483D8B},{"darkslategray",0x2F4F4F},{"darkslategrey",0x2F4F4F},{"darkturquoise",0x00CED1},
    {"darkviolet",0x9400D3},{"deeppink",0xFF1493},{"deepskyblue",0x00BFFF},{"dimgray",0x696969},{"dimgrey",0x696969},
    {"dodgerblue",0x1E90FF},{"firebrick",0xB22222},{"floralwhite",0xFFFAF0},{"forestgreen",0x228B22},{"fuchsia",0xFF00FF},
    {"gainsboro",0xDCDCDC},{"ghostwhite",0xF8F8FF},{"gold",0xFFD700},{"goldenrod",0xDAA520},{"gray",0x808080},
    {"green",0x008000},{"greenyellow",0xADFF2F},{"grey",0x808080},{"honeydew",0xF0FFF0},{"hotpink",0xFF69B4},
    {"indianred",0xCD5C5C},{"indigo",0x4B0082},{"ivory",0xFFFFF0},{"khaki",0xF0E68C},{"lavender",0xE6E6FA},
    {"lavenderblush",0xFFF0F5},{"lawngreen",0x7CFC00},{"lemonchiffon",0xFFFACD},{"lightblue",0xADD8E6},
    {"lightcoral",0xF08080},{"lightcyan",0xE0FFFF},{"lightgoldenrodyellow",0xFAFAD2},{"lightgray",0xD3D3D3},
    {"lightgreen",0x90EE90},{"lightgrey",0xD3D3D3},{"lightpink",0xFFB6C1},{"lightsalmon",0xFFA07A},
    {"lightseagreen",0x20B2AA},{"lightskyblue",0x87CEFA},{"lightslategray",0x778899},{"lightslategrey",0x778899},
    {"lightsteelblue",0xB0C4DE},{"lightyellow",0xFFFFE0},{"lime",0x00FF00},{"limegreen",0x32CD32},{"linen",0xFAF0E6},
    {"magenta",0xFF00FF},{"maroon",0x800000},{"mediumaquamarine",0x66CDAA},{"mediumblue",0x0000CD},
    {"mediumorchid",0xBA55D3},{"mediumpurple",0x9370DB},{"mediumseagreen",0x3CB371},{"mediumslateblue",0x7B68EE},
    {"mediumspringgreen",0x00FA9A},{"mediumturquoise",0x48D1CC},{"mediumvioletred",0xC71585},{"midnightblue",0x191970},
    {"mintcream",0xF5FFFA},{"mistyrose",0xFFE4E1},{"moccasin",0xFFE4B5},{"navajowhite",0xFFDEAD},{"navy",0x000080},
    {"oldlace",0xFDF5E6},{"olive",0x808000},{"olivedrab",0x6B8E23},{"orange",0xFFA500},{"orangered",0xFF4500},
    {"orchid",0xDA70D6},{"palegoldenrod",0xEEE8AA},{"palegreen",0x98FB98},{"paleturquoise",0xAFEEEE},
    {"palevioletred",0xDB7093},{"papayawhip",0xFFEFD5},{"peachpuff",0xFFDAB9},{"peru",0xCD853F},{"pink",0xFFC0CB},
    {"plum",0xDDA0DD},{"powderblue",0xB0E0E6},{"purple",0x800080},{"rebeccapurple",0x663399},{"red",0xFF0000},
    {"rosybrown",0xBC8F8F},{"royalblue",0x4169E1},{"saddlebrown",0x8B4513},{"salmon",0xFA8072},{"sandybrown",0xF4A460},
    {"seagreen",0x2E8B57},{"seashell",0xFFF5EE},{"sienna",0xA0522D},{"silver",0xC0C0C0},{"skyblue",0x87CEEB},
    {"slateblue",0x6A5ACD},{"slategray",0x708090},{"slategrey",0x708090},{"snow",0xFFFAFA},{"springgreen",0x00FF7F},
    {"steelblue",0x4682B4},{"tan",0xD2B48C},{"teal",0x008080},{"thistle",0xD8BFD8},{"tomato",0xFF6347},
    {"turquoise",0x40E0D0},{"violet",0xEE82EE},{"wheat",0xF5DEB3},{"white",0xFFFFFF},{"whitesmoke",0xF5F5F5},
    {"yellow",0xFFFF00},{"yellowgreen",0x9ACD32},
    /* cores do sistema */
    {"canvas",0xFFFFFF},{"canvastext",0x000000},{"linktext",0x0000EE},{"visitedtext",0x551A8B},{"buttonface",0xEFEFEF},
    {"buttontext",0x000000},{"buttonborder",0x767676},{"field",0xFFFFFF},{"fieldtext",0x000000},{"graytext",0x808080},
    {"highlight",0x3390FF},{"highlighttext",0xFFFFFF},{"window",0xFFFFFF},{"windowtext",0x000000},
};

/* numero com fracao -> x1000 (inteiro, sem ponto flutuante) */
static int parse_num(const char* s, int n, int* i, i32* out){
    int k = *i, neg = 0;
    if(k < n && (s[k] == '-' || s[k] == '+')){ neg = s[k] == '-'; k++; }
    i64 ip = 0, fp = 0; int fd = 0, any = 0;
    while(k < n && s[k] >= '0' && s[k] <= '9'){ if(ip < 100000000) ip = ip * 10 + (s[k] - '0'); k++; any = 1; }
    if(k < n && s[k] == '.' && k + 1 < n && s[k + 1] >= '0' && s[k + 1] <= '9'){
        k++;
        while(k < n && s[k] >= '0' && s[k] <= '9'){ if(fd < 3){ fp = fp * 10 + (s[k] - '0'); fd++; } k++; any = 1; }
    }
    if(!any) return 0;
    while(fd < 3){ fp *= 10; fd++; }
    i64 v = ip * 1000 + fp;
    if(k < n && (s[k] == 'e' || s[k] == 'E') && k + 1 < n && ((s[k + 1] >= '0' && s[k + 1] <= '9') || s[k + 1] == '-' || s[k + 1] == '+')){
        k++; int en = 0, e = 0;
        if(s[k] == '-' || s[k] == '+'){ en = s[k] == '-'; k++; }
        while(k < n && s[k] >= '0' && s[k] <= '9'){ e = e * 10 + (s[k] - '0'); k++; }
        while(e-- > 0){ if(en) v /= 10; else if(v < 100000000000LL) v *= 10; }
    }
    *out = (i32)(neg ? -v : v);
    *i = k;
    return 1;
}

static int hexv(int c){ c = lower(c); return c >= '0' && c <= '9' ? c - '0' : c >= 'a' && c <= 'f' ? c - 'a' + 10 : -1; }
static i32 clamp255(i32 v){ return v < 0 ? 0 : v > 255 ? 255 : v; }

static u32 hsl_to_rgb(i32 h, i32 s, i32 l){            /* h graus, s/l em 0..1000 */
    h = ((h % 360) + 360) % 360;
    i32 c = (1000 - (2 * l - 1000 < 0 ? -(2 * l - 1000) : 2 * l - 1000)) * s / 1000;
    i32 hp = h * 1000 / 60;
    i32 m2 = hp % 2000 - 1000; if(m2 < 0) m2 = -m2;
    i32 x = c * (1000 - m2) / 1000;
    i32 r = 0, g = 0, b = 0;
    if(h < 60){ r = c; g = x; } else if(h < 120){ r = x; g = c; } else if(h < 180){ g = c; b = x; }
    else if(h < 240){ g = x; b = c; } else if(h < 300){ r = x; b = c; } else { r = c; b = x; }
    i32 m = l - c / 2;
    return ((u32)clamp255((r + m) * 255 / 1000) << 16) | ((u32)clamp255((g + m) * 255 / 1000) << 8) | (u32)clamp255((b + m) * 255 / 1000);
}

/* espacos de cor modernos -> sRGB (ponto flutuante; so na analise do CSS) */
static double srgb_gamma(double x){ return x <= 0.0031308 ? 12.92 * x : 1.055 * pow(x, 1 / 2.4) - 0.055; }
static u32 pack_rgb(double r, double g, double b){
    i32 R = (i32)(srgb_gamma(r) * 255 + 0.5), G = (i32)(srgb_gamma(g) * 255 + 0.5), B = (i32)(srgb_gamma(b) * 255 + 0.5);
    return ((u32)clamp255(R) << 16) | ((u32)clamp255(G) << 8) | (u32)clamp255(B);
}
static u32 oklab_rgb(double L, double a, double b){
    double l_ = L + 0.3963377774 * a + 0.2158037573 * b;
    double m_ = L - 0.1055613458 * a - 0.0638541728 * b;
    double s_ = L - 0.0894841775 * a - 1.2914855480 * b;
    double l = l_ * l_ * l_, m = m_ * m_ * m_, s = s_ * s_ * s_;
    return pack_rgb(4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s,
                   -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s,
                   -0.0041960863 * l - 0.7034186147 * m + 1.7076147010 * s);
}
static u32 lab_rgb(double L, double a, double b){
    /* CIE Lab (D50) -> XYZ -> sRGB linear (Bradford para D65 embutido) */
    double fy = (L + 16) / 116, fx = fy + a / 500, fz = fy - b / 200;
    double e = 216.0 / 24389, k = 24389.0 / 27;
    double x = (fx * fx * fx > e ? fx * fx * fx : (116 * fx - 16) / k) * 0.96422;
    double y = L > k * e ? fy * fy * fy : L / k;
    double z = (fz * fz * fz > e ? fz * fz * fz : (116 * fz - 16) / k) * 0.82521;
    return pack_rgb(3.1338561 * x - 1.6168667 * y - 0.4906146 * z,
                   -0.9787684 * x + 1.9161415 * y + 0.0334540 * z,
                    0.0719453 * x - 0.2289914 * y + 1.4052427 * z);
}

u32 css_parse_color(const char* s, int n, int* ok){
    *ok = 0;
    while(n > 0 && is_ws((u8)*s)){ s++; n--; }
    while(n > 0 && is_ws((u8)s[n - 1])) n--;
    if(n <= 0) return 0;
    if(s[0] == '#'){
        int d[8], k = 0;
        for(int i = 1; i < n && k < 8; i++){ d[k] = hexv((u8)s[i]); if(d[k] < 0) return 0; k++; }
        u32 r, g, b, a = 255;
        if(k == 3 || k == 4){ r = (u32)d[0] * 17; g = (u32)d[1] * 17; b = (u32)d[2] * 17; if(k == 4) a = (u32)d[3] * 17; }
        else if(k == 6 || k == 8){ r = (u32)(d[0] * 16 + d[1]); g = (u32)(d[2] * 16 + d[3]); b = (u32)(d[4] * 16 + d[5]); if(k == 8) a = (u32)(d[6] * 16 + d[7]); }
        else return 0;
        *ok = 1;
        return (a << 24) | (r << 16) | (g << 8) | b;
    }
    if(str_ieqn(s, "transparent", n)){ *ok = 1; return 0; }
    int fn = 0;
    while(fn < n && s[fn] != '(') fn++;
    if(fn < n && (str_ieqn(s, "light-dark", fn) || str_ieqn(s, "color-mix", fn))){
        /* light-dark(a, b): tema claro -> a.  color-mix(in x, a p%, b q%) */
        const char* arg[3]; int al[3]; int na = 0, d = 0, st = fn + 1;
        for(int i = fn + 1; i < n && na < 3; i++){
            if(s[i] == '(') d++;
            else if(s[i] == ')'){ if(d == 0){ arg[na] = s + st; al[na++] = i - st; break; } d--; }
            else if(s[i] == ',' && d == 0){ arg[na] = s + st; al[na++] = i - st; st = i + 1; }
        }
        if(str_ieqn(s, "light-dark", fn)) return na >= 1 ? css_parse_color(arg[0], al[0], ok) : 0;
        if(na < 3) return 0;
        i32 p1 = -1, p2 = -1; u32 c1, c2; int o1, o2;
        for(int k = 1; k <= 2; k++){
            const char* a = arg[k]; int l = al[k];
            while(l > 0 && is_ws((u8)a[l - 1])) l--;
            int e = l; while(e > 0 && a[e - 1] != ' ') e--;
            i32 pc = -1;
            if(e > 0 && l > e && a[l - 1] == '%'){ int j = e; i32 v; if(parse_num(a, l, &j, &v)) pc = v / 1000; l = e; }
            if(k == 1){ c1 = css_parse_color(a, l, &o1); p1 = pc; } else { c2 = css_parse_color(a, l, &o2); p2 = pc; }
        }
        if(!o1 || !o2) return 0;
        if(p1 < 0) p1 = p2 >= 0 ? 100 - p2 : 50;
        u32 w = (u32)(p1 * 255 / 100);
        u32 a1 = c1 >> 24, a2 = c2 >> 24;
        *ok = 1;
        return ((a1 * w + a2 * (255 - w)) / 255 << 24) | mix(c2 & 0xFFFFFF, c1 & 0xFFFFFF, w);
    }
    if(fn < n && (str_ieqn(s, "oklch", fn) || str_ieqn(s, "oklab", fn) || str_ieqn(s, "lab", fn) || str_ieqn(s, "lch", fn)
                  || str_ieqn(s, "hwb", fn) || str_ieqn(s, "color", fn))){
        int i = fn + 1;
        if(str_ieqn(s, "color", fn)){                       /* color(srgb r g b) */
            while(i < n && is_ws((u8)s[i])) i++;
            if(!str_istarts(s + i, "srgb")) return 0;
            i += 4; if(i < n && s[i] == '-') return 0;          /* srgb-linear */
        }
        i32 v[4] = {0, 0, 0, 1000}; int pct[4] = {0}, k = 0;
        while(i < n && k < 4){
            while(i < n && (is_ws((u8)s[i]) || s[i] == ',' || s[i] == '/')) i++;
            if(i >= n || s[i] == ')') break;
            if(str_istarts(s + i, "none")){ v[k++] = 0; i += 4; continue; }
            if(!parse_num(s, n, &i, &v[k])) return 0;
            if(i < n && s[i] == '%'){ pct[k] = 1; i++; }
            while(i < n && s[i] >= 'a' && s[i] <= 'z') i++;
            k++;
        }
        if(k < 3) return 0;
        u32 a = (u32)clamp255(pct[3] ? v[3] * 255 / 100000 : v[3] * 255 / 1000);
        double x = v[0] / 1000.0, y = v[1] / 1000.0, z = v[2] / 1000.0;
        u32 col;
        if(str_ieqn(s, "oklch", fn) || str_ieqn(s, "oklab", fn)){
            double L = pct[0] ? x / 100 : x;
            if(str_ieqn(s, "oklch", fn)){
                double C = pct[1] ? y * 0.4 / 100 : y, h = z * 3.14159265358979 / 180;
                col = oklab_rgb(L, C * cos(h), C * sin(h));
            } else col = oklab_rgb(L, pct[1] ? y * 0.4 / 100 : y, pct[2] ? z * 0.4 / 100 : z);
        } else if(str_ieqn(s, "lch", fn)){
            double C = pct[1] ? y * 1.5 : y, h = z * 3.14159265358979 / 180;
            col = lab_rgb(x, C * cos(h), C * sin(h));
        } else if(str_ieqn(s, "lab", fn)) col = lab_rgb(x, pct[1] ? y * 1.25 : y, pct[2] ? z * 1.25 : z);
        else if(str_ieqn(s, "hwb", fn)){
            double w = y / 100, bl = z / 100;
            if(w + bl >= 1){ u32 g = (u32)clamp255((i32)(w / (w + bl) * 255)); col = (g << 16) | (g << 8) | g; }
            else {
                u32 c = hsl_to_rgb(v[0] / 1000, 1000, 500);
                double f = 1 - w - bl;
                i32 r = (i32)(((c >> 16) & 255) * f + w * 255), g = (i32)(((c >> 8) & 255) * f + w * 255), b = (i32)((c & 255) * f + w * 255);
                col = ((u32)clamp255(r) << 16) | ((u32)clamp255(g) << 8) | (u32)clamp255(b);
            }
        } else {
            double r = pct[0] ? x / 100 : x, g = pct[1] ? y / 100 : y, b = pct[2] ? z / 100 : z;
            col = ((u32)clamp255((i32)(r * 255 + 0.5)) << 16) | ((u32)clamp255((i32)(g * 255 + 0.5)) << 8) | (u32)clamp255((i32)(b * 255 + 0.5));
        }
        *ok = 1;
        return (a << 24) | col;
    }
    if(fn < n){
        int rgb = str_ieqn(s, "rgb", fn) || str_ieqn(s, "rgba", fn);
        int hsl = str_ieqn(s, "hsl", fn) || str_ieqn(s, "hsla", fn);
        if(!rgb && !hsl) return 0;
        i32 v[4] = {0, 0, 0, 1000}; int pct[4] = {0}, k = 0, i = fn + 1;
        while(i < n && k < 4){
            while(i < n && (is_ws((u8)s[i]) || s[i] == ',' || s[i] == '/')) i++;
            if(i >= n || s[i] == ')') break;
            if(!parse_num(s, n, &i, &v[k])){
                if(str_istarts(s + i, "none")){ v[k] = 0; i += 4; k++; continue; }
                return 0;
            }
            if(i < n && s[i] == '%'){ pct[k] = 1; i++; }
            while(i < n && ((s[i] >= 'a' && s[i] <= 'z'))) i++;    /* deg */
            k++;
        }
        if(k < 3) return 0;
        u32 a = 255;
        if(k == 4) a = (u32)clamp255(pct[3] ? v[3] * 255 / 100000 : v[3] * 255 / 1000);
        u32 col;
        if(rgb){
            i32 c[3];
            for(int j = 0; j < 3; j++) c[j] = clamp255(pct[j] ? v[j] * 255 / 100000 : v[j] / 1000);
            col = ((u32)c[0] << 16) | ((u32)c[1] << 8) | (u32)c[2];
        } else col = hsl_to_rgb(v[0] / 1000, pct[1] ? v[1] / 100 : v[1] / 100, pct[2] ? v[2] / 100 : v[2] / 100);
        *ok = 1;
        return (a << 24) | col;
    }
    for(size_t i = 0; i < sizeof COLORS / sizeof COLORS[0]; i++)
        if(str_ieqn(s, COLORS[i].n, n)){ *ok = 1; return 0xFF000000 | COLORS[i].c; }
    return 0;
}

/* ============================================================ valores */
#define CUR_COLOR 0x01020304u           /* sentinela: currentColor */

typedef struct { Style* st; const Style* parent; Node* el; i32 root_fs; Arena* ar; const char* base; } Ctx;   /* base: URL da folha (url()) */

/* comprimento; aceita px em, rem, %, vw, vh, pt, calc()/min()/max()/clamp() */
static int parse_len_at(Ctx* c, const char* s, int n, int* i, Len* out);

static int parse_calc(Ctx* c, const char* s, int n, int* i, Len* out){
    /* soma de produtos: a [*|/ k] (+|- b ...) */
    Len acc = { 0, 0, L_VAL };
    int sign = 1;
    for(;;){
        while(*i < n && is_ws((u8)s[*i])) (*i)++;
        if(*i >= n || s[*i] == ')' || s[*i] == ',') break;
        Len t;
        if(s[*i] == '('){ (*i)++; if(!parse_calc(c, s, n, i, &t)) return 0; if(*i < n && s[*i] == ')') (*i)++; }
        else if(!parse_len_at(c, s, n, i, &t)) return 0;
        for(;;){
            while(*i < n && is_ws((u8)s[*i])) (*i)++;
            if(*i < n && (s[*i] == '*' || s[*i] == '/')){
                char op = s[(*i)++];
                while(*i < n && is_ws((u8)s[*i])) (*i)++;
                i32 k;
                int j = *i;
                if(!parse_num(s, n, &j, &k)) return 0;
                if(j < n && (s[j] == '%' || (s[j] >= 'a' && s[j] <= 'z'))){
                    /* 2 * 10px: o outro lado tem unidade */
                    Len u; if(!parse_len_at(c, s, n, i, &u)) return 0;
                    i32 m = t.px;                       /* t era numero puro (px = valor) */
                    t.px = (i32)((i64)u.px * m / 1); t.pct = (i32)((i64)u.pct * m / 1);
                    continue;
                }
                *i = j;
                if(op == '*'){ t.px = (i32)((i64)t.px * k / 1000); t.pct = (i32)((i64)t.pct * k / 1000); }
                else if(k){ t.px = (i32)((i64)t.px * 1000 / k); t.pct = (i32)((i64)t.pct * 1000 / k); }
            } else break;
        }
        acc.px += sign * t.px; acc.pct += sign * t.pct;
        while(*i < n && is_ws((u8)s[*i])) (*i)++;
        if(*i < n && s[*i] == '+'){ sign = 1; (*i)++; }
        else if(*i < n && s[*i] == '-' && !(*i + 1 < n && s[*i + 1] >= '0' && s[*i + 1] <= '9' && *i > 0 && !is_ws((u8)s[*i - 1]))){ sign = -1; (*i)++; }
        else break;
    }
    *out = acc;
    return 1;
}

static int parse_len_at(Ctx* c, const char* s, int n, int* i, Len* out){
    while(*i < n && is_ws((u8)s[*i])) (*i)++;
    if(*i >= n) return 0;
    const char* p = s + *i; int rem = n - *i;
    if(rem >= 4 && str_istarts(p, "auto")){ *i += 4; out->kind = L_AUTO; out->px = out->pct = 0; return 1; }
    if(rem >= 4 && str_istarts(p, "none")){ *i += 4; out->kind = L_NONE; out->px = out->pct = 0; return 1; }
    int fl = 0; while(fl < rem && ((p[fl] >= 'a' && p[fl] <= 'z') || p[fl] == '-')) fl++;
    if(fl < rem && p[fl] == '(' && fl){
        int is_calc = str_istarts(p, "calc(") || str_istarts(p, "-webkit-calc(");
        int is_min = str_istarts(p, "min("), is_max = str_istarts(p, "max("), is_clamp = str_istarts(p, "clamp(");
        if(!is_calc && !is_min && !is_max && !is_clamp) return 0;
        *i += fl + 1;
        Len args[3]; int na = 0;
        for(;;){
            Len a;
            if(!parse_calc(c, s, n, i, &a)) return 0;
            if(na < 3) args[na++] = a;
            while(*i < n && is_ws((u8)s[*i])) (*i)++;
            if(*i < n && s[*i] == ','){ (*i)++; continue; }
            break;
        }
        if(*i < n && s[*i] == ')') (*i)++;
        Len r = args[0];
        if(is_min || is_max){
            /* sem conhecer o bloco: compara so os px puros; mistura -> heuristica */
            for(int k = 1; k < na; k++){
                Len b = args[k];
                if(!r.pct && !b.pct){ if(is_min ? b.px < r.px : b.px > r.px) r = b; }
                else if(is_min && !b.pct) r = b;                /* min(100%, 600px) -> 600px */
                else if(is_max && b.pct && !r.pct) r = b;
            }
        } else if(is_clamp && na == 3){
            r = args[1];
            if(!r.pct && !args[0].pct && r.px < args[0].px) r = args[0];
            if(!r.pct && !args[2].pct && r.px > args[2].px) r = args[2];
        }
        r.kind = L_VAL;
        *out = r;
        return 1;
    }
    if(rem >= 4 && str_istarts(p, "var(")) return 0;
    i32 v;
    if(!parse_num(s, n, i, &v)) return 0;
    p = s + *i; rem = n - *i;
    i32 fs = c->st ? c->st->font_size : 16;
    out->kind = L_VAL; out->pct = 0;
    if(rem > 0 && p[0] == '%'){ out->px = 0; out->pct = v / 10; (*i)++; return 1; }
    int ul = 0; while(ul < rem && ((p[ul] >= 'a' && p[ul] <= 'z') || (p[ul] >= 'A' && p[ul] <= 'Z'))) ul++;
    i64 px;
    if(ul == 0) px = v / 1000;                              /* numero puro (0 ou fator) */
    else if(str_ieqn(p, "px", ul)) px = v / 1000;
    else if(str_ieqn(p, "em", ul)) px = (i64)v * fs / 1000;
    else if(str_ieqn(p, "rem", ul)) px = (i64)v * c->root_fs / 1000;
    else if(str_ieqn(p, "ex", ul) || str_ieqn(p, "ch", ul)) px = (i64)v * fs / 2000;
    else if(str_ieqn(p, "vw", ul) || str_ieqn(p, "dvw", ul) || str_ieqn(p, "svw", ul) || str_ieqn(p, "lvw", ul)) px = (i64)v * css_vw / 100000;
    else if(str_ieqn(p, "vh", ul) || str_ieqn(p, "dvh", ul) || str_ieqn(p, "svh", ul) || str_ieqn(p, "lvh", ul)) px = (i64)v * css_vh / 100000;
    else if(str_ieqn(p, "vmin", ul)) px = (i64)v * (css_vw < css_vh ? css_vw : css_vh) / 100000;
    else if(str_ieqn(p, "vmax", ul)) px = (i64)v * (css_vw > css_vh ? css_vw : css_vh) / 100000;
    else if(str_ieqn(p, "pt", ul)) px = (i64)v * 4 / 3000;
    else if(str_ieqn(p, "pc", ul)) px = (i64)v * 16 / 1000;
    else if(str_ieqn(p, "in", ul)) px = (i64)v * 96 / 1000;
    else if(str_ieqn(p, "cm", ul)) px = (i64)v * 96 / 2540;
    else if(str_ieqn(p, "mm", ul)) px = (i64)v * 96 / 25400;
    else if(str_ieqn(p, "fr", ul)) px = v / 1000;
    else return 0;
    if(ul == 0 && v != 0 && c->st){ /* numero sem unidade: so 0 e valido (exceto em fatores do calc) */ }
    out->px = (i32)px;
    *i += ul;
    return 1;
}

static int parse_len(Ctx* c, const char* s, int n, Len* out){
    int i = 0;
    if(!parse_len_at(c, s, n, &i, out)) return 0;
    while(i < n && is_ws((u8)s[i])) i++;
    return i == n;
}

/* divide um valor em tokens no nivel de cima (respeita parenteses) */
static int split_tokens(const char* s, int n, const char** tok, int* tl, int max){
    int k = 0, i = 0;
    while(i < n && k < max){
        while(i < n && (is_ws((u8)s[i]) || s[i] == ',')) i++;
        if(i >= n) break;
        int t0 = i, depth = 0; char q = 0;
        while(i < n){
            char ch = s[i];
            if(q){ if(ch == q) q = 0; }
            else if(ch == '"' || ch == '\'') q = ch;
            else if(ch == '(') depth++;
            else if(ch == ')') depth--;
            else if(depth <= 0 && (is_ws((u8)ch) || ch == ',')) break;
            i++;
        }
        tok[k] = s + t0; tl[k] = i - t0; k++;
    }
    return k;
}

/* ---- var(): substitui recursivamente ---- */
static const char* var_lookup(const Style* st, const char* name, int nl){
    u32 h = hash_str(name, nl) | 1;
    for(VarScope* sc = st->vars; sc; sc = sc->parent){
        for(int i = (int)(h & (u32)(sc->cap - 1)); sc->tab[i].hash; i = (i + 1) & (sc->cap - 1))
            if(sc->tab[i].hash == h && !strncmp(sc->tab[i].name, name, (size_t)nl) && !sc->tab[i].name[nl]) return sc->tab[i].value;
    }
    return NULL;
}
const char* css_var(const Style* st, const char* name, int n){ return st ? var_lookup(st, name, n) : NULL; }
static int expand_vars(const Style* st, const char* s, int n, Buf* out, int depth){
    if(depth > 8) return 0;
    for(int i = 0; i < n;){
        if(i + 4 <= n && str_istarts(s + i, "var(")){
            int j = i + 4, d = 1;
            while(j < n && d){ if(s[j] == '(') d++; else if(s[j] == ')') d--; j++; }
            int a0 = i + 4, a1 = j - 1;
            while(a0 < a1 && is_ws((u8)s[a0])) a0++;
            int ne = a0; while(ne < a1 && s[ne] != ',' && !is_ws((u8)s[ne])) ne++;
            const char* v = var_lookup(st, s + a0, ne - a0);
            if(v){ if(!expand_vars(st, v, (int)strlen(v), out, depth + 1)) return 0; }
            else {
                int cm = ne; while(cm < a1 && s[cm] != ',') cm++;
                if(cm >= a1) return 0;                         /* sem valor e sem fallback */
                if(!expand_vars(st, s + cm + 1, a1 - cm - 1, out, depth + 1)) return 0;
            }
            i = j;
            continue;
        }
        buf_putc(out, s[i++]);
    }
    return 1;
}

/* ---- fontes ---- */
static i32 font_kw(const char* s, int n, i32 parent){
    static const struct { const char* k; i32 v; } K[] = {
        {"xx-small", 9}, {"x-small", 10}, {"small", 13}, {"medium", 16}, {"large", 18},
        {"x-large", 24}, {"xx-large", 32}, {"xxx-large", 48} };
    for(size_t i = 0; i < sizeof K / sizeof K[0]; i++) if(str_ieqn(s, K[i].k, n)) return K[i].v;
    if(str_ieqn(s, "smaller", n)) return parent * 5 / 6;
    if(str_ieqn(s, "larger", n)) return parent * 6 / 5;
    return -1;
}
static void set_font_size(Ctx* c, const char* v, int n){
    i32 parent = c->parent ? c->parent->font_size : 16;
    i32 k = font_kw(v, n, parent);
    if(k > 0){ c->st->font_size = (i16)k; return; }
    if(str_ieqn(v, "inherit", n)){ c->st->font_size = (i16)parent; return; }
    Style tmp = *c->st; tmp.font_size = (i16)parent;           /* em relativo ao pai */
    Ctx c2 = *c; c2.st = &tmp;
    Len l;
    if(parse_len(&c2, v, n, &l) && l.kind == L_VAL){
        i32 px = l.px + l.pct * parent / 10000;
        if(px < 1) px = 1;
        if(px > 400) px = 400;
        c->st->font_size = (i16)px;
    }
}
static void set_weight(Style* st, const char* v, int n, const Style* parent){
    int pw = parent && parent->weight ? parent->weight : 400, w = st->weight ? st->weight : 400;
    if(str_ieqn(v, "bold", n)) w = 700;
    else if(str_ieqn(v, "normal", n)) w = 400;
    else if(str_ieqn(v, "bolder", n)) w = pw < 350 ? 400 : pw < 550 ? 700 : 900;
    else if(str_ieqn(v, "lighter", n)) w = pw < 550 ? 100 : pw < 750 ? 400 : 700;
    else if(str_ieqn(v, "inherit", n)) w = pw;
    else if(v[0] >= '1' && v[0] <= '9'){ w = atoi(v); if(w < 1) w = 1; if(w > 1000) w = 1000; }
    st->weight = (u16)w;
    st->bold = w >= 600;
}
/* familias que o "sistema" tem (como o fontconfig de um Linux com Noto e
 * Liberation); nomes desconhecidos sao pulados, como nos navegadores */
static int builtin_family(const char* f){
    static const struct { const char* n; u8 fam; } K[] = {
        {"sans-serif", FAM_SANS}, {"system-ui", FAM_SANS}, {"ui-sans-serif", FAM_SANS}, {"noto sans", FAM_SANS},
        {"dejavu sans", FAM_SANS}, {"ubuntu", FAM_SANS}, {"cantarell", FAM_SANS},
        {"serif", FAM_SERIF}, {"ui-serif", FAM_SERIF}, {"noto serif", FAM_SERIF}, {"dejavu serif", FAM_SERIF},
        {"monospace", FAM_MONO}, {"ui-monospace", FAM_MONO}, {"noto sans mono", FAM_MONO}, {"noto mono", FAM_MONO},
        {"dejavu sans mono", FAM_MONO}, {"courier", FAM_MONO}, {"courier new", FAM_MONO}, {"liberation mono", FAM_MONO},
        {"cousine", FAM_MONO}, {"ubuntu mono", FAM_MONO},
        {"arial", FAM_ARIAL}, {"helvetica", FAM_ARIAL}, {"helvetica neue", FAM_ARIAL}, {"liberation sans", FAM_ARIAL},
        {"arimo", FAM_ARIAL}, {"nimbus sans", FAM_ARIAL}, {"nimbus sans l", FAM_ARIAL},
        {"times", FAM_TIMES}, {"times new roman", FAM_TIMES}, {"liberation serif", FAM_TIMES}, {"tinos", FAM_TIMES},
        {"nimbus roman", FAM_TIMES},
    };
    for(size_t i = 0; i < sizeof K / sizeof K[0]; i++) if(!strcmp(f, K[i].n)) return K[i].fam;
    return -1;
}
static void set_family(Style* st, const char* v, int n){
    int fam = -1, fb = -1;
    int i = 0;
    while(i < n){
        while(i < n && (is_ws((u8)v[i]) || v[i] == ',')) i++;
        if(i >= n) break;
        char name[64]; int k = 0;
        if(v[i] == '"' || v[i] == '\''){
            char q = v[i++];
            while(i < n && v[i] != q){ if(k < 63) name[k++] = (char)lower((u8)v[i]); i++; }
            i++;
        } else {
            while(i < n && v[i] != ','){ if(k < 63) name[k++] = (char)lower((u8)v[i]); i++; }
            while(k && is_ws((u8)name[k - 1])) k--;
        }
        while(i < n && v[i] != ',') i++;
        name[k] = 0;
        if(!k) continue;
        int b = builtin_family(name);
        if(fam < 0){
            int w = b < 0 ? font_family_id(name) : -1;
            if(w >= 0) fam = w;
            else if(b >= 0){ fam = b; fb = b; break; }
        } else if(b >= 0){ fb = b; break; }
    }
    if(fam < 0) fam = FAM_SERIF;              /* padrao dos navegadores: serif */
    if(fb < 0) fb = fam < FAM_BUILTIN ? fam : FAM_SERIF;
    st->fam = (u8)fam; st->ffb = (u8)fb;
    st->mono = fb == FAM_MONO;
}
static void set_line_height(Ctx* c, const char* v, int n){
    if(str_ieqn(v, "normal", n)){ c->st->line_height = 0; return; }
    if(str_ieqn(v, "inherit", n)){ c->st->line_height = c->parent ? c->parent->line_height : 0; return; }
    int i = 0; i32 num;
    if(parse_num(v, n, &i, &num) && i == n){             /* fator: 1.5 */
        c->st->line_height = (i16)-(num / 10);               /* negativo = fator x100 */
        return;
    }
    Len l;
    if(parse_len(c, v, n, &l) && l.kind == L_VAL){
        i32 px = l.px + l.pct * c->st->font_size / 10000;
        c->st->line_height = (i16)(px < 1 ? 1 : px);
    }
}
static void set_font(Ctx* c, const char* v, int n){
    const char* tok[16]; int tl[16];
    int k = split_tokens(v, n, tok, tl, 16);
    if(k == 1 && (str_ieqn(tok[0], "inherit", tl[0]) || str_ieqn(tok[0], "initial", tl[0]))) return;
    c->st->bold = 0; c->st->weight = 400; c->st->italic = 0; c->st->line_height = 0;
    for(int i = 0; i < k; i++){
        const char* t = tok[i]; int l = tl[i];
        if(str_ieqn(t, "italic", l) || str_ieqn(t, "oblique", l)) c->st->italic = 1;
        else if(str_ieqn(t, "bold", l) || str_ieqn(t, "bolder", l) || (t[0] >= '1' && t[0] <= '9' && l == 3 && t[1] == '0' && t[2] == '0')) set_weight(c->st, t, l, c->parent);
        else if(str_ieqn(t, "normal", l) || str_ieqn(t, "small-caps", l)) {}
        else if((t[0] >= '0' && t[0] <= '9') || t[0] == '.' || font_kw(t, l, 16) > 0 || str_istarts(t, "calc") || str_istarts(t, "var")){
            const char* sl = memchr(t, '/', (size_t)l);
            set_font_size(c, t, sl ? (int)(sl - t) : l);
            if(sl) set_line_height(c, sl + 1, l - (int)(sl - t) - 1);
            else if(i + 1 < k && tok[i + 1][0] == '/'){
                if(tl[i + 1] > 1) set_line_height(c, tok[i + 1] + 1, tl[i + 1] - 1);
                else if(i + 2 < k){ set_line_height(c, tok[i + 2], tl[i + 2]); i++; }
                i++;
            }
            /* o resto e a familia */
            if(i + 1 < k) set_family(c->st, tok[i + 1], (int)(v + n - tok[i + 1]));
            break;
        }
    }
}

/* ---- bordas ---- */
static int border_width_kw(Ctx* c, const char* t, int l, i32* w){
    if(str_ieqn(t, "thin", l)){ *w = 1; return 1; }
    if(str_ieqn(t, "medium", l)){ *w = 3; return 1; }
    if(str_ieqn(t, "thick", l)){ *w = 5; return 1; }
    Len ln;
    if(parse_len(c, t, l, &ln) && ln.kind == L_VAL && !ln.pct){ *w = ln.px; return 1; }
    return 0;
}
static int is_border_style(const char* t, int l){
    static const char* S[] = {"none","hidden","dotted","dashed","solid","double","groove","ridge","inset","outset",NULL};
    for(int i = 0; S[i]; i++) if(str_ieqn(t, S[i], l)) return 1 + (i < 2);       /* 2 = sem borda */
    return 0;
}
/* "1px solid red" num lado (mask: bits dos lados) */
static void set_border(Ctx* c, const char* v, int n, int mask){
    const char* tok[8]; int tl[8];
    int k = split_tokens(v, n, tok, tl, 8);
    i32 w = 3; u32 col = CUR_COLOR; int style = 0;
    for(int i = 0; i < k; i++){
        int bs = is_border_style(tok[i], tl[i]);
        if(bs){ style = bs; continue; }
        i32 ww; if(border_width_kw(c, tok[i], tl[i], &ww)){ w = ww; continue; }
        int ok; u32 cc = css_parse_color(tok[i], tl[i], &ok);
        if(ok) col = cc;
        else if(str_ieqn(tok[i], "currentcolor", tl[i])) col = CUR_COLOR;
    }
    if(style != 1) w = 0;               /* sem estilo (ou none) = sem borda */
    for(int s = 0; s < 4; s++) if(mask & (1 << s)){ c->st->border_w[s] = (i16)w; c->st->border_c[s] = col; }
}

/* 1 a 4 valores -> top right bottom left */
static void set_box4(Ctx* c, const char* v, int n, Len* out){
    const char* tok[4]; int tl[4];
    int k = split_tokens(v, n, tok, tl, 4);
    Len l[4];
    for(int i = 0; i < k; i++) if(!parse_len(c, tok[i], tl[i], &l[i])) return;
    if(k == 1){ out[0] = out[1] = out[2] = out[3] = l[0]; }
    else if(k == 2){ out[0] = out[2] = l[0]; out[1] = out[3] = l[1]; }
    else if(k == 3){ out[0] = l[0]; out[1] = out[3] = l[1]; out[2] = l[2]; }
    else if(k == 4){ out[0] = l[0]; out[1] = l[1]; out[2] = l[2]; out[3] = l[3]; }
}

static int display_of(const char* v, int n){
    static const struct { const char* k; int d; } D[] = {
        {"none", D_NONE}, {"inline", D_INLINE}, {"block", D_BLOCK}, {"inline-block", D_INLINE_BLOCK},
        {"list-item", D_LIST_ITEM}, {"flex", D_FLEX}, {"inline-flex", D_INLINE_FLEX}, {"grid", D_GRID},
        {"inline-grid", D_INLINE_GRID}, {"table", D_TABLE}, {"inline-table", D_INLINE_TABLE},
        {"table-row-group", D_ROW_GROUP}, {"table-header-group", D_ROW_GROUP}, {"table-footer-group", D_ROW_GROUP},
        {"table-row", D_ROW}, {"table-cell", D_CELL}, {"table-caption", D_CAPTION}, {"contents", D_CONTENTS},
        {"flow-root", D_BLOCK}, {"table-column", D_NONE}, {"table-column-group", D_NONE},
        {"-webkit-box", D_FLEX}, {"-webkit-flex", D_FLEX}, {"-ms-flexbox", D_FLEX}, {"run-in", D_BLOCK},
        {"block flex", D_FLEX}, {"inline flex", D_INLINE_FLEX}, {"block flow", D_BLOCK}, {"ruby", D_INLINE} };
    for(size_t i = 0; i < sizeof D / sizeof D[0]; i++) if(str_ieqn(v, D[i].k, n)) return D[i].d;
    return -1;
}

/* gradiente: usa a primeira cor visivel como fundo liso */
/* ---- camadas de fundo/mascara: url, tamanho, posicao, repeticao ---- */
/* url(...) -> URL absoluta (relativa a folha de estilo, nao a pagina) */
static const char* css_url(Ctx* c, const char* v){
    const char* u = strstr(v, "url(");
    if(!u) return NULL;
    u += 4;
    while(*u == ' ') u++;
    char q = (*u == '"' || *u == '\'') ? *u++ : 0;
    const char* e = u;
    while(*e && (q ? *e != q : *e != ')')) e++;
    while(!q && e > u && e[-1] == ' ') e--;
    if(e - u >= 5 && str_istarts(u, "data:")) return arena_strndup(c->ar, u, (size_t)(e - u));
    char rel[URLMAX], abs_[URLMAX];
    if(e - u >= URLMAX) return NULL;
    memcpy(rel, u, (size_t)(e - u)); rel[e - u] = 0;
    const char* base = c->base ? c->base : (doc ? doc->url : "");
    if(!url_resolve(base, rel, abs_, sizeof abs_)) return NULL;
    return arena_strdup(c->ar, abs_);
}
static int pos_kw(const char* t, int l, Len* out, int* axis){
    Len p = { 0, 0, L_VAL };
    if(str_ieqn(t, "left", l)){ *axis = 1; }
    else if(str_ieqn(t, "right", l)){ p.pct = 10000; *axis = 1; }
    else if(str_ieqn(t, "top", l)){ *axis = 2; }
    else if(str_ieqn(t, "bottom", l)){ p.pct = 10000; *axis = 2; }
    else if(str_ieqn(t, "center", l)){ p.pct = 5000; *axis = 0; }
    else return 0;
    *out = p; return 1;
}
/* posicao: "center", "right 10px"... ; tok ja separados */
static void set_pos(Ctx* c, const char** tok, int* tl, int k, Len* px, Len* py){
    Len v[2]; int ax[2]; int nv = 0;
    for(int i = 0; i < k && nv < 2; i++){
        int a = 0; Len l;
        if(pos_kw(tok[i], tl[i], &l, &a)){
            /* "right 10px": deslocamento a partir daquela borda */
            Len off;
            if(k >= 3 && a && i + 1 < k && parse_len(c, tok[i + 1], tl[i + 1], &off) && off.kind == L_VAL){
                if(l.pct){ l.px -= off.px; l.pct -= off.pct; } else { l.px += off.px; l.pct += off.pct; }
                i++;
            }
            v[nv] = l; ax[nv++] = a;
        }
        else if(parse_len(c, tok[i], tl[i], &l) && l.kind == L_VAL){ v[nv] = l; ax[nv++] = 0; }
    }
    Len half = { 0, 5000, L_VAL };
    if(nv == 0) return;
    if(nv == 1){
        if(ax[0] == 2){ *py = v[0]; *px = half; } else { *px = v[0]; *py = half; }
        return;
    }
    if(ax[0] == 2 || ax[1] == 1){ *py = v[0]; *px = v[1]; } else { *px = v[0]; *py = v[1]; }
}
static void set_size(Ctx* c, const char** tok, int* tl, int k, u8* kind, Len* w, Len* h){
    if(!k) return;
    if(str_ieqn(tok[0], "cover", tl[0])){ *kind = 1; return; }
    if(str_ieqn(tok[0], "contain", tl[0])){ *kind = 2; return; }
    Len a = { 0, 0, L_AUTO }, b = { 0, 0, L_AUTO };
    if(!str_ieqn(tok[0], "auto", tl[0])) parse_len(c, tok[0], tl[0], &a);
    if(k > 1 && !str_ieqn(tok[1], "auto", tl[1])) parse_len(c, tok[1], tl[1], &b);
    *kind = a.kind == L_VAL || b.kind == L_VAL ? 3 : 0;
    *w = a; *h = b;
}
static int set_repeat(const char* t, int l, u8* rep){
    if(str_ieqn(t, "no-repeat", l)){ *rep = 0; return 1; }
    if(str_ieqn(t, "repeat-x", l)){ *rep = 1; return 1; }
    if(str_ieqn(t, "repeat-y", l)){ *rep = 2; return 1; }
    if(str_ieqn(t, "repeat", l) || str_ieqn(t, "space", l) || str_ieqn(t, "round", l)){ *rep = 3; return 1; }
    return 0;
}
/* atalho background/mask: cor, url, repeticao, posicao [/ tamanho] */
static void set_layer(Ctx* c, const char* v, int n, int mask){
    Style* st = c->st;
    const char* tok[16]; int tl[16];
    int k = split_tokens(v, n, tok, tl, 16);
    const char* pt[4]; int pl[4]; int np = 0;
    const char* zt[2]; int zl[2]; int nz = 0; int after_slash = 0;
    u8* rep = mask ? &st->mask_rep : &st->bg_rep;
    for(int i = 0; i < k; i++){
        const char* t = tok[i]; int l = tl[i];
        if(str_istarts(t, "url(")){ const char* u = css_url(c, t); if(mask) st->mask_url = u; else st->bg_url = u; continue; }
        if(set_repeat(t, l, rep)) continue;
        const char* sl = memchr(t, '/', (size_t)l);
        if(sl && !str_istarts(t, "url(")){
            if(sl > t && np < 4){ pt[np] = t; pl[np++] = (int)(sl - t); }
            after_slash = 1;
            if(sl + 1 < t + l && nz < 2){ zt[nz] = sl + 1; zl[nz++] = (int)(t + l - sl - 1); }
            continue;
        }
        int a; Len tmp;
        int is_pos = pos_kw(t, l, &tmp, &a) || (t[0] >= '0' && t[0] <= '9') || t[0] == '-' || t[0] == '.' || str_istarts(t, "calc(");
        if(after_slash){ if(nz < 2 && (is_pos || str_ieqn(t, "auto", l) || str_ieqn(t, "cover", l) || str_ieqn(t, "contain", l))){ zt[nz] = t; zl[nz++] = l; } continue; }
        if(is_pos && np < 4){ pt[np] = t; pl[np++] = l; }
    }
    if(np) set_pos(c, pt, pl, np, mask ? &st->mask_px : &st->bg_px, mask ? &st->mask_py : &st->bg_py);
    if(nz) set_size(c, zt, zl, nz, mask ? &st->mask_szk : &st->bg_szk, mask ? &st->mask_sw : &st->bg_sw, mask ? &st->mask_sh : &st->bg_sh);
}


static int is_global_kw(const char* v, int n){
    return str_ieqn(v, "inherit", n) || str_ieqn(v, "initial", n) || str_ieqn(v, "unset", n) || str_ieqn(v, "revert", n) || str_ieqn(v, "revert-layer", n);
}

static void apply_decl(Ctx* c, int prop, const char* v, int n);

static const Style* initial_style(void);

/* content: de ::before/::after -> texto (NULL = none/normal: sem caixa) */
static const char* parse_content(Ctx* c, const char* v, int n){
    if(str_ieqn(v, "none", n) || str_ieqn(v, "normal", n)) return NULL;
    Buf b = {0};
    int i = 0;
    while(i < n){
        char ch = v[i];
        if(is_ws((u8)ch)){ i++; continue; }
        if(ch == '/') break;                                  /* texto alternativo */
        if(ch == '"' || ch == '\''){
            i++;
            while(i < n && v[i] != ch){
                if(v[i] == '\\' && i + 1 < n){
                    i++;
                    u32 cp = 0; int hn = 0;
                    while(hn < 6 && i < n){
                        int h = lower((u8)v[i]);
                        if(h >= '0' && h <= '9') cp = cp * 16 + (u32)(h - '0');
                        else if(h >= 'a' && h <= 'f') cp = cp * 16 + (u32)(h - 'a' + 10);
                        else break;
                        hn++; i++;
                    }
                    if(hn){
                        if(i < n && v[i] == ' ') i++;
                        if(cp == 0xA) cp = ' ';
                        /* area de uso privado: icones de fontes que nao temos */
                        if(cp && !(cp >= 0xE000 && cp <= 0xF8FF) && cp < 0xF0000){ char u[4]; int k = utf8_encode(cp, u); buf_put(&b, u, k); }
                    } else if(v[i] != '\n'){ buf_put(&b, v + i, 1); i++; }
                    continue;
                }
                buf_put(&b, v + i, 1); i++;
            }
            i++;
            continue;
        }
        int j = i; while(j < n && !is_ws((u8)v[j]) && v[j] != '(') j++;
        int w = j - i;
        if(j < n && v[j] == '('){
            int depth = 0, k = j;
            for(; k < n; k++){ if(v[k] == '(') depth++; else if(v[k] == ')' && --depth == 0){ k++; break; } }
            if(w == 4 && str_istarts(v + i, "attr")){
                char name[64]; int a0 = j + 1, a1 = a0;
                while(a0 < k && is_ws((u8)v[a0])) a0++;
                a1 = a0; while(a1 < k && v[a1] != ')' && !is_ws((u8)v[a1]) && v[a1] != ',') a1++;
                int ln = a1 - a0 < 63 ? a1 - a0 : 63;
                for(int q = 0; q < ln; q++) name[q] = (char)lower((u8)v[a0 + q]);
                name[ln] = 0;
                const char* av = c->el ? dom_attr(c->el, name) : NULL;
                if(av) buf_puts(&b, av);
            }
            /* counter(), url(), gradientes...: a caixa existe, sem texto */
            i = k;
            continue;
        }
        if(str_ieqn(v + i, "open-quote", w)) buf_puts(&b, "\xE2\x80\x9C");
        else if(str_ieqn(v + i, "close-quote", w)) buf_puts(&b, "\xE2\x80\x9D");
        i = j;
    }
    const char* r = arena_strndup(c->ar, b.p ? b.p : "", (size_t)b.n);
    buf_free(&b);
    return r;
}

/* aplica uma propriedade (valor ja sem var()) */
static void apply_decl(Ctx* c, int prop, const char* v, int n){
    Style* st = c->st;
    const Style* par = c->parent ? c->parent : initial_style();
    if(n <= 0) return;
    if(prop == P_ALL){
        if(!is_global_kw(v, n)) return;
        /* all: unset/initial: volta ao inicial (herdadas copiam o pai em unset) */
        const Style* ini = initial_style();
        Style keep = *st;
        *st = *ini;
        st->vars = keep.vars;
        if(!str_ieqn(v, "initial", n)){
            st->color = par->color; st->font_size = par->font_size; st->bold = par->bold; st->weight = par->weight; st->italic = par->italic;
            st->mono = par->mono; st->fam = par->fam; st->ffb = par->ffb; st->text_align = par->text_align; st->white_space = par->white_space;
            st->line_height = par->line_height; st->visibility = par->visibility; st->list_style = par->list_style;
            st->letter_spacing = par->letter_spacing; st->cursor_pointer = par->cursor_pointer;
            st->upper = par->upper; st->lower = par->lower; st->capitalize = par->capitalize;
        }
        return;
    }
    if(is_global_kw(v, n)){
        int inh = str_ieqn(v, "inherit", n);
        const Style* src = inh ? par : initial_style();
        switch(prop){
        case P_COLOR: st->color = src->color; break;
        case P_BACKGROUND: case P_BACKGROUND_COLOR: st->bg = inh ? par->bg : 0; break;
        case P_DISPLAY: if(!inh) st->display = D_INLINE; else st->display = par->display; break;
        case P_FONT_WEIGHT: st->bold = src->bold; st->weight = src->weight; break;
        case P_FONT_STYLE: st->italic = src->italic; break;
        case P_TEXT_ALIGN: st->text_align = src->text_align; break;
        case P_WHITE_SPACE: st->white_space = src->white_space; break;
        case P_VISIBILITY: st->visibility = src->visibility; break;
        case P_WIDTH: st->width = src->width; break;
        case P_HEIGHT: st->height = src->height; break;
        case P_MARGIN: for(int k = 0; k < 4; k++) st->margin[k] = src->margin[k]; break;
        case P_PADDING: for(int k = 0; k < 4; k++) st->padding[k] = src->padding[k]; break;
        case P_TEXT_DECORATION: case P_TEXT_DECORATION_LINE: st->underline = src->underline; st->strike = src->strike; break;
        case P_BORDER: for(int k = 0; k < 4; k++){ st->border_w[k] = src->border_w[k]; st->border_c[k] = src->border_c[k]; } break;
        case P_POSITION: st->position = src->position; break;
        case P_FLOAT: st->floating = src->floating; break;
        case P_LINE_HEIGHT: st->line_height = src->line_height; break;
        case P_FONT_FAMILY: st->mono = src->mono; st->fam = src->fam; st->ffb = src->ffb; break;
        default: break;
        }
        return;
    }
    const char* tok[8]; int tl[8]; int k; int ok; u32 col; Len l;
    switch(prop){
    case P_DISPLAY: {
        int d = display_of(v, n);
        if(d >= 0){ st->display = (u8)d; st->flow_root = (u8)((st->flow_root & ~1) | (str_ieqn(v, "flow-root", n) ? 1 : 0)); }
        break;
    }
    case P_CLEAR:
        st->clear = str_ieqn(v, "left", n) || str_ieqn(v, "inline-start", n) ? 1 : str_ieqn(v, "right", n) || str_ieqn(v, "inline-end", n) ? 2
                  : str_ieqn(v, "both", n) ? 3 : 0;
        break;
    case P_CONTENT: st->content = parse_content(c, v, n); break;
    case P_POSITION:
        if(str_ieqn(v, "static", n)) st->position = POS_STATIC;
        else if(str_ieqn(v, "relative", n)) st->position = POS_RELATIVE;
        else if(str_ieqn(v, "absolute", n)) st->position = POS_ABSOLUTE;
        else if(str_ieqn(v, "fixed", n)) st->position = POS_FIXED;
        else if(str_ieqn(v, "sticky", n) || str_ieqn(v, "-webkit-sticky", n)) st->position = POS_STICKY;
        break;
    case P_FLOAT:
        st->floating = str_ieqn(v, "left", n) || str_ieqn(v, "inline-start", n) ? 1 : str_ieqn(v, "right", n) || str_ieqn(v, "inline-end", n) ? 2 : 0;
        break;
    case P_COLOR:
        col = css_parse_color(v, n, &ok);
        if(ok) st->color = col;
        else if(str_ieqn(v, "currentcolor", n)) st->color = par->color;
        break;
    case P_BACKGROUND_COLOR:
        col = css_parse_color(v, n, &ok);
        if(ok) st->bg = col; else if(str_ieqn(v, "currentcolor", n)) st->bg = st->color;
        break;
    case P_BACKGROUND:
        /* o atalho redefine tudo: cor transparente e sem imagem, salvo o que vier */
        st->bg = 0; st->bg_url = NULL; st->bg_grad = NULL;
        st->bg_rep = 3; st->bg_szk = 0; st->bg_px = len_px(0); st->bg_py = len_px(0);
        if(str_ieqn(v, "none", n)) break;
        set_layer(c, v, n, 0);
        k = split_tokens(v, n, tok, tl, 8);
        for(int i = 0; i < k; i++){
            col = css_parse_color(tok[i], tl[i], &ok);
            if(ok) st->bg = col;
            else if(str_istarts(tok[i], "url(")) {}
            else if((str_istarts(tok[i], "linear-gradient") || str_istarts(tok[i], "radial-gradient") || str_istarts(tok[i], "repeating-")) && !st->bg_grad)
                st->bg_grad = arena_strndup(c->ar, tok[i], (size_t)tl[i]);
        }
        break;
    case P_BACKGROUND_IMAGE:
        st->bg_url = NULL; st->bg_grad = NULL;
        if(str_istarts(v, "url(")) st->bg_url = css_url(c, v);
        else if(str_istarts(v, "linear-gradient") || str_istarts(v, "radial-gradient") || str_istarts(v, "repeating-")){
            k = split_tokens(v, n, tok, tl, 8);
            st->bg_grad = arena_strndup(c->ar, tok[0], (size_t)tl[0]);
        }
        break;
    case P_FONT_SIZE: break;                              /* aplicado antes */
    case P_FONT_WEIGHT: set_weight(st, v, n, par); break;
    case P_FONT_STYLE: st->italic = str_ieqn(v, "italic", n) || str_ieqn(v, "oblique", n); break;
    case P_FONT_FAMILY: set_family(st, v, n); break;
    case P_FONT: set_font(c, v, n); break;
    case P_LINE_HEIGHT: set_line_height(c, v, n); break;
    case P_TEXT_ALIGN:
        if(str_ieqn(v, "center", n) || str_ieqn(v, "-webkit-center", n) || str_ieqn(v, "-moz-center", n)) st->text_align = TA_CENTER;
        else if(str_ieqn(v, "right", n) || str_ieqn(v, "end", n) || str_ieqn(v, "-webkit-right", n)) st->text_align = TA_RIGHT;
        else if(str_ieqn(v, "justify", n)) st->text_align = TA_JUSTIFY;
        else st->text_align = TA_LEFT;
        break;
    case P_TEXT_DECORATION: case P_TEXT_DECORATION_LINE:
        st->underline = str_icontains(v, "underline") != NULL;
        st->strike = str_icontains(v, "line-through") != NULL;
        break;
    case P_TEXT_TRANSFORM:
        st->upper = str_ieqn(v, "uppercase", n); st->lower = str_ieqn(v, "lowercase", n); st->capitalize = str_ieqn(v, "capitalize", n);
        break;
    case P_TEXT_INDENT: if(parse_len(c, v, n, &l) && l.kind == L_VAL) st->indent = (i16)l.px; break;
    case P_LETTER_SPACING: if(parse_len(c, v, n, &l) && l.kind == L_VAL) st->letter_spacing = (i16)l.px; else st->letter_spacing = 0; break;
    case P_TEXT_OVERFLOW: st->ellipsis = str_ieqn(v, "ellipsis", n); break;
    case P_WHITE_SPACE:
        if(str_ieqn(v, "nowrap", n)) st->white_space = WS_NOWRAP;
        else if(str_ieqn(v, "pre", n)) st->white_space = WS_PRE;
        else if(str_ieqn(v, "pre-wrap", n) || str_ieqn(v, "break-spaces", n)) st->white_space = WS_PRE_WRAP;
        else if(str_ieqn(v, "pre-line", n)) st->white_space = WS_PRE_LINE;
        else st->white_space = WS_NORMAL;
        break;
    case P_WORD_BREAK: case P_OVERFLOW_WRAP: case P_WORD_WRAP:
        st->break_words = str_ieqn(v, "break-all", n) || str_ieqn(v, "break-word", n) || str_ieqn(v, "anywhere", n);
        break;
    case P_VISIBILITY: st->visibility = !(str_ieqn(v, "hidden", n) || str_ieqn(v, "collapse", n)); break;
    case P_LIST_STYLE: case P_LIST_STYLE_TYPE:
        k = split_tokens(v, n, tok, tl, 4);
        for(int i = 0; i < k; i++){
            const char* t = tok[i]; int tn = tl[i];
            if(str_ieqn(t, "none", tn)) st->list_style = LS_NONE;
            else if(str_ieqn(t, "disc", tn)) st->list_style = LS_DISC;
            else if(str_ieqn(t, "circle", tn)) st->list_style = LS_CIRCLE;
            else if(str_ieqn(t, "square", tn)) st->list_style = LS_SQUARE;
            else if(str_ieqn(t, "decimal", tn) || str_ieqn(t, "decimal-leading-zero", tn)) st->list_style = LS_DECIMAL;
            else if(str_istarts(t, "lower-alpha") || str_istarts(t, "upper-alpha") || str_istarts(t, "lower-latin") || str_istarts(t, "upper-latin")) st->list_style = LS_ALPHA;
            else if(str_istarts(t, "lower-roman") || str_istarts(t, "upper-roman")) st->list_style = LS_ROMAN;
        }
        break;
    case P_MARGIN: set_box4(c, v, n, st->margin); break;
    case P_MARGIN_TOP: case P_MARGIN_BLOCK_START: if(parse_len(c, v, n, &l)) st->margin[0] = l; break;
    case P_MARGIN_RIGHT: case P_MARGIN_INLINE_END: if(parse_len(c, v, n, &l)) st->margin[1] = l; break;
    case P_MARGIN_BOTTOM: case P_MARGIN_BLOCK_END: if(parse_len(c, v, n, &l)) st->margin[2] = l; break;
    case P_MARGIN_LEFT: case P_MARGIN_INLINE_START: if(parse_len(c, v, n, &l)) st->margin[3] = l; break;
    case P_MARGIN_INLINE: { Len b[4] = {st->margin[0], st->margin[1], st->margin[0], st->margin[1]}; Len t4[4]; memcpy(t4, b, sizeof t4);
        k = split_tokens(v, n, tok, tl, 2);
        if(k >= 1 && parse_len(c, tok[0], tl[0], &l)){ st->margin[3] = l; st->margin[1] = l; }
        if(k == 2 && parse_len(c, tok[1], tl[1], &l)) st->margin[1] = l;
        break; }
    case P_MARGIN_BLOCK:
        k = split_tokens(v, n, tok, tl, 2);
        if(k >= 1 && parse_len(c, tok[0], tl[0], &l)){ st->margin[0] = l; st->margin[2] = l; }
        if(k == 2 && parse_len(c, tok[1], tl[1], &l)) st->margin[2] = l;
        break;
    case P_PADDING: set_box4(c, v, n, st->padding); break;
    case P_PADDING_TOP: case P_PADDING_BLOCK_START: if(parse_len(c, v, n, &l)) st->padding[0] = l; break;
    case P_PADDING_RIGHT: case P_PADDING_INLINE_END: if(parse_len(c, v, n, &l)) st->padding[1] = l; break;
    case P_PADDING_BOTTOM: case P_PADDING_BLOCK_END: if(parse_len(c, v, n, &l)) st->padding[2] = l; break;
    case P_PADDING_LEFT: case P_PADDING_INLINE_START: if(parse_len(c, v, n, &l)) st->padding[3] = l; break;
    case P_PADDING_INLINE:
        k = split_tokens(v, n, tok, tl, 2);
        if(k >= 1 && parse_len(c, tok[0], tl[0], &l)){ st->padding[3] = l; st->padding[1] = l; }
        if(k == 2 && parse_len(c, tok[1], tl[1], &l)) st->padding[1] = l;
        break;
    case P_PADDING_BLOCK:
        k = split_tokens(v, n, tok, tl, 2);
        if(k >= 1 && parse_len(c, tok[0], tl[0], &l)){ st->padding[0] = l; st->padding[2] = l; }
        if(k == 2 && parse_len(c, tok[1], tl[1], &l)) st->padding[2] = l;
        break;
    case P_BORDER: set_border(c, v, n, 15); break;
    case P_BORDER_TOP: set_border(c, v, n, 1); break;
    case P_BORDER_RIGHT: set_border(c, v, n, 2); break;
    case P_BORDER_BOTTOM: set_border(c, v, n, 4); break;
    case P_BORDER_LEFT: set_border(c, v, n, 8); break;
    case P_BORDER_WIDTH: {
        k = split_tokens(v, n, tok, tl, 4); i32 w[4];
        for(int i = 0; i < k; i++) if(!border_width_kw(c, tok[i], tl[i], &w[i])) return;
        if(k == 1){ w[1] = w[2] = w[3] = w[0]; } else if(k == 2){ w[2] = w[0]; w[3] = w[1]; } else if(k == 3){ w[3] = w[1]; }
        if(k) for(int s = 0; s < 4; s++) if(st->border_w[s] || st->border_c[s]) st->border_w[s] = (i16)w[s];
        break; }
    case P_BORDER_TOP_WIDTH: case P_BORDER_RIGHT_WIDTH: case P_BORDER_BOTTOM_WIDTH: case P_BORDER_LEFT_WIDTH: {
        int s = prop == P_BORDER_TOP_WIDTH ? 0 : prop == P_BORDER_RIGHT_WIDTH ? 1 : prop == P_BORDER_BOTTOM_WIDTH ? 2 : 3;
        i32 w; if(border_width_kw(c, v, n, &w) && (st->border_w[s] || st->border_c[s])) st->border_w[s] = (i16)w;
        break; }
    case P_BORDER_COLOR: {
        k = split_tokens(v, n, tok, tl, 4); u32 cc[4];
        for(int i = 0; i < k; i++){ cc[i] = css_parse_color(tok[i], tl[i], &ok); if(!ok) return; }
        if(k == 1){ cc[1] = cc[2] = cc[3] = cc[0]; } else if(k == 2){ cc[2] = cc[0]; cc[3] = cc[1]; } else if(k == 3){ cc[3] = cc[1]; }
        if(k) for(int s = 0; s < 4; s++) st->border_c[s] = cc[s];
        break; }
    case P_BORDER_TOP_COLOR: case P_BORDER_RIGHT_COLOR: case P_BORDER_BOTTOM_COLOR: case P_BORDER_LEFT_COLOR: {
        int s = prop == P_BORDER_TOP_COLOR ? 0 : prop == P_BORDER_RIGHT_COLOR ? 1 : prop == P_BORDER_BOTTOM_COLOR ? 2 : 3;
        col = css_parse_color(v, n, &ok); if(ok) st->border_c[s] = col;
        break; }
    case P_BORDER_STYLE: {
        k = split_tokens(v, n, tok, tl, 4); int bs[4];
        for(int i = 0; i < k; i++) bs[i] = is_border_style(tok[i], tl[i]);
        if(k == 1){ bs[1] = bs[2] = bs[3] = bs[0]; } else if(k == 2){ bs[2] = bs[0]; bs[3] = bs[1]; } else if(k == 3){ bs[3] = bs[1]; }
        if(k) for(int s = 0; s < 4; s++){
            if(bs[s] == 2) st->border_w[s] = 0;
            else if(bs[s] == 1 && !st->border_w[s]) st->border_w[s] = 3;
        }
        break; }
    case P_BORDER_TOP_STYLE: case P_BORDER_RIGHT_STYLE: case P_BORDER_BOTTOM_STYLE: case P_BORDER_LEFT_STYLE: {
        int s = prop == P_BORDER_TOP_STYLE ? 0 : prop == P_BORDER_RIGHT_STYLE ? 1 : prop == P_BORDER_BOTTOM_STYLE ? 2 : 3;
        int bs = is_border_style(v, n);
        if(bs == 2) st->border_w[s] = 0;
        else if(bs == 1 && !st->border_w[s]) st->border_w[s] = 3;
        break; }
    case P_BORDER_RADIUS:
        if(parse_len_at(c, v, n, &(int){0}, &l) && l.kind == L_VAL) st->radius = (i16)(l.pct ? 999 : l.px);
        break;
    case P_BORDER_SPACING: if(parse_len_at(c, v, n, &(int){0}, &l) && l.kind == L_VAL) st->gap_col = l; break;
    case P_WIDTH: if(parse_len(c, v, n, &l)) st->width = l; break;
    case P_HEIGHT: if(parse_len(c, v, n, &l)) st->height = l; break;
    case P_MIN_WIDTH: if(parse_len(c, v, n, &l)) st->min_w = l; break;
    case P_MAX_WIDTH: if(parse_len(c, v, n, &l)) st->max_w = l; break;
    case P_MIN_HEIGHT: if(parse_len(c, v, n, &l)) st->min_h = l; break;
    case P_MAX_HEIGHT: if(parse_len(c, v, n, &l)) st->max_h = l; break;
    case P_TOP: if(parse_len(c, v, n, &l)) st->inset[0] = l; break;
    case P_RIGHT: if(parse_len(c, v, n, &l)) st->inset[1] = l; break;
    case P_BOTTOM: if(parse_len(c, v, n, &l)) st->inset[2] = l; break;
    case P_LEFT: if(parse_len(c, v, n, &l)) st->inset[3] = l; break;
    case P_INSET: set_box4(c, v, n, st->inset); break;
    case P_OVERFLOW: case P_OVERFLOW_X: case P_OVERFLOW_Y:
        if(prop != P_OVERFLOW_X) st->overflow_hidden = str_istarts(v, "hidden") || str_istarts(v, "clip");
        if(str_istarts(v, "hidden") || str_istarts(v, "clip")) st->overflow_hidden = 1;
        /* hidden/auto/scroll criam um contexto proprio para floats (clip nao) */
        if(str_istarts(v, "hidden") || str_istarts(v, "auto") || str_istarts(v, "scroll") || str_istarts(v, "overlay")) st->flow_root |= 2;
        else if(prop == P_OVERFLOW) st->flow_root &= (u8)~2;
        break;
    case P_BOX_SIZING: st->box_border = str_ieqn(v, "border-box", n); break;
    case P_FLEX_DIRECTION: st->flex_dir_col = str_istarts(v, "column"); break;
    case P_FLEX_WRAP: st->flex_wrap = str_ieqn(v, "wrap", n) || str_ieqn(v, "wrap-reverse", n); break;
    case P_FLEX_FLOW:
        k = split_tokens(v, n, tok, tl, 2);
        for(int i = 0; i < k; i++){
            if(str_istarts(tok[i], "column")) st->flex_dir_col = 1;
            else if(str_istarts(tok[i], "row")) st->flex_dir_col = 0;
            else if(str_istarts(tok[i], "wrap")) st->flex_wrap = 1;
            else if(str_ieqn(tok[i], "nowrap", tl[i])) st->flex_wrap = 0;
        }
        break;
    case P_FLEX:
        if(str_ieqn(v, "none", n)){ st->flex_grow = 0; st->flex_shrink = 0; st->flex_basis.kind = L_AUTO; break; }
        if(str_ieqn(v, "auto", n)){ st->flex_grow = 100; st->flex_shrink = 100; st->flex_basis.kind = L_AUTO; break; }
        k = split_tokens(v, n, tok, tl, 3);
        {
            int nums = 0;
            st->flex_basis = len_px(0); st->flex_shrink = 100;
            for(int i = 0; i < k; i++){
                int j = 0; i32 num;
                if(parse_num(tok[i], tl[i], &j, &num) && j == tl[i]){
                    if(nums == 0) st->flex_grow = (i16)(num / 10); else st->flex_shrink = (i16)(num / 10);
                    nums++;
                } else if(parse_len(c, tok[i], tl[i], &l)) st->flex_basis = l;
            }
        }
        break;
    case P_FLEX_GROW: { int j = 0; i32 num; if(parse_num(v, n, &j, &num)) st->flex_grow = (i16)(num / 10); break; }
    case P_FLEX_SHRINK: { int j = 0; i32 num; if(parse_num(v, n, &j, &num)) st->flex_shrink = (i16)(num / 10); break; }
    case P_FLEX_BASIS: if(parse_len(c, v, n, &l)) st->flex_basis = l; else if(str_ieqn(v, "content", n)) st->flex_basis.kind = L_AUTO; break;
    case P_JUSTIFY_CONTENT:
        if(str_istarts(v, "center")) st->justify = JC_CENTER;
        else if(str_istarts(v, "flex-end") || str_istarts(v, "end") || str_istarts(v, "right")) st->justify = JC_END;
        else if(str_istarts(v, "space-between")) st->justify = JC_BETWEEN;
        else if(str_istarts(v, "space-around")) st->justify = JC_AROUND;
        else if(str_istarts(v, "space-evenly")) st->justify = JC_EVENLY;
        else st->justify = JC_START;
        break;
    case P_ALIGN_ITEMS: case P_ALIGN_SELF: case P_PLACE_ITEMS: {
        u8 a = AI_STRETCH;
        if(str_istarts(v, "center")) a = AI_CENTER;
        else if(str_istarts(v, "flex-start") || str_istarts(v, "start") || str_istarts(v, "self-start")) a = AI_START;
        else if(str_istarts(v, "flex-end") || str_istarts(v, "end") || str_istarts(v, "self-end")) a = AI_END;
        else if(str_istarts(v, "baseline")) a = AI_BASELINE;
        else if(str_istarts(v, "auto")) a = 255;
        if(prop == P_ALIGN_SELF) st->align_self = a; else st->align_items = a == 255 ? AI_STRETCH : a;
        break; }
    case P_GAP:
        k = split_tokens(v, n, tok, tl, 2);
        if(k >= 1 && parse_len(c, tok[0], tl[0], &l)){ st->gap_row = l; st->gap_col = l; }
        if(k == 2 && parse_len(c, tok[1], tl[1], &l)) st->gap_col = l;
        break;
    case P_ROW_GAP: if(parse_len(c, v, n, &l)) st->gap_row = l; break;
    case P_COLUMN_GAP: if(parse_len(c, v, n, &l)) st->gap_col = l; break;
    case P_GRID_TEMPLATE_COLUMNS: st->grid_cols = arena_strndup(c->ar, v, (size_t)n); break;
    case P_GRID_COLUMN:
        if(strstr(v, "-1") || str_icontains(v, "span 99")) st->grid_span = -1;
        else if(str_icontains(v, "span ")) st->grid_span = (i16)atoi(str_icontains(v, "span ") + 5);
        break;
    case P_ORDER: st->order = (i16)atoi(v); break;
    case P_Z_INDEX: st->z_index = (i16)atoi(v); break;
    case P_OPACITY: { int j = 0; i32 num; if(parse_num(v, n, &j, &num)){ if(j < n && v[j] == '%') num /= 100; st->opacity = (i16)clamp255(num * 255 / 1000); } break; }
    case P_VERTICAL_ALIGN:
        if(str_ieqn(v, "top", n) || str_ieqn(v, "text-top", n)) st->valign = VA_TOP;
        else if(str_ieqn(v, "middle", n)) st->valign = VA_MIDDLE;
        else if(str_ieqn(v, "bottom", n) || str_ieqn(v, "text-bottom", n)) st->valign = VA_BOTTOM;
        else if(str_ieqn(v, "sub", n)) st->valign = VA_SUB;
        else if(str_ieqn(v, "super", n)) st->valign = VA_SUPER;
        else st->valign = VA_BASELINE;
        break;
    case P_MASK:
        st->masked = !str_ieqn(v, "none", n); st->mask_url = NULL;
        st->mask_rep = 3; st->mask_szk = 0; st->mask_px = len_px(0); st->mask_py = len_px(0);
        if(st->masked) set_layer(c, v, n, 1);
        break;
    case P_MASK_IMAGE: st->masked = !str_ieqn(v, "none", n); st->mask_url = str_istarts(v, "url(") ? css_url(c, v) : NULL; break;
    case P_MASK_SIZE: case P_BACKGROUND_SIZE: {
        int m = prop == P_MASK_SIZE;
        k = split_tokens(v, n, tok, tl, 8);
        set_size(c, tok, tl, k, m ? &st->mask_szk : &st->bg_szk, m ? &st->mask_sw : &st->bg_sw, m ? &st->mask_sh : &st->bg_sh);
        break;
    }
    case P_MASK_POSITION: case P_BACKGROUND_POSITION: {
        int m = prop == P_MASK_POSITION;
        k = split_tokens(v, n, tok, tl, 8);
        set_pos(c, tok, tl, k, m ? &st->mask_px : &st->bg_px, m ? &st->mask_py : &st->bg_py);
        break;
    }
    case P_MASK_REPEAT: case P_BACKGROUND_REPEAT:
        k = split_tokens(v, n, tok, tl, 8);
        if(k == 2){ u8 a = 0, b = 0; set_repeat(tok[0], tl[0], &a); set_repeat(tok[1], tl[1], &b); u8 r = (u8)((a & 1) | (b & 2)); if(prop == P_MASK_REPEAT) st->mask_rep = r; else st->bg_rep = r; }
        else if(k) set_repeat(tok[0], tl[0], prop == P_MASK_REPEAT ? &st->mask_rep : &st->bg_rep);
        break;
    case P_TRANSFORM: case P_TRANSLATE: {
        /* so translacoes: translate(x[, y]), translateX/Y, translate3d; o resto e ignorado */
        st->tx = len_px(0); st->ty = len_px(0);
        if(prop == P_TRANSLATE){
            k = split_tokens(v, n, tok, tl, 3);
            if(k >= 1) parse_len(c, tok[0], tl[0], &st->tx);
            if(k >= 2) parse_len(c, tok[1], tl[1], &st->ty);
            break;
        }
        for(const char* q = v; q < v + n; q++){
            int isx = str_istarts(q, "translatex("), isy = str_istarts(q, "translatey(");
            int both = str_istarts(q, "translate(") || str_istarts(q, "translate3d(");
            if(!isx && !isy && !both) continue;
            const char* a = strchr(q, '(') + 1;
            const char* e = a; int d = 0;
            while(e < v + n && !(*e == ')' && d == 0)){ if(*e == '(') d++; else if(*e == ')') d--; e++; }
            k = split_tokens(a, (int)(e - a), tok, tl, 3);
            Len l2;
            if(isx && k >= 1 && parse_len(c, tok[0], tl[0], &l2)){ st->tx.px += l2.px; st->tx.pct += l2.pct; }
            if(isy && k >= 1 && parse_len(c, tok[0], tl[0], &l2)){ st->ty.px += l2.px; st->ty.pct += l2.pct; }
            if(both){
                if(k >= 1 && parse_len(c, tok[0], tl[0], &l2)){ st->tx.px += l2.px; st->tx.pct += l2.pct; }
                if(k >= 2 && parse_len(c, tok[1], tl[1], &l2)){ st->ty.px += l2.px; st->ty.pct += l2.pct; }
            }
            q = e;
        }
        break; }
    case P_CURSOR: st->cursor_pointer = str_ieqn(v, "pointer", n); break;
    case P_CLIP:
        if(str_istarts(v, "rect(")){
            int j = 5; i32 a = 1, b = 1;
            while(j < n && (v[j] == ' ')) j++;
            parse_num(v, n, &j, &a);
            while(j < n && (v[j] == ' ' || v[j] == ',' || (v[j] >= 'a' && v[j] <= 'z'))) j++;
            parse_num(v, n, &j, &b);
            if(a == 0 && b <= 1000) st->hidden_clip = 1;
        }
        break;
    case P_CLIP_PATH:
        if(str_istarts(v, "inset(50%") || str_istarts(v, "inset(100%") || str_istarts(v, "circle(0")) st->hidden_clip = 1;
        break;
    }
}

/* ======================================================= estilo padrao */
static const char UA_CSS[] =
    "html,address,blockquote,body,dd,div,dl,dt,fieldset,form,frame,frameset,h1,h2,h3,h4,h5,h6,noframes,"
    "ol,p,ul,center,dir,hr,menu,pre,header,footer,nav,section,article,aside,main,figure,figcaption,hgroup,"
    "details,summary,legend,search,listing,xmp,plaintext,optgroup,address,dialog[open]{display:block}"
    "li{display:list-item}"
    "head,script,style,title,meta,link,template,noscript,datalist,param,source,track,base,area,"
    "[hidden],input[type=hidden i],dialog,noembed,noframes,map,slot,option{display:none}"
    "dialog[open]{display:block;position:absolute;left:0;right:0;margin:auto;border:2px solid;padding:1em;background:white}"
    "table{display:table;border-spacing:2px;box-sizing:border-box}"
    "tr{display:table-row}thead,tbody,tfoot{display:table-row-group}"
    "td,th{display:table-cell;padding:1px;vertical-align:middle}th{font-weight:bold;text-align:center}"
    "caption{display:table-caption;text-align:center}col,colgroup{display:none}"
    "body{margin:8px}"
    "h1{font-size:2em;margin:.67em 0;font-weight:bold}"
    "h2{font-size:1.5em;margin:.83em 0;font-weight:bold}"
    "h3{font-size:1.17em;margin:1em 0;font-weight:bold}"
    "h4{margin:1.33em 0;font-weight:bold}"
    "h5{font-size:.83em;margin:1.67em 0;font-weight:bold}"
    "h6{font-size:.67em;margin:2.33em 0;font-weight:bold}"
    "p,dl,ul,ol,menu,dir{margin:1em 0}blockquote,figure{margin:1em 40px}"
    "ul,ol,menu,dir{padding-left:40px}ol{list-style-type:decimal}"
    "ul ul,ol ul{list-style-type:circle}ul ul ul,ol ul ul,ul ol ul{list-style-type:square}"
    "li ul,li ol,ul ul,ol ol,ul ol,ol ul{margin-top:0;margin-bottom:0}"
    "dd{margin-left:40px}"
    "b,strong{font-weight:bold}i,em,cite,var,dfn,address{font-style:italic}"
    "code,kbd,samp,tt,pre,listing,xmp,plaintext{font-family:monospace}"
    "pre,listing,xmp,plaintext{white-space:pre;margin:1em 0}"
    "a:link{color:#0000EE;text-decoration:underline;cursor:pointer}"
    "u,ins{text-decoration:underline}s,strike,del{text-decoration:line-through}"
    "small{font-size:smaller}big{font-size:larger}"
    "sub{vertical-align:sub;font-size:smaller}sup{vertical-align:super;font-size:smaller}"
    "hr{border:1px solid #c0c0c0;margin:.5em 0;height:0}"
    "center{text-align:center}nobr{white-space:nowrap}"
    "mark{background:yellow;color:black}"
    "fieldset{margin:0 2px;padding:.35em .75em .625em;border:2px solid #c0c0c0}legend{padding:0 2px}"
    "details>:not(summary){display:none}details[open]>:not(summary){display:block}"
    "summary{display:block;cursor:pointer}"
    "img,svg,video,canvas,iframe,embed,object,audio{display:inline-block}"
    "iframe{border:2px solid #c0c0c0}"
    "input,select,textarea,button{display:inline-block;font-size:13px}"
    "input,textarea,select{border:1px solid #767676;padding:2px 3px;background:white;color:black}"
    "button,input[type=submit i],input[type=button i],input[type=reset i]{border:1px solid #767676;"
    "padding:2px 8px;background:#efefef;color:black;border-radius:3px;cursor:pointer;text-align:center}"
    "input[type=checkbox i],input[type=radio i]{border:1px solid #767676;padding:0;width:13px;height:13px;margin:3px}"
    "input[type=radio i]{border-radius:7px}"
    "input[type=image i]{border:0}"
    "textarea{white-space:pre-wrap}"
    "abbr[title]{text-decoration:underline}"
    "ruby>rt{display:none}";

/* modo quirks: tabelas nao herdam fonte nem alinhamento */
static const char QUIRKS_CSS[] =
    "table{text-align:left;font-size:medium;font-weight:normal;font-style:normal;white-space:normal;line-height:normal}"
    "img{vertical-align:bottom}";
static Sheet* quirks_sheet = NULL;

static Style ua_initial;
static int initial_ready = 0;
static const Style* initial_style(void){
    if(!initial_ready){
        memset(&ua_initial, 0, sizeof ua_initial);
        ua_initial.display = D_INLINE;
        ua_initial.font_size = 16;
        ua_initial.color = 0xFF000000;
        ua_initial.fam = FAM_SERIF; ua_initial.ffb = FAM_SERIF; ua_initial.weight = 400;
        ua_initial.bg_rep = 3; ua_initial.mask_rep = 3;
        ua_initial.bg_sw.kind = L_AUTO; ua_initial.bg_sh.kind = L_AUTO; ua_initial.mask_sw.kind = L_AUTO; ua_initial.mask_sh.kind = L_AUTO;
        ua_initial.bg_px = len_px(0); ua_initial.bg_py = len_px(0); ua_initial.mask_px = len_px(0); ua_initial.mask_py = len_px(0);
        ua_initial.visibility = 1;
        ua_initial.opacity = 255;
        ua_initial.flex_shrink = 100;
        ua_initial.align_self = 255;
        for(int k = 0; k < 4; k++){ ua_initial.margin[k] = len_px(0); ua_initial.padding[k] = len_px(0); ua_initial.border_c[k] = CUR_COLOR; }
        ua_initial.min_w.kind = L_AUTO; ua_initial.min_h.kind = L_AUTO;   /* auto: min-content em itens flex */
        ua_initial.max_w.kind = L_NONE; ua_initial.max_h.kind = L_NONE;
        ua_initial.gap_row = len_px(0); ua_initial.gap_col = len_px(0);
        initial_ready = 1;
    }
    return &ua_initial;
}

void css_init(void){
    if(ua_sheet) return;
    (void)font_family_id("");                 /* tabela de familias pronta antes das threads de estilo */
    ua_sheet = css_parse_sheet(&ua_arena, UA_CSS, (int)sizeof UA_CSS - 1, 0);
    quirks_sheet = css_parse_sheet(&ua_arena, QUIRKS_CSS, (int)sizeof QUIRKS_CSS - 1, 0);
}

/* ============================================================== indice */
typedef struct { const char* key; Rule** r; int n, cap; } Bucket;
#define NBUCKET 65536
static Bucket* buckets = NULL;           /* [NBUCKET] hash aberto */
static Rule** universal = NULL; static int nuniversal = 0, cuniversal = 0;
static Rule** rootrules = NULL; static int nrootrules = 0, crootrules = 0;      /* :root (so o elemento raiz) */
static int index_gen = -1, index_quirks = -1;
static Doc* index_doc = NULL;

static void bucket_push(Rule*** arr, int* n, int* cap, Rule* r){
    if(*n == *cap){ *cap = *cap ? *cap * 2 : 8; *arr = (Rule**)realloc(*arr, sizeof(Rule*) * (size_t)*cap); }
    (*arr)[(*n)++] = r;
}
static Bucket* bucket_find(const char* key, int create){
    u32 h = hash_str(key, (int)strlen(key)) & (NBUCKET - 1);
    for(int probe = 0; probe < NBUCKET; probe++){
        Bucket* b = &buckets[(h + (u32)probe) & (NBUCKET - 1)];
        if(!b->key){ if(!create) return NULL; b->key = key; return b; }
        if(!strcmp(b->key, key)) return b;
    }
    return NULL;
}
static void rule_ancestors(Rule* r);
static Arena key_arena;
static u32 rule_order = 0;
/* chave de indice de um composto ("" se nao tem id/classe/tag/atributo) */
static void comp_key(const Comp* c, char* key, int cap){
    if(c->id) snprintf(key, (size_t)cap, "#%s", c->id);
    else if(c->ncls) snprintf(key, (size_t)cap, ".%s", c->cls[0]);
    else if(c->tag) snprintf(key, (size_t)cap, "%s", c->tag);
    else if(c->nat) snprintf(key, (size_t)cap, "[%s", c->at[0].name);
    else key[0] = 0;
}
static int bucket_add(const char* key, Rule* r){
    Bucket* b = bucket_find(key, 0);
    if(!b){ b = bucket_find(arena_strdup(&key_arena, key), 1); if(!b) return 0; }
    bucket_push(&b->r, &b->n, &b->cap, r);
    return 1;
}
static void index_rule(Rule* r){
    r->order = rule_order++;            /* ordem do documento (as folhas podem ter sido analisadas em paralelo) */
    r->multi = 0;
    /* :focus, :active, :visited...: nunca casam aqui, nem entram no indice */
    for(int k = 0; k < r->sel->n; k++) if(r->sel->c[k].ps & PS_NEVER) return;
    if(!r->anc_ready) rule_ancestors(r);
    Comp* c = &r->sel->c[r->sel->n - 1];
    char key[300];
    comp_key(c, key, sizeof key);
    if(key[0]){ if(!bucket_add(key, r)) bucket_push(&universal, &nuniversal, &cuniversal, r); return; }
    if((c->ps & PS_ROOT) && !c->is_){ bucket_push(&rootrules, &nrootrules, &crootrules, r); return; }
    if(c->is_){
        /* :is(.a, .b) / :where(...): a regra entra no balde de cada alternativa */
        int ok = 1, cnt = 0;
        for(Sel* alt = c->is_; alt && ok; alt = alt->next){ comp_key(&alt->c[alt->n - 1], key, sizeof key); if(!key[0]) ok = 0; cnt++; }
        if(ok && cnt <= 16){
            for(Sel* alt = c->is_; alt; alt = alt->next){ comp_key(&alt->c[alt->n - 1], key, sizeof key); bucket_add(key, r); }
            r->multi = cnt > 1;
            return;
        }
    }
    bucket_push(&universal, &nuniversal, &cuniversal, r);
}
static Sheet* indexed[256]; static int nindexed = 0;
/* pagina trocada: as folhas antigas foram liberadas (e um Doc novo pode
 * reaproveitar o endereco): o proximo indice e feito do zero */
void css_forget_doc(void){ index_doc = NULL; nindexed = 0; index_gen = -1; }
static void build_index(Doc* d){
    if(index_gen == sheets_gen && index_doc == d && index_quirks == d->quirks) return;
    Sheet* list[256]; int n = 0;
    for(Sheet* sh = d->sheets; sh && n < 256; sh = sh->next) list[n++] = sh;
    /* so folhas novas (as antigas na mesma ordem relativa)? indexa so as novas */
    int incr = index_doc == d && index_quirks == d->quirks && buckets;
    if(incr){
        int j = 0;
        for(int i = 0; i < n && j < nindexed; i++) if(list[i] == indexed[j]) j++;
        incr = j == nindexed;
    }
    if(incr){
        for(int i = 0; i < n; i++){
            int old = 0;
            for(int k = 0; k < nindexed; k++) if(indexed[k] == list[i]){ old = 1; break; }
            if(!old) for(int k = 0; k < list[i]->n; k++) index_rule(list[i]->rules[k]);
        }
        /* ordem do documento: renumera tudo (barato: sem hash) */
        u32 o = 0;
        for(int k = 0; k < ua_sheet->n; k++) ua_sheet->rules[k]->order = o++;
        if(d->quirks) for(int k = 0; k < quirks_sheet->n; k++) quirks_sheet->rules[k]->order = o++;
        for(int i = 0; i < n; i++) for(int k = 0; k < list[i]->n; k++) list[i]->rules[k]->order = o++;
        rule_order = o;
    } else {
        if(!buckets) buckets = (Bucket*)calloc(NBUCKET, sizeof(Bucket));
        for(int i = 0; i < NBUCKET; i++){ free(buckets[i].r); }
        memset(buckets, 0, sizeof(Bucket) * NBUCKET);
        arena_reset(&key_arena);
        nuniversal = 0; nrootrules = 0;
        rule_order = 0;
        for(int i = 0; i < ua_sheet->n; i++) index_rule(ua_sheet->rules[i]);
        if(d->quirks) for(int i = 0; i < quirks_sheet->n; i++) index_rule(quirks_sheet->rules[i]);
        for(int i = 0; i < n; i++) for(int k = 0; k < list[i]->n; k++) index_rule(list[i]->rules[k]);
    }
    memcpy(indexed, list, sizeof(Sheet*) * (size_t)n); nindexed = n;
    index_gen = sheets_gen; index_doc = d; index_quirks = d->quirks;
    { int tot = nuniversal; for(int i = 0; i < NBUCKET; i++) tot += buckets[i].n;
      printf("[css] indice%s: %d regras (%d universais, %d :root)\n", incr ? " (incremental)" : "", tot, nuniversal, nrootrules); }
#ifdef VELA_DEBUG_INDEX
    for(int i = 0; i < nuniversal && i < 200; i++){
        Sel* sl = universal[i]->sel; Comp* c = &sl->c[sl->n - 1];
        printf("[univ] n=%d ps=%x nnot=%d is=%d pe=%d nth=%d comb=%c\n", sl->n, c->ps, c->nnot, c->is_ != NULL, sl->pe, c->nth_kind, c->comb ? c->comb : '-');
    }
#endif
}

/* ---- filtro de ancestrais: contagem de hashes de tag/id/classe dos
 * ancestros do elemento sendo estilizado. Uma regra "a .b .c" so precisa
 * ser testada se "a" e ".b" aparecem em algum ancestral. */
#define BLOOM 4096
/* contexto de uma thread calculando estilos: regras casadas, filtro de
 * ancestrais e a arena onde os estilos sao alocados */
/* compartilhamento de estilo: elementos com o mesmo pai (o mesmo Style),
 * a mesma tag e as mesmas regras casadas, sem estilo inline nem atributos de
 * apresentacao, tem o mesmo estilo computado */
typedef struct { u32 h; const Style* parent; const char* tag; Rule** rules; int nr, npr; Style* st; Style* ps[2]; } Share;
#define NSHARE 2048
typedef struct SCtx { Rule** matched; int nm, cm; Rule** pm; int npm, cpm; Rule** tmp; int ntmp, ctmp; u16 bloom[BLOOM]; Arena* ar;
                      Share share[NSHARE]; int shared, computed; } SCtx;
static u32 feat_hash(char kind, const char* s, int n){ return (hash_str(s, n) * 31u + (u32)kind) & (BLOOM - 1); }
static void bloom_node(SCtx* x, Node* e, int add){
    u16* bloom = x->bloom;
    const char* t = e->tag; if(t) bloom[feat_hash('t', t, (int)strlen(t))] += add ? 1 : -1;
    const char* id = dom_attr(e, "id"); if(id && *id) bloom[feat_hash('#', id, (int)strlen(id))] += add ? 1 : -1;
    const char* cl = dom_attr(e, "class");
    if(cl) for(const char* p = cl; *p;){
        while(*p && is_ws((u8)*p)) p++;
        const char* s0 = p; while(*p && !is_ws((u8)*p)) p++;
        if(p > s0) bloom[feat_hash('.', s0, (int)(p - s0))] += add ? 1 : -1;
    }
}
static void rule_ancestors(Rule* r){
    r->anc_ready = 1; r->nanc = 0;
    Sel* s = r->sel;
    /* compostos a esquerda de um combinador de descendencia/filho */
    int ok_from = s->n - 1;
    for(int i = s->n - 1; i > 0; i--){
        char cmb = s->c[i].comb;
        if(cmb == '+' || cmb == '~') break;              /* irmaos: nao sao ancestrais */
        ok_from = i - 1;
        Comp* c = &s->c[i - 1];
        if(r->nanc < 4 && c->id) r->anc[r->nanc++] = feat_hash('#', c->id, (int)strlen(c->id));
        if(r->nanc < 4 && c->ncls) r->anc[r->nanc++] = feat_hash('.', c->cls[0], (int)strlen(c->cls[0]));
        if(r->nanc < 4 && c->tag && !c->id && !c->ncls) r->anc[r->nanc++] = feat_hash('t', c->tag, (int)strlen(c->tag));
    }
    (void)ok_from;
}

/* ============================================================== cascata */
static Mutex block_lock = MUTEX_INIT;     /* analise preguicosa de blocos, de varias threads */

static void try_rules(SCtx* x, Node* e, Rule** rs, int n){
    for(int i = 0; i < n; i++){
        Rule* r = rs[i];
        int skip = 0;
        for(int k = 0; k < r->nanc; k++) if(!x->bloom[r->anc[k]]){ skip = 1; break; }
        if(skip) continue;
        if(match_sel(e, r->sel, r->sel->n - 1)){
            Block* b = r->b;
            if(!__atomic_load_n(&b->parsed, __ATOMIC_ACQUIRE)){
                mutex_lock(&block_lock);
                if(!b->parsed){ b->nd = parse_decls(b->a, b->src, b->len, &b->d); b->uses_attr = strstr(b->src, "attr(") != NULL; __atomic_store_n(&b->parsed, 1, __ATOMIC_RELEASE); }
                mutex_unlock(&block_lock);
            }
            if(b->nd){
                Rule*** arr = r->sel->pe ? &x->pm : &x->matched;            /* ::before/::after: estilo a parte */
                int* cnt = r->sel->pe ? &x->npm : &x->nm;
                if(r->multi){ int dup = 0; for(int k = *cnt - 1; k >= 0 && !dup; k--) if((*arr)[k] == r) dup = 1; if(dup) continue; }
                bucket_push(arr, cnt, r->sel->pe ? &x->cpm : &x->cm, r);
            }
        }
    }
}
static int rule_cmp(const void* a, const void* b){
    const Rule* x = *(Rule* const*)a; const Rule* y = *(Rule* const*)b;
    if(x->origin != y->origin) return x->origin - y->origin;
    if(x->spec != y->spec) return x->spec < y->spec ? -1 : 1;
    return x->order < y->order ? -1 : x->order > y->order ? 1 : 0;
}

/* o escopo do pai e compartilhado: o elemento ganha um escopo proprio
 * (com o do pai como `parent`) na primeira propriedade que definir */
static void var_put(VarScope* sc, const char* name, const char* val, u32 h){
    int i = (int)(h & (u32)(sc->cap - 1));
    while(sc->tab[i].hash && !(sc->tab[i].hash == h && !strcmp(sc->tab[i].name, name))) i = (i + 1) & (sc->cap - 1);
    if(!sc->tab[i].hash) sc->n++;
    sc->tab[i].name = name; sc->tab[i].value = val; sc->tab[i].hash = h;
}
static void set_var(Arena* ar, Style* st, const Style* parent, const char* name, const char* val){
    VarScope* sc = st->vars;
    if(!sc || (parent && sc == parent->vars)){
        sc = (VarScope*)arena_alloc(ar, sizeof(VarScope));
        sc->parent = st->vars; sc->cap = 16;
        sc->tab = (Var*)arena_alloc(ar, sizeof(Var) * 16);
        st->vars = sc;
    }
    if(sc->n * 10 >= sc->cap * 7){
        Var* old = sc->tab; int oc = sc->cap;
        sc->cap *= 4; sc->n = 0;
        sc->tab = (Var*)arena_alloc(ar, sizeof(Var) * (size_t)sc->cap);
        for(int i = 0; i < oc; i++) if(old[i].hash) var_put(sc, old[i].name, old[i].value, old[i].hash);
    }
    /* "--x: var(--x, 1rem)": ciclo -> a propriedade fica invalida e quem
     * usar var(--x, ...) pega o valor reserva */
    int nl = (int)strlen(name);
    for(const char* p = val; p && (p = strstr(p, "var(")); p += 4){
        const char* q = p + 4; while(is_ws((u8)*q)) q++;
        if(!strncmp(q, name, (size_t)nl) && (q[nl] == ',' || q[nl] == ')' || is_ws((u8)q[nl]))){ val = NULL; break; }
    }
    var_put(sc, name, val, hash_str(name, nl) | 1);
}

static void apply_value(Ctx* c, int prop, const char* v, int n){
    if(n >= 4 && memchr(v, '(', (size_t)n) && str_icontains(v, "var(")){
        Buf b = {0};
        char tmp[1024];
        if(expand_vars(c->st, v, n, &b, 0) && b.n){
            int bn = b.n < 1023 ? b.n : 1023;
            memcpy(tmp, b.p, (size_t)bn); tmp[bn] = 0;
            buf_free(&b);
            apply_decl(c, prop, tmp, bn);
        } else buf_free(&b);
        return;
    }
    apply_decl(c, prop, v, n);
}

/* dicas de apresentacao (atributos HTML antigos) */
static void presentational(Ctx* c, Node* e, int font_size){
    const char* v;
    char tmp[300];
    #define PX_OR_PCT(val) ({ const char* _v = (val); int _l = (int)strlen(_v); \
        (_l && _v[_l - 1] == '%') ? snprintf(tmp, sizeof tmp, "%s", _v) : snprintf(tmp, sizeof tmp, "%dpx", atoi(_v)); tmp; })
    if((v = dom_attr(e, "bgcolor"))){ apply_decl(c, P_BACKGROUND_COLOR, v, (int)strlen(v)); }
    if(dom_is(e, "body") && (v = dom_attr(e, "text"))) apply_decl(c, P_COLOR, v, (int)strlen(v));
    if((v = dom_attr(e, "width")) && *v && !dom_is(e, "img") && !dom_is(e, "canvas") && !dom_is(e, "svg")) { const char* t = PX_OR_PCT(v); apply_decl(c, P_WIDTH, t, (int)strlen(t)); }
    if((v = dom_attr(e, "height")) && *v && !dom_is(e, "img") && !dom_is(e, "canvas") && !dom_is(e, "svg")){ const char* t = PX_OR_PCT(v); apply_decl(c, P_HEIGHT, t, (int)strlen(t)); }
    if((v = dom_attr(e, "align"))){
        if(dom_is(e, "table") || dom_is(e, "img")){
            if(str_ieq(v, "center")){ c->st->margin[1].kind = L_AUTO; c->st->margin[3].kind = L_AUTO; }
            else if(str_ieq(v, "left")) c->st->floating = 1;
            else if(str_ieq(v, "right")) c->st->floating = 2;
        } else apply_decl(c, P_TEXT_ALIGN, v, (int)strlen(v));
    }
    if((v = dom_attr(e, "valign"))) apply_decl(c, P_VERTICAL_ALIGN, v, (int)strlen(v));
    if(dom_is(e, "font") && font_size){
        if((v = dom_attr(e, "size"))){
            static const i16 SZ[8] = {10, 10, 13, 16, 18, 24, 32, 48};
            int s = atoi(v); if(v[0] == '+') s = 3 + atoi(v + 1); else if(v[0] == '-') s = 3 - atoi(v + 1);
            if(s < 1) s = 1; if(s > 7) s = 7;
            c->st->font_size = SZ[s];
        }
        return;
    }
    if(dom_is(e, "font")){
        if((v = dom_attr(e, "color"))) apply_decl(c, P_COLOR, v, (int)strlen(v));
        if((v = dom_attr(e, "face"))) set_family(c->st, v, (int)strlen(v));
    }
    if(dom_is(e, "table")){
        if((v = dom_attr(e, "border")) && atoi(v) > 0){ snprintf(tmp, sizeof tmp, "%dpx solid #808080", atoi(v)); apply_decl(c, P_BORDER, tmp, (int)strlen(tmp)); }
        if((v = dom_attr(e, "cellspacing"))) c->st->gap_col = len_px(atoi(v));
    }
    if(dom_is(e, "td") || dom_is(e, "th")){
        Node* t = e->parent; while(t && !dom_is(t, "table")) t = t->parent;
        if(t){
            if((v = dom_attr(t, "cellpadding"))) for(int k = 0; k < 4; k++) c->st->padding[k] = len_px(atoi(v));
            if((v = dom_attr(t, "border")) && atoi(v) > 0) for(int k = 0; k < 4; k++){ c->st->border_w[k] = 1; c->st->border_c[k] = 0xFF808080; }
        }
        if(dom_attr(e, "nowrap")) c->st->white_space = WS_NOWRAP;
    }
    if(dom_is(e, "img")){
        if((v = dom_attr(e, "border"))){ int b = atoi(v); for(int k = 0; k < 4; k++){ c->st->border_w[k] = (i16)b; c->st->border_c[k] = CUR_COLOR; } }
        if((v = dom_attr(e, "hspace"))){ c->st->margin[1] = c->st->margin[3] = len_px(atoi(v)); }
        if((v = dom_attr(e, "vspace"))){ c->st->margin[0] = c->st->margin[2] = len_px(atoi(v)); }
    }
    if(dom_is(e, "hr")){
        if((v = dom_attr(e, "size"))) c->st->border_w[0] = (i16)(atoi(v) > 1 ? atoi(v) - 1 : 1);
        if((v = dom_attr(e, "color"))){ int ok; u32 col = css_parse_color(v, (int)strlen(v), &ok); if(ok){ for(int k = 0; k < 4; k++) c->st->border_c[k] = col; c->st->bg = col; } }
        if((v = dom_attr(e, "width"))){ const char* t = PX_OR_PCT(v); apply_decl(c, P_WIDTH, t, (int)strlen(t)); c->st->margin[1].kind = L_AUTO; c->st->margin[3].kind = L_AUTO; }
    }
    if((dom_is(e, "ul") || dom_is(e, "ol") || dom_is(e, "li")) && (v = dom_attr(e, "type"))){
        if(!strcmp(v, "1")) c->st->list_style = LS_DECIMAL;
        else if(str_ieq(v, "a")) c->st->list_style = LS_ALPHA;
        else if(str_ieq(v, "i")) c->st->list_style = LS_ROMAN;
        else apply_decl(c, P_LIST_STYLE_TYPE, v, (int)strlen(v));
    }
    #undef PX_OR_PCT
}

static Style* new_style(SCtx* x, const Style* parent){
    Style* st = (Style*)arena_alloc(x->ar, sizeof(Style));
    const Style* ini = initial_style();
    *st = *ini;
    if(parent){
        st->color = parent->color; st->font_size = parent->font_size; st->bold = parent->bold; st->weight = parent->weight;
        st->italic = parent->italic; st->mono = parent->mono; st->fam = parent->fam; st->ffb = parent->ffb; st->text_align = parent->text_align;
        st->white_space = parent->white_space; st->list_style = parent->list_style;
        st->line_height = parent->line_height; st->visibility = parent->visibility;
        st->upper = parent->upper; st->lower = parent->lower; st->capitalize = parent->capitalize;
        st->cursor_pointer = parent->cursor_pointer; st->vars = parent->vars;
        st->underline = parent->underline; st->strike = parent->strike;
        st->letter_spacing = parent->letter_spacing; st->indent = parent->indent;
        st->break_words = parent->break_words;
    }
    return st;
}

/* a cascata: aplica as regras M (ja em ordem) e o estilo inline sobre st */
static void cascade(SCtx* x, Node* e, Style* st, const Style* parent, i32 root_fs, Rule** M, int NM, Decl* idecl, int nid, int pseudo){
    Ctx c = { st, parent, e, root_fs, x->ar };
    /* 1) propriedades --custom (normais, depois !important) */
    for(int imp = 0; imp < 2; imp++){
        for(int i = 0; i < NM; i++) for(int k = 0; k < M[i]->b->nd; k++){
            Decl* d = &M[i]->b->d[k]; c.base = M[i]->b->sheet ? M[i]->b->sheet->base : NULL;
            if(d->prop == P_CUSTOM && d->important == imp) set_var(x->ar, st, parent, d->name, d->val);
        }
        for(int k = 0; k < nid; k++) if(idecl[k].prop == P_CUSTOM && idecl[k].important == imp) set_var(x->ar, st, parent, idecl[k].name, idecl[k].val);
    }
    /* 2) font-size primeiro: em/ex das outras propriedades dependem dele */
    const Decl* fs = NULL;
    for(int imp = 0; imp < 2; imp++){
        for(int i = 0; i < NM; i++) for(int k = 0; k < M[i]->b->nd; k++){
            Decl* d = &M[i]->b->d[k]; c.base = M[i]->b->sheet ? M[i]->b->sheet->base : NULL;
            if((d->prop == P_FONT_SIZE || d->prop == P_FONT) && d->important == imp) fs = d;
        }
        for(int k = 0; k < nid; k++) if((idecl[k].prop == P_FONT_SIZE || idecl[k].prop == P_FONT) && idecl[k].important == imp) fs = &idecl[k];
    }
    if(!pseudo && dom_is(e, "font") && dom_attr(e, "size")) presentational(&c, e, 1);
    if(fs){
        if(fs->prop == P_FONT_SIZE){
            const char* v = fs->val; int n = fs->vlen;
            Buf b = {0};
            if(str_icontains(v, "var(") && expand_vars(st, v, n, &b, 0)){ set_font_size(&c, b.p, b.n); }
            else set_font_size(&c, v, n);
            buf_free(&b);
        } else apply_value(&c, P_FONT, fs->val, fs->vlen);
    }
    if(!pseudo && e->parent && e->parent->type == N_DOC) c.root_fs = st->font_size;
    /* 3) o resto, por precedencia */
    int ua_end = 0;
    while(ua_end < NM && M[ua_end]->origin == 0) ua_end++;
    for(int i = 0; i < ua_end; i++) for(int k = 0; k < M[i]->b->nd; k++){
        Decl* d = &M[i]->b->d[k]; c.base = M[i]->b->sheet ? M[i]->b->sheet->base : NULL;
        if(!d->important && d->prop != P_CUSTOM && d->prop != P_FONT_SIZE) apply_value(&c, d->prop, d->val, d->vlen);
    }
    if(!pseudo) presentational(&c, e, 0);
    for(int i = ua_end; i < NM; i++) for(int k = 0; k < M[i]->b->nd; k++){
        Decl* d = &M[i]->b->d[k]; c.base = M[i]->b->sheet ? M[i]->b->sheet->base : NULL;
        if(!d->important && d->prop != P_CUSTOM && d->prop != P_FONT_SIZE && d->prop != P_FONT) apply_value(&c, d->prop, d->val, d->vlen);
    }
    for(int k = 0; k < nid; k++) if(!idecl[k].important && idecl[k].prop != P_CUSTOM && idecl[k].prop != P_FONT_SIZE && idecl[k].prop != P_FONT)
        { c.base = NULL; apply_value(&c, idecl[k].prop, idecl[k].val, idecl[k].vlen); }
    for(int i = 0; i < NM; i++) for(int k = 0; k < M[i]->b->nd; k++){
        Decl* d = &M[i]->b->d[k]; c.base = M[i]->b->sheet ? M[i]->b->sheet->base : NULL;
        if(d->important && d->prop != P_CUSTOM && d->prop != P_FONT_SIZE && d->prop != P_FONT) apply_value(&c, d->prop, d->val, d->vlen);
    }
    for(int k = 0; k < nid; k++) if(idecl[k].important && idecl[k].prop != P_CUSTOM && idecl[k].prop != P_FONT_SIZE && idecl[k].prop != P_FONT)
        { c.base = NULL; apply_value(&c, idecl[k].prop, idecl[k].val, idecl[k].vlen); }

    /* acertos finais */
    for(int s = 0; s < 4; s++) if(st->border_c[s] == CUR_COLOR) st->border_c[s] = st->color;
    if(st->position == POS_ABSOLUTE || st->position == POS_FIXED || st->floating){
        if(st->display == D_INLINE || st->display == D_INLINE_BLOCK) st->display = D_BLOCK;
        else if(st->display == D_INLINE_FLEX) st->display = D_FLEX;
        else if(st->display == D_INLINE_GRID) st->display = D_GRID;
        else if(st->display == D_INLINE_TABLE) st->display = D_TABLE;
    }
    if(parent && (parent->display == D_FLEX || parent->display == D_INLINE_FLEX || parent->display == D_GRID || parent->display == D_INLINE_GRID)){
        if(st->display == D_INLINE || st->display == D_INLINE_BLOCK) st->display = D_BLOCK;
        else if(st->display == D_INLINE_FLEX) st->display = D_FLEX;
        else if(st->display == D_INLINE_TABLE) st->display = D_TABLE;
        else if(st->display == D_INLINE_GRID) st->display = D_GRID;
    }
    if(!pseudo && e->parent && e->parent->type == N_DOC && st->display == D_INLINE) st->display = D_BLOCK;
    if(st->position == POS_ABSOLUTE && st->overflow_hidden){
        i32 w = st->width.kind == L_VAL && !st->width.pct ? st->width.px : 100;
        i32 h = st->height.kind == L_VAL && !st->height.pct ? st->height.px : 100;
        if(w <= 1 && h <= 1) st->hidden_clip = 1;
    }
    if(st->position == POS_ABSOLUTE || st->position == POS_FIXED){
        /* "skip links" fora da tela: left:-9999px */
        if((st->inset[3].kind == L_VAL && st->inset[3].px < -1000) || (st->inset[0].kind == L_VAL && st->inset[0].px < -1000)) st->hidden_clip = 1;
    }
    if(st->font_size < 1) st->font_size = 1;
}


/* ::before / ::after: um Node gerado (fora do DOM) com um texto filho */
static void make_pseudo(SCtx* x, Node* e, int pe, Style* ps){
    Node* p = (Node*)arena_alloc(x->ar, sizeof(Node));
    Node* t = (Node*)arena_alloc(x->ar, sizeof(Node));
    memset(p, 0, sizeof *p); memset(t, 0, sizeof *t);
    p->type = N_ELEM; p->tag = pe == 1 ? "::before" : "::after"; p->flags = NF_PSEUDO;
    p->parent = e; p->cs = ps; p->first = p->last = t;
    t->type = N_TEXT; t->parent = p; t->text = (char*)ps->content; t->tlen = (int)strlen(ps->content);
    if(!t->tlen){ p->first = p->last = NULL; }
    e->pse[pe - 1] = p;
}
/* ps[]: os estilos gerados (NULL = sem caixa), para o compartilhamento */
static void style_pseudos(SCtx* x, Node* e, Style* st, i32 root_fs, Style** out){
    for(int pe = 1; pe <= 2; pe++){
        x->ntmp = 0;
        for(int i = 0; i < x->npm; i++) if(x->pm[i]->sel->pe == pe) bucket_push(&x->tmp, &x->ntmp, &x->ctmp, x->pm[i]);
        if(!x->ntmp) continue;
        Style* ps = new_style(x, st);
        cascade(x, e, ps, st, root_fs, x->tmp, x->ntmp, NULL, 0, pe);
        if(!ps->content || ps->display == D_NONE) continue;
        make_pseudo(x, e, pe, ps);
        if(out) out[pe - 1] = ps;
    }
}

/* o estilo pode vir do cache de compartilhamento? (nada alem das regras
 * casadas pode influir: sem atributos de apresentacao, sem tags especiais) */
static int can_share(const Node* e){
    static const char* const TAGS[] = { "td", "th", "table", "font", "body", "img", "canvas", "svg", "hr", "html",
        "input", "select", "textarea", "iframe", "video", "embed", "object", NULL };
    for(int i = 0; TAGS[i]; i++) if(!strcmp(e->tag, TAGS[i])) return 0;
    static const char* const ATTRS[] = { "align", "bgcolor", "border", "cellspacing", "cellpadding", "color", "face", "height",
        "hspace", "nowrap", "size", "text", "valign", "vspace", "width", "type", "dir", "hidden", "start", "value", NULL };
    for(int k = 0; k < e->nattr; k++){
        const char* n = e->attrs[k].name;
        for(int i = 0; ATTRS[i]; i++) if(!strcmp(n, ATTRS[i])) return 0;
    }
    return 1;
}

/* calcula o estilo de um elemento (o pai ja tem o seu) e o dos seus
 * ::before/::after */
static void style_element(SCtx* x, Node* e, const Style* parent, i32 root_fs){
    e->pse[0] = e->pse[1] = NULL;
    /* regras que casam */
    x->nm = 0; x->npm = 0;
    const char* id = dom_attr(e, "id");
    char key[300];
    if(id && *id){ snprintf(key, sizeof key, "#%s", id); Bucket* b = bucket_find(key, 0); if(b) try_rules(x, e, b->r, b->n); }
    const char* cls = dom_attr(e, "class");
    if(cls){
        const char* seen[32]; int ns = 0;
        for(const char* p = cls; *p;){
            while(*p && is_ws((u8)*p)) p++;
            const char* s0 = p; while(*p && !is_ws((u8)*p)) p++;
            int l = (int)(p - s0);
            if(!l || l > 290) continue;
            int dup = 0;
            for(int k = 0; k < ns; k++) if((int)strlen(seen[k]) == l + 1 && !memcmp(seen[k] + 1, s0, (size_t)l)) dup = 1;
            if(dup) continue;
            key[0] = '.'; memcpy(key + 1, s0, (size_t)l); key[l + 1] = 0;
            Bucket* b = bucket_find(key, 0);
            if(b){ try_rules(x, e, b->r, b->n); if(ns < 32) seen[ns++] = b->key; }
        }
    }
    { Bucket* b = bucket_find(e->tag, 0); if(b) try_rules(x, e, b->r, b->n); }
    for(int i = 0; i < e->nattr; i++){
        int l = (int)strlen(e->attrs[i].name);
        if(l > 290) continue;
        key[0] = '['; memcpy(key + 1, e->attrs[i].name, (size_t)l + 1);
        Bucket* b = bucket_find(key, 0); if(b) try_rules(x, e, b->r, b->n);
    }
    try_rules(x, e, universal, nuniversal);
    if(e->parent && e->parent->type == N_DOC) try_rules(x, e, rootrules, nrootrules);
    if(x->nm > 1) qsort(x->matched, (size_t)x->nm, sizeof(Rule*), rule_cmp);

    const char* inl = dom_attr(e, "style");
    Share* sh = NULL; u32 h = 0;
    if(x->npm > 1) qsort(x->pm, (size_t)x->npm, sizeof(Rule*), rule_cmp);
    int attr_content = 0;
    for(int i = 0; i < x->npm; i++) if(x->pm[i]->b->uses_attr) attr_content = 1;
    if(!attr_content && !(inl && *inl) && parent && can_share(e)){
        h = (u32)(u64)parent * 2654435761u ^ hash_str(e->tag, (int)strlen(e->tag));
        for(int i = 0; i < x->nm; i++) h = h * 31u + (u32)((u64)x->matched[i] >> 4);
        for(int i = 0; i < x->npm; i++) h = h * 37u + (u32)((u64)x->pm[i] >> 4);
        sh = &x->share[h & (NSHARE - 1)];
        if(sh->st && sh->h == h && sh->parent == parent && sh->nr == x->nm && sh->npr == x->npm && !strcmp(sh->tag, e->tag)
           && !memcmp(sh->rules, x->matched, sizeof(Rule*) * (size_t)x->nm)
           && !memcmp(sh->rules + x->nm, x->pm, sizeof(Rule*) * (size_t)x->npm)){
            e->cs = sh->st; x->shared++;
            for(int pe = 1; pe <= 2; pe++) if(sh->ps[pe - 1]) make_pseudo(x, e, pe, sh->ps[pe - 1]);
            return;
        }
    }
    Decl* idecl = NULL; int nid = 0;
    if(inl && *inl) nid = parse_decls(x->ar, inl, (int)strlen(inl), &idecl);

    Style* st = new_style(x, parent);
    e->cs = st;
    cascade(x, e, st, parent, root_fs, x->matched, x->nm, idecl, nid, 0);
    Style* pst[2] = { NULL, NULL };
    if(x->npm && st->display != D_NONE) style_pseudos(x, e, st, root_fs, pst);
    x->computed++;
    if(sh){
        sh->h = h; sh->parent = parent; sh->tag = e->tag; sh->nr = x->nm; sh->npr = x->npm; sh->st = st;
        sh->ps[0] = pst[0]; sh->ps[1] = pst[1];
        sh->rules = (Rule**)arena_alloc(x->ar, sizeof(Rule*) * (size_t)(x->nm + x->npm + 1));
        memcpy(sh->rules, x->matched, sizeof(Rule*) * (size_t)x->nm);
        memcpy(sh->rules + x->nm, x->pm, sizeof(Rule*) * (size_t)x->npm);
    }
}

static void clear_styles(Node* n){
    n->pse[0] = n->pse[1] = NULL;
    for(Node* k = n->first; k; k = k->next){ k->cs = NULL; k->pse[0] = k->pse[1] = NULL; if(k->first) clear_styles(k); }
}

static void style_tree(SCtx* x, Node* n, const Style* parent, i32 root_fs){
    for(Node* k = n->first; k; k = k->next){
        if(k->type != N_ELEM){ k->cs = NULL; continue; }
        style_element(x, k, parent, root_fs);
        i32 rfs = (n->type == N_DOC) ? k->cs->font_size : root_fs;
        if(k->cs->display == D_NONE){ clear_styles(k); k->cs = NULL; }   /* sem caixa */
        else if(k->first){ bloom_node(x, k, 1); style_tree(x, k, k->cs, rfs); bloom_node(x, k, 0); }
    }
}

/* ---- estilo em paralelo: subarvores grandes viram tarefas executadas
 * pelos workers e pela propria thread principal. Cada tarefa tem seu
 * contexto e sua arena; o pai de cada subarvore ja tem estilo. */
#define MAXJOBS 1024
#define MAXHELP 4
typedef struct { Node* n; Node* last; i32 rfs; int size; } SJob;   /* irmaos n..last */
static struct {
    SJob job[MAXJOBS]; int njobs;
    volatile int next, finished;
    volatile u32 all_done;                 /* futex: 1 quando finished == njobs */
    volatile int open;                     /* ha tarefas desta rodada para pegar */
    volatile int active;                   /* ajudantes dentro de run_jobs */
    volatile int queued[MAXHELP];          /* help_task[i] ainda na fila dos workers */
} sp;
static SCtx job_ctx[MAXHELP + 1];          /* 0 = thread principal */
static Arena job_arena[MAXHELP + 1];
static Task help_task[MAXHELP];

/* tamanho de cada subarvore (indice = id do nodo), calculado uma vez por passo */
static int* subsz = NULL; static int csubsz = 0;
static void subtree_sizes(Doc* d){
    if(csubsz < d->nnodes + 1){ csubsz = d->nnodes + 64; subsz = (int*)realloc(subsz, sizeof(int) * (size_t)csubsz); }
    memset(subsz, 0, sizeof(int) * (size_t)(d->nnodes + 1));
    /* em pos-ordem: cada nodo soma 1 + o seu tamanho ao pai */
    Node* n = d->root;
    while(n->first) n = n->first;
    for(;;){
        if(n == d->root) break;
        Node* p = n->parent;
        if(n->id > 0 && n->id <= d->nnodes && p && p->id > 0 && p->id <= d->nnodes) subsz[p->id] += subsz[n->id] + 1;
        if(n->next){ n = n->next; while(n->first) n = n->first; }
        else n = p;
    }
}
static int count_nodes(Node* n, int cap){
    (void)cap;
    return n->id > 0 ? subsz[n->id] : 0;
}

static void run_jobs(SCtx* x){
    for(;;){
        int i = __atomic_fetch_add(&sp.next, 1, __ATOMIC_ACQ_REL);
        if(i >= sp.njobs) return;
        Node* k0 = sp.job[i].n;
        /* filtro de ancestrais destas subarvores */
        memset(x->bloom, 0, sizeof x->bloom);
        for(Node* a = k0->parent; a && a->type == N_ELEM; a = a->parent) bloom_node(x, a, 1);
        const Style* par = k0->parent && k0->parent->type == N_ELEM ? k0->parent->cs : NULL;
        for(Node* k = k0; k; k = k->next){
            if(k->type == N_ELEM){
                style_element(x, k, par, sp.job[i].rfs);
                i32 rfs = (k->parent && k->parent->type == N_DOC) ? k->cs->font_size : sp.job[i].rfs;
                if(k->cs->display == D_NONE){ clear_styles(k); k->cs = NULL; }
                else if(k->first){ bloom_node(x, k, 1); style_tree(x, k, k->cs, rfs); bloom_node(x, k, 0); }
            } else k->cs = NULL;
            if(k == sp.job[i].last) break;
        }
        if(__atomic_add_fetch(&sp.finished, 1, __ATOMIC_ACQ_REL) == sp.njobs){
            __atomic_store_n(&sp.all_done, 1, __ATOMIC_RELEASE);
            sys_futex_wake(&sp.all_done, 1);
        }
    }
}
/* um ajudante pode sair da fila dos workers atrasado (eles estavam
 * ocupados com imagens): se a rodada ja acabou, nao faz nada */
static void help_run(Task* t){
    int i = (int)(t - help_task);
    __atomic_store_n(&sp.queued[i], 0, __ATOMIC_SEQ_CST);
    __atomic_add_fetch(&sp.active, 1, __ATOMIC_SEQ_CST);
    if(__atomic_load_n(&sp.open, __ATOMIC_SEQ_CST)) run_jobs(&job_ctx[i + 1]);
    __atomic_sub_fetch(&sp.active, 1, __ATOMIC_SEQ_CST);
}

/* estiliza ate a profundidade em que as subarvores ficam "do tamanho certo";
 * as grandes demais sao divididas, as boas viram tarefa */
static void split(SCtx* x, Node* n, const Style* parent, i32 rfs0, int target){
    int open = -1;                  /* tarefa juntando irmaos pequenos seguidos */
    for(Node* k = n->first; k; k = k->next){
        if(k->type != N_ELEM){ if(open < 0) k->cs = NULL; continue; }
        i32 rfs = (n->type == N_DOC) ? 16 : rfs0;
        int size = count_nodes(k, target * 2 + 1) + 1;
        if(sp.njobs >= MAXJOBS - 1 || n->type == N_DOC){
            style_element(x, k, parent, rfs);
            if(n->type == N_DOC) rfs = k->cs->font_size;
            if(k->cs->display == D_NONE){ clear_styles(k); k->cs = NULL; }
            else if(n->type == N_DOC && k->first && size > target){ bloom_node(x, k, 1); split(x, k, k->cs, rfs, target); bloom_node(x, k, 0); }
            else if(k->first){ bloom_node(x, k, 1); style_tree(x, k, k->cs, rfs); bloom_node(x, k, 0); }
            continue;
        }
        if(size <= target){
            if(open >= 0 && sp.job[open].size + size <= target){ sp.job[open].last = k; sp.job[open].size += size; }
            else { open = sp.njobs++; sp.job[open].n = k; sp.job[open].last = k; sp.job[open].rfs = rfs; sp.job[open].size = size; }
            continue;
        }
        open = -1;
        /* grande: estiliza o proprio elemento aqui e divide os filhos */
        style_element(x, k, parent, rfs);
        if(n->type == N_DOC) rfs = k->cs->font_size;
        if(k->cs->display == D_NONE){ clear_styles(k); k->cs = NULL; continue; }
        bloom_node(x, k, 1);
        split(x, k, k->cs, rfs, target);
        bloom_node(x, k, 0);
    }
}

void css_style_doc(Doc* d, i32 vw, i32 vh){
    css_init();
    css_vw = vw; css_vh = vh;
    u64 t0 = sys_time_ms();
    build_index(d);
    u64 t1 = sys_time_ms();
    (void)initial_style();
    /* nenhum ajudante da rodada anterior pode estar usando os contextos */
    while(__atomic_load_n(&sp.active, __ATOMIC_SEQ_CST)) sys_yield();
    for(int i = 0; i <= MAXHELP; i++){
        arena_reset(&job_arena[i]); job_ctx[i].ar = &job_arena[i];
        memset(job_ctx[i].share, 0, sizeof job_ctx[i].share); job_ctx[i].shared = job_ctx[i].computed = 0;
    }
    SCtx* x = &job_ctx[0];
    memset(x->bloom, 0, sizeof x->bloom);
    int total = d->nnodes;
    int helpers = threads_started ? (int)cpu_count() - 1 : 0;
    if(helpers > MAXHELP) helpers = MAXHELP;
    if(helpers < 1 || total < 400){ style_tree(x, d->root, NULL, 16); return; }
    /* fase 1 (principal): o topo da arvore e a lista de subarvores */
    sp.njobs = 0; sp.next = 0; sp.finished = 0; sp.all_done = 0;
    subtree_sizes(d);
    split(x, d->root, NULL, 16, total / (helpers * 4) + 1);
    u64 t2 = sys_time_ms();
    if(!sp.njobs) return;
    /* fase 2: workers e a principal pegam subarvores ate acabar */
    __atomic_store_n(&sp.open, 1, __ATOMIC_SEQ_CST);
    for(int i = 0; i < helpers; i++){
        if(__atomic_load_n(&sp.queued[i], __ATOMIC_SEQ_CST)) continue;   /* ainda na fila: serve a esta rodada */
        sp.queued[i] = 1;
        help_task[i].run = help_run; help_task[i].done = NULL;
        work_submit_front(&help_task[i]);
    }
    run_jobs(x);
    while(!__atomic_load_n(&sp.all_done, __ATOMIC_ACQUIRE)) sys_futex_wait(&sp.all_done, 0, 5);
    __atomic_store_n(&sp.open, 0, __ATOMIC_SEQ_CST);
    u64 t3 = sys_time_ms();
    int nsh = 0, ncp = 0; for(int i = 0; i <= MAXHELP; i++){ nsh += job_ctx[i].shared; ncp += job_ctx[i].computed; }
    if(t3 - t0 > 40) printf("[css] estilo: indice %u ms, topo %u ms, subarvores %u ms (%d tarefas; %d calculados, %d compartilhados)\n",
        (unsigned)(t1 - t0), (unsigned)(t2 - t1), (unsigned)(t3 - t2), sp.njobs, ncp, nsh);
}

/* ---- getComputedStyle (subconjunto) ---- */
static void put_color(Buf* out, u32 c){
    u32 a = c >> 24;
    if(a == 255) buf_printf(out, "rgb(%u, %u, %u)", (c >> 16) & 255, (c >> 8) & 255, c & 255);
    else buf_printf(out, "rgba(%u, %u, %u, %u.%02u)", (c >> 16) & 255, (c >> 8) & 255, c & 255, a / 255, a * 100 / 255 % 100);
}
void css_computed(Node* n, const char* prop, Buf* out){
    Style* s = n ? n->cs : NULL;
    static const char* DISP[] = {"none","inline","block","inline-block","list-item","flex","inline-flex","grid",
        "inline-grid","table","inline-table","table-row-group","table-row","table-cell","table-caption","contents"};
    if(!strcmp(prop, "display")){ buf_puts(out, s ? DISP[s->display] : "none"); return; }
    if(!s) return;
    if(!strcmp(prop, "color")) put_color(out, s->color);
    else if(!strcmp(prop, "background-color")) put_color(out, s->bg);
    else if(!strcmp(prop, "font-size")) buf_printf(out, "%dpx", s->font_size);
    else if(!strcmp(prop, "font-weight")) buf_printf(out, "%d", s->weight ? s->weight : 400);
    else if(!strcmp(prop, "font-style")) buf_puts(out, s->italic ? "italic" : "normal");
    else if(!strcmp(prop, "visibility")) buf_puts(out, s->visibility ? "visible" : "hidden");
    else if(!strcmp(prop, "position")){ static const char* P[] = {"static","relative","absolute","fixed","sticky"}; buf_puts(out, P[s->position]); }
    else if(!strcmp(prop, "width")) buf_printf(out, "%dpx", n->bw);
    else if(!strcmp(prop, "height")) buf_printf(out, "%dpx", n->bh);
    else if(!strcmp(prop, "opacity")) buf_printf(out, "%d", s->opacity >= 255 ? 1 : 0);
    else if(!strcmp(prop, "text-align")){ static const char* T[] = {"left","right","center","justify"}; buf_puts(out, T[s->text_align]); }
    else if(!strcmp(prop, "overflow")) buf_puts(out, s->overflow_hidden ? "hidden" : "visible");
    else if(!strcmp(prop, "line-height")) buf_printf(out, "%dpx", s->line_height > 0 ? s->line_height : s->font_size * 6 / 5);
    else if(!strncmp(prop, "--", 2)){ const char* v = var_lookup(s, prop, (int)strlen(prop)); if(v) buf_puts(out, v); }
}
