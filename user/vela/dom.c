/* dom.c — arvore DOM e parser HTML tolerante.
 *
 * O parser segue as regras do HTML5 que mais importam na pratica:
 * html/head/body implicitos, tags vazias, fechamento implicito (p, li, dd,
 * td, tr, option...), texto bruto em script/style/textarea/title,
 * entidades nomeadas e numericas. Cada nodo tem um id inteiro, que e como
 * o JavaScript se refere a ele. */
#include "vela.h"

Doc* doc = NULL;

/* -------------------------------------------------------------- nodos */
Doc* doc_new(const char* url){
    Doc* d = (Doc*)calloc(1, sizeof *d);
    snprintf(d->url, sizeof d->url, "%s", url ? url : "about:blank");
    d->root = dom_new(d, N_DOC, "#document");
    return d;
}
void doc_free(Doc* d){
    if(!d) return;
    arena_reset(&d->arena);
    free(d->byid);
    free(d);
}
Node* dom_new(Doc* d, int type, const char* tag){
    Node* n = (Node*)arena_alloc(&d->arena, sizeof(Node));
    n->type = (u8)type;
    if(tag){
        char t[64]; int k = 0;
        for(; tag[k] && k < 63; k++) t[k] = (char)lower((u8)tag[k]);
        t[k] = 0;
        n->tag = arena_strdup(&d->arena, t);
    }
    if(d->nnodes == d->cnodes){
        d->cnodes = d->cnodes ? d->cnodes * 2 : 1024;
        d->byid = (Node**)realloc(d->byid, sizeof(Node*) * (size_t)d->cnodes);
    }
    n->id = d->nnodes + 1;
    d->byid[d->nnodes++] = n;
    return n;
}
Node* dom_new_text(Doc* d, const char* s, int n){
    Node* t = dom_new(d, N_TEXT, NULL);
    t->text = arena_strndup(&d->arena, s, (size_t)n); t->tlen = n;
    return t;
}
Node* dom_get(Doc* d, int id){ return d && id > 0 && id <= d->nnodes ? d->byid[id - 1] : NULL; }

void dom_remove(Node* c){
    Node* p = c->parent;
    if(!p) return;
    if(c->prev) c->prev->next = c->next; else p->first = c->next;
    if(c->next) c->next->prev = c->prev; else p->last = c->prev;
    c->parent = c->prev = c->next = NULL;
}
void dom_insert(Node* p, Node* c, Node* before){
    if(before && before->parent != p) before = NULL;
    if(c == before) return;
    for(Node* a = p; a; a = a->parent) if(a == c) return;   /* ciclo */
    if(c->type == N_FRAG){
        while(c->first){ Node* k = c->first; dom_remove(k); dom_insert(p, k, before); }
        return;
    }
    dom_remove(c);
    c->parent = p;
    if(before){
        c->next = before; c->prev = before->prev;
        if(before->prev) before->prev->next = c; else p->first = c;
        before->prev = c;
    } else {
        c->prev = p->last; c->next = NULL;
        if(p->last) p->last->next = c; else p->first = c;
        p->last = c;
    }
}
void dom_append(Node* p, Node* c){ dom_insert(p, c, NULL); }

const char* dom_attr(const Node* n, const char* name){
    if(!n || n->type != N_ELEM) return NULL;
    for(int i = 0; i < n->nattr; i++) if(strcmp(n->attrs[i].name, name) == 0) return n->attrs[i].value;
    return NULL;
}
void dom_set_attr(Doc* d, Node* n, const char* name, const char* value){
    char lname[64]; int k = 0;
    for(; name[k] && k < 63; k++) lname[k] = (char)lower((u8)name[k]);
    lname[k] = 0;
    for(int i = 0; i < n->nattr; i++)
        if(strcmp(n->attrs[i].name, lname) == 0){ n->attrs[i].value = arena_strdup(&d->arena, value); return; }
    if(n->nattr == n->cattr){
        int c = n->cattr ? n->cattr * 2 : 4;
        Attr* a = (Attr*)arena_alloc(&d->arena, sizeof(Attr) * (size_t)c);
        if(n->nattr) memcpy(a, n->attrs, sizeof(Attr) * (size_t)n->nattr);
        n->attrs = a; n->cattr = c;
    }
    n->attrs[n->nattr].name = arena_strdup(&d->arena, lname);
    n->attrs[n->nattr].value = arena_strdup(&d->arena, value);
    n->nattr++;
}
void dom_del_attr(Node* n, const char* name){
    for(int i = 0; i < n->nattr; i++)
        if(str_ieq(n->attrs[i].name, name)){
            memmove(n->attrs + i, n->attrs + i + 1, sizeof(Attr) * (size_t)(n->nattr - i - 1));
            n->nattr--; return;
        }
}
int dom_is(const Node* n, const char* tag){ return n && n->type == N_ELEM && strcmp(n->tag, tag) == 0; }

Node* dom_next_in_tree(Node* n, Node* stop){
    if(n->first) return n->first;
    while(n && n != stop){
        if(n->next) return n->next;
        n = n->parent;
    }
    return NULL;
}
Node* dom_find(Node* root, const char* tag){
    for(Node* n = root; n; n = dom_next_in_tree(n, root)) if(dom_is(n, tag)) return n;
    return NULL;
}
Node* dom_by_id(Node* root, const char* id){
    for(Node* n = root; n; n = dom_next_in_tree(n, root)){
        const char* v = dom_attr(n, "id");
        if(v && strcmp(v, id) == 0) return n;
    }
    return NULL;
}
Node* dom_body(Doc* d){
    Node* html = dom_find(d->root, "html");
    if(!html) return NULL;
    for(Node* c = html->first; c; c = c->next) if(dom_is(c, "body") || dom_is(c, "frameset")) return c;
    return NULL;
}
Node* dom_head(Doc* d){
    Node* html = dom_find(d->root, "html");
    for(Node* c = html ? html->first : NULL; c; c = c->next) if(dom_is(c, "head")) return c;
    return NULL;
}

void dom_text_content(const Node* n, Buf* out){
    if(n->type == N_TEXT || n->type == N_COMMENT){ buf_put(out, n->text, n->tlen); return; }
    for(Node* c = n->first; c; c = c->next) if(c->type != N_COMMENT) dom_text_content(c, out);
}
void dom_set_text(Doc* d, Node* n, const char* s, int len){
    if(n->type == N_TEXT || n->type == N_COMMENT){
        n->text = arena_strndup(&d->arena, s, (size_t)len); n->tlen = len; return;
    }
    while(n->first) dom_remove(n->first);
    if(len > 0) dom_append(n, dom_new_text(d, s, len));
}

Node* dom_clone(Doc* d, Node* n, int deep){
    Node* c = dom_new(d, n->type, n->tag);
    if(n->text){ c->text = arena_strndup(&d->arena, n->text, (size_t)n->tlen); c->tlen = n->tlen; }
    for(int i = 0; i < n->nattr; i++) dom_set_attr(d, c, n->attrs[i].name, n->attrs[i].value);
    if(n->value) c->value = arena_strdup(&d->arena, n->value);
    c->checked = n->checked;
    if(deep) for(Node* k = n->first; k; k = k->next) dom_append(c, dom_clone(d, k, 1));
    return c;
}

/* ------------------------------------------------------- serializacao */
static void escape(Buf* b, const char* s, int n, int attr){
    for(int i = 0; i < n; i++){
        char c = s[i];
        if(c == '&') buf_puts(b, "&amp;");
        else if(c == '<' && !attr) buf_puts(b, "&lt;");
        else if(c == '>' && !attr) buf_puts(b, "&gt;");
        else if(c == '"' && attr) buf_puts(b, "&quot;");
        else if((u8)c == 0xC2 && i + 1 < n && (u8)s[i + 1] == 0xA0){ buf_puts(b, "&nbsp;"); i++; }
        else buf_putc(b, c);
    }
}
static int is_raw(const char* t){
    return !strcmp(t, "script") || !strcmp(t, "style") || !strcmp(t, "xmp") || !strcmp(t, "iframe")
        || !strcmp(t, "noembed") || !strcmp(t, "noframes") || !strcmp(t, "noscript");
}
void dom_serialize(const Node* n, Buf* b, int outer){
    if(n->type == N_TEXT){
        if(n->parent && n->parent->type == N_ELEM && is_raw(n->parent->tag)) buf_put(b, n->text, n->tlen);
        else escape(b, n->text, n->tlen, 0);
        return;
    }
    if(n->type == N_COMMENT){ buf_puts(b, "<!--"); buf_put(b, n->text, n->tlen); buf_puts(b, "-->"); return; }
    if(outer && n->type == N_ELEM){
        buf_putc(b, '<'); buf_puts(b, n->tag);
        for(int i = 0; i < n->nattr; i++){
            buf_putc(b, ' '); buf_puts(b, n->attrs[i].name); buf_puts(b, "=\"");
            escape(b, n->attrs[i].value, (int)strlen(n->attrs[i].value), 1); buf_putc(b, '"');
        }
        buf_putc(b, '>');
        if(html_is_void(n->tag)) return;
    }
    for(Node* c = n->first; c; c = c->next) dom_serialize(c, b, 1);
    if(outer && n->type == N_ELEM){ buf_puts(b, "</"); buf_puts(b, n->tag); buf_putc(b, '>'); }
}

/* ----------------------------------------------------------- entidades */
static const struct { const char* name; u16 cp; } ENT[] = {
    {"amp",'&'},{"lt",'<'},{"gt",'>'},{"quot",'"'},{"apos",'\''},{"nbsp",0xA0},{"iexcl",0xA1},{"cent",0xA2},
    {"pound",0xA3},{"curren",0xA4},{"yen",0xA5},{"brvbar",0xA6},{"sect",0xA7},{"uml",0xA8},{"copy",0xA9},
    {"ordf",0xAA},{"laquo",0xAB},{"not",0xAC},{"shy",0xAD},{"reg",0xAE},{"macr",0xAF},{"deg",0xB0},
    {"plusmn",0xB1},{"sup2",0xB2},{"sup3",0xB3},{"acute",0xB4},{"micro",0xB5},{"para",0xB6},{"middot",0xB7},
    {"cedil",0xB8},{"sup1",0xB9},{"ordm",0xBA},{"raquo",0xBB},{"frac14",0xBC},{"frac12",0xBD},{"frac34",0xBE},
    {"iquest",0xBF},{"Agrave",0xC0},{"Aacute",0xC1},{"Acirc",0xC2},{"Atilde",0xC3},{"Auml",0xC4},{"Aring",0xC5},
    {"AElig",0xC6},{"Ccedil",0xC7},{"Egrave",0xC8},{"Eacute",0xC9},{"Ecirc",0xCA},{"Euml",0xCB},{"Igrave",0xCC},
    {"Iacute",0xCD},{"Icirc",0xCE},{"Iuml",0xCF},{"ETH",0xD0},{"Ntilde",0xD1},{"Ograve",0xD2},{"Oacute",0xD3},
    {"Ocirc",0xD4},{"Otilde",0xD5},{"Ouml",0xD6},{"times",0xD7},{"Oslash",0xD8},{"Ugrave",0xD9},{"Uacute",0xDA},
    {"Ucirc",0xDB},{"Uuml",0xDC},{"Yacute",0xDD},{"THORN",0xDE},{"szlig",0xDF},{"agrave",0xE0},{"aacute",0xE1},
    {"acirc",0xE2},{"atilde",0xE3},{"auml",0xE4},{"aring",0xE5},{"aelig",0xE6},{"ccedil",0xE7},{"egrave",0xE8},
    {"eacute",0xE9},{"ecirc",0xEA},{"euml",0xEB},{"igrave",0xEC},{"iacute",0xED},{"icirc",0xEE},{"iuml",0xEF},
    {"eth",0xF0},{"ntilde",0xF1},{"ograve",0xF2},{"oacute",0xF3},{"ocirc",0xF4},{"otilde",0xF5},{"ouml",0xF6},
    {"divide",0xF7},{"oslash",0xF8},{"ugrave",0xF9},{"uacute",0xFA},{"ucirc",0xFB},{"uuml",0xFC},{"yacute",0xFD},
    {"thorn",0xFE},{"yuml",0xFF},{"OElig",0x152},{"oelig",0x153},{"Scaron",0x160},{"scaron",0x161},{"Yuml",0x178},
    {"fnof",0x192},{"circ",0x2C6},{"tilde",0x2DC},{"Alpha",0x391},{"Beta",0x392},{"Gamma",0x393},{"Delta",0x394},
    {"Omega",0x3A9},{"alpha",0x3B1},{"beta",0x3B2},{"gamma",0x3B3},{"delta",0x3B4},{"epsilon",0x3B5},{"lambda",0x3BB},
    {"mu",0x3BC},{"pi",0x3C0},{"sigma",0x3C3},{"tau",0x3C4},{"phi",0x3C6},{"omega",0x3C9},{"ensp",0x2002},
    {"emsp",0x2003},{"thinsp",0x2009},{"zwnj",0x200C},{"zwj",0x200D},{"lrm",0x200E},{"rlm",0x200F},{"ndash",0x2013},
    {"mdash",0x2014},{"lsquo",0x2018},{"rsquo",0x2019},{"sbquo",0x201A},{"ldquo",0x201C},{"rdquo",0x201D},
    {"bdquo",0x201E},{"dagger",0x2020},{"Dagger",0x2021},{"bull",0x2022},{"hellip",0x2026},{"permil",0x2030},
    {"prime",0x2032},{"Prime",0x2033},{"lsaquo",0x2039},{"rsaquo",0x203A},{"oline",0x203E},{"frasl",0x2044},
    {"euro",0x20AC},{"trade",0x2122},{"larr",0x2190},{"uarr",0x2191},{"rarr",0x2192},{"darr",0x2193},{"harr",0x2194},
    {"lArr",0x21D0},{"rArr",0x21D2},{"hArr",0x21D4},{"forall",0x2200},{"part",0x2202},{"exist",0x2203},{"empty",0x2205},
    {"nabla",0x2207},{"isin",0x2208},{"notin",0x2209},{"prod",0x220F},{"sum",0x2211},{"minus",0x2212},{"radic",0x221A},
    {"infin",0x221E},{"and",0x2227},{"or",0x2228},{"cap",0x2229},{"cup",0x222A},{"int",0x222B},{"asymp",0x2248},
    {"ne",0x2260},{"equiv",0x2261},{"le",0x2264},{"ge",0x2265},{"sub",0x2282},{"sup",0x2283},{"sdot",0x22C5},
    {"loz",0x25CA},{"spades",0x2660},{"clubs",0x2663},{"hearts",0x2665},{"diams",0x2666},{"check",0x2713},
    {"star",0x2606},{"starf",0x2605},
};

/* decodifica &...; em s[*i] (s[*i] == '&'); escreve UTF-8 em out; 0 se nao for entidade */
static int decode_entity(const char* s, int n, int* i, char* out){
    int j = *i + 1;
    u32 cp = 0;
    if(j < n && s[j] == '#'){
        j++;
        int hex = j < n && (s[j] == 'x' || s[j] == 'X');
        if(hex) j++;
        int start = j;
        while(j < n && j - start < 8){
            int c = lower((u8)s[j]);
            if(c >= '0' && c <= '9') cp = cp * (hex ? 16 : 10) + (u32)(c - '0');
            else if(hex && c >= 'a' && c <= 'f') cp = cp * 16 + (u32)(c - 'a' + 10);
            else break;
            j++;
        }
        if(j == start) return 0;
        if(j < n && s[j] == ';') j++;
        if(cp == 0 || cp > 0x10FFFF || (cp >= 0xD800 && cp < 0xE000)) cp = 0xFFFD;
        if(cp >= 0x80 && cp < 0xA0){               /* windows-1252 nos numericos */
            static const u16 w[32] = {0x20AC,0x81,0x201A,0x192,0x201E,0x2026,0x2020,0x2021,0x2C6,0x2030,0x160,0x2039,0x152,0x8D,0x17D,0x8F,
                0x90,0x2018,0x2019,0x201C,0x201D,0x2022,0x2013,0x2014,0x2DC,0x2122,0x161,0x203A,0x153,0x9D,0x17E,0x178};
            cp = w[cp - 0x80];
        }
    } else {
        int start = j;
        while(j < n && j - start < 10 && ((s[j] >= 'a' && s[j] <= 'z') || (s[j] >= 'A' && s[j] <= 'Z') || (s[j] >= '0' && s[j] <= '9'))) j++;
        int len = j - start;
        if(!len) return 0;
        for(size_t k = 0; k < sizeof ENT / sizeof ENT[0]; k++){
            if((int)strlen(ENT[k].name) == len && memcmp(ENT[k].name, s + start, (size_t)len) == 0){ cp = ENT[k].cp; break; }
        }
        if(!cp) return 0;
        if(j < n && s[j] == ';') j++;
        else if(j < n && s[j] == '=') return 0;         /* "?a=1&times=2" em URLs */
    }
    *i = j;
    return utf8_encode(cp, out);
}

static void put_decoded(Buf* b, const char* s, int n){
    for(int i = 0; i < n;){
        if(s[i] == '&'){
            char u[4]; int k = decode_entity(s, n, &i, u);
            if(k){ buf_put(b, u, k); continue; }
        }
        buf_putc(b, s[i++]);
    }
}

/* -------------------------------------------------------------- parser */
static const char* VOID_TAGS[] = { "area", "base", "br", "col", "embed", "hr", "img", "input", "link",
    "meta", "param", "source", "track", "wbr", "keygen", "basefont", "frame", NULL };
int html_is_void(const char* t){
    for(int i = 0; VOID_TAGS[i]; i++) if(!strcmp(t, VOID_TAGS[i])) return 1;
    return 0;
}
static int in_list(const char* t, const char* const* l){
    for(; *l; l++) if(!strcmp(t, *l)) return 1;
    return 0;
}
static const char* const CLOSES_P[] = { "address", "article", "aside", "blockquote", "center", "details",
    "dialog", "dir", "div", "dl", "fieldset", "figcaption", "figure", "footer", "form", "h1", "h2", "h3",
    "h4", "h5", "h6", "header", "hgroup", "hr", "main", "menu", "nav", "ol", "p", "pre", "section",
    "summary", "table", "ul", "li", "dd", "dt", "listing", "xmp", "search", NULL };
static const char* const HEAD_TAGS[] = { "meta", "link", "title", "style", "script", "base", "noscript",
    "template", NULL };
static const char* const SCOPE_STOP[] = { "table", "td", "th", "caption", "button", "html", "marquee",
    "object", "template", NULL };

#define MAXDEPTH 400
typedef struct {
    Doc* d;
    const char* s; int n, i;
    Node* st[MAXDEPTH]; int sp;
    Node *html, *head, *body;
    int frag;                    /* analisando fragmento (innerHTML) */
    Node* before;                /* fragmento: insere antes deste nodo */
    Buf text;
} P;

static Node* cur(P* p){ return p->st[p->sp - 1]; }

static void ensure_body(P* p){
    if(p->frag || p->body) return;
    p->body = dom_new(p->d, N_ELEM, "body");
    dom_append(p->html, p->body);
    p->sp = 1; p->st[p->sp++] = p->body;
}
static Node* ensure_head(P* p){
    if(!p->head){
        p->head = dom_new(p->d, N_ELEM, "head");
        dom_insert(p->html, p->head, p->html->first);
    }
    return p->head;
}

static void insert_node(P* p, Node* parent, Node* c){
    if(p->frag && parent == p->st[0] && p->before) dom_insert(parent, c, p->before);
    else dom_append(parent, c);
}

static void flush_text(P* p){
    if(!p->text.n) return;
    int allws = 1;
    for(int k = 0; k < p->text.n; k++) if(!is_ws((u8)p->text.p[k])){ allws = 0; break; }
    if(!p->frag && !p->body){
        if(allws){ p->text.n = 0; return; }
        ensure_body(p);
    }
    Node* parent = cur(p);
    Node* last = (p->frag && parent == p->st[0] && p->before) ? p->before->prev : parent->last;
    if(last && last->type == N_TEXT){
        char* t = (char*)arena_alloc(&p->d->arena, (size_t)(last->tlen + p->text.n + 1));
        memcpy(t, last->text, (size_t)last->tlen); memcpy(t + last->tlen, p->text.p, (size_t)p->text.n);
        last->text = t; last->tlen += p->text.n;
    } else insert_node(p, parent, dom_new_text(p->d, p->text.p, p->text.n));
    p->text.n = 0;
}

static void pop_to(P* p, int idx){ if(idx >= 1) p->sp = idx; }   /* remove st[idx..] */

/* procura `tag` aberta (de cima para baixo), sem atravessar os limites */
static int find_open(P* p, const char* tag, const char* const* stops){
    for(int k = p->sp - 1; k >= 1; k--){
        if(!strcmp(p->st[k]->tag, tag)) return k;
        if(stops && in_list(p->st[k]->tag, stops)) return -1;
    }
    return -1;
}

static void close_p(P* p){
    int k = find_open(p, "p", SCOPE_STOP);
    if(k > 0) pop_to(p, k);
}

static void start_tag(P* p, const char* tag, Attr* attrs, int na, int selfclose){
    if(!strcmp(tag, "html")){
        if(!p->frag) for(int k = 0; k < na; k++) if(!dom_attr(p->html, attrs[k].name)) dom_set_attr(p->d, p->html, attrs[k].name, attrs[k].value);
        return;
    }
    if(!strcmp(tag, "head")){ if(!p->frag) ensure_head(p); return; }
    if(!strcmp(tag, "body") || !strcmp(tag, "frameset")){
        if(p->frag) return;
        ensure_body(p);
        for(int k = 0; k < na; k++) if(!dom_attr(p->body, attrs[k].name)) dom_set_attr(p->d, p->body, attrs[k].name, attrs[k].value);
        return;
    }
    Node* parent;
    if(!p->frag && !p->body && in_list(tag, HEAD_TAGS)) parent = ensure_head(p);
    else {
        ensure_body(p);
        /* fechamentos implicitos */
        if(in_list(tag, CLOSES_P)) close_p(p);
        if(!strcmp(tag, "li")){
            for(int k = p->sp - 1; k >= 1; k--){
                const char* t = p->st[k]->tag;
                if(!strcmp(t, "li")){ pop_to(p, k); break; }
                if(!strcmp(t, "ul") || !strcmp(t, "ol") || !strcmp(t, "menu") || in_list(t, SCOPE_STOP)) break;
            }
        } else if(!strcmp(tag, "dd") || !strcmp(tag, "dt")){
            for(int k = p->sp - 1; k >= 1; k--){
                const char* t = p->st[k]->tag;
                if(!strcmp(t, "dd") || !strcmp(t, "dt")){ pop_to(p, k); break; }
                if(!strcmp(t, "dl") || in_list(t, SCOPE_STOP)) break;
            }
        } else if(!strcmp(tag, "option")){
            if(dom_is(cur(p), "option")) p->sp--;
        } else if(!strcmp(tag, "optgroup")){
            if(dom_is(cur(p), "option")) p->sp--;
            if(dom_is(cur(p), "optgroup")) p->sp--;
        } else if(!strcmp(tag, "tr")){
            int k = find_open(p, "table", NULL);
            if(k > 0){
                while(p->sp - 1 > k && !dom_is(cur(p), "tbody") && !dom_is(cur(p), "thead") && !dom_is(cur(p), "tfoot")) p->sp--;
            }
        } else if(!strcmp(tag, "td") || !strcmp(tag, "th")){
            int k = find_open(p, "table", NULL);
            if(k > 0){
                for(int j = p->sp - 1; j > k; j--)
                    if(dom_is(p->st[j], "td") || dom_is(p->st[j], "th")){ pop_to(p, j); break; }
                Node* c = cur(p);
                if(dom_is(c, "table") || dom_is(c, "tbody") || dom_is(c, "thead") || dom_is(c, "tfoot")){
                    Node* tr = dom_new(p->d, N_ELEM, "tr");
                    insert_node(p, c, tr);
                    if(p->sp < MAXDEPTH) p->st[p->sp++] = tr;
                }
            }
        } else if(!strcmp(tag, "tbody") || !strcmp(tag, "thead") || !strcmp(tag, "tfoot")){
            int k = find_open(p, "table", NULL);
            if(k > 0) pop_to(p, k + 1);
        } else if(!strcmp(tag, "a")){
            int k = find_open(p, "a", SCOPE_STOP);
            if(k > 0) pop_to(p, k);
        } else if(tag[0] == 'h' && tag[1] >= '1' && tag[1] <= '6' && !tag[2]){
            const char* t = cur(p)->tag;
            if(t && t[0] == 'h' && t[1] >= '1' && t[1] <= '6' && !t[2]) p->sp--;
        } else if(!strcmp(tag, "button")){
            int k = find_open(p, "button", SCOPE_STOP);
            if(k > 0) pop_to(p, k);
        }
        parent = cur(p);
    }
    Node* e = dom_new(p->d, N_ELEM, tag);
    for(int k = 0; k < na; k++) dom_set_attr(p->d, e, attrs[k].name, attrs[k].value);
    insert_node(p, parent, e);
    if(html_is_void(tag) || selfclose) return;
    if(parent == p->head) return;              /* conteudo do head: texto bruto abaixo */
    if(p->sp < MAXDEPTH) p->st[p->sp++] = e;
}

static void end_tag(P* p, const char* tag){
    if(!strcmp(tag, "html") || !strcmp(tag, "body") || !strcmp(tag, "head")) return;
    if(!strcmp(tag, "br")){ start_tag(p, "br", NULL, 0, 1); return; }
    if(!strcmp(tag, "p")){ close_p(p); return; }
    int table_like = !strcmp(tag, "table") || !strcmp(tag, "tr") || !strcmp(tag, "td") || !strcmp(tag, "th")
        || !strcmp(tag, "tbody") || !strcmp(tag, "thead") || !strcmp(tag, "tfoot") || !strcmp(tag, "caption");
    static const char* const STOPS[] = { "table", "td", "th", "caption", "html", NULL };
    int k = find_open(p, tag, table_like ? NULL : STOPS);
    if(k > 0) pop_to(p, k);
}

/* le o conteudo de um elemento de texto bruto ate </tag> */
static void raw_text(P* p, Node* e, int rcdata){
    const char* s = p->s; int n = p->n, i = p->i;
    int tl = (int)strlen(e->tag);
    int end = n, next = n;
    for(int k = i; k + 1 + tl < n; k++){
        if(s[k] == '<' && s[k + 1] == '/' && str_istarts(s + k + 2, e->tag)){
            char c = k + 2 + tl < n ? s[k + 2 + tl] : '>';
            if(c == '>' || is_ws((u8)c) || c == '/'){
                end = k;
                next = k + 2 + tl;
                while(next < n && s[next] != '>') next++;
                if(next < n) next++;
                break;
            }
        }
    }
    if(end > i){
        if(rcdata){ Buf b = {0}; put_decoded(&b, s + i, end - i); dom_append(e, dom_new_text(p->d, b.p ? b.p : "", b.n)); buf_free(&b); }
        else dom_append(e, dom_new_text(p->d, s + i, end - i));
    }
    p->i = next;
}

static void parse(P* p){
    const char* s = p->s; int n = p->n;
    Attr attrs[64];
    while(p->i < n){
        int i = p->i;
        char c = s[i];
        if(c != '<'){
            if(c == '&'){
                char u[4]; int k = decode_entity(s, n, &p->i, u);
                if(k){ buf_put(&p->text, u, k); continue; }
            }
            if(c == '\r'){ if(i + 1 < n && s[i + 1] == '\n'){ p->i++; continue; } c = '\n'; }
            buf_putc(&p->text, c ? c : ' ');
            p->i++;
            continue;
        }
        /* comentario, doctype, instrucao */
        if(i + 3 < n && s[i + 1] == '!' && s[i + 2] == '-' && s[i + 3] == '-'){
            flush_text(p);
            int j = i + 4;
            while(j + 2 < n && !(s[j] == '-' && s[j + 1] == '-' && s[j + 2] == '>')) j++;
            if(p->body || p->frag){
                Node* cm = dom_new(p->d, N_COMMENT, NULL);
                cm->text = arena_strndup(&p->d->arena, s + i + 4, (size_t)(j - i - 4 > 0 ? j - i - 4 : 0)); cm->tlen = (int)strlen(cm->text);
                insert_node(p, cur(p), cm);
            }
            p->i = j + 3 < n ? j + 3 : n;
            continue;
        }
        if(i + 1 < n && (s[i + 1] == '!' || s[i + 1] == '?')){
            flush_text(p);
            int j = i + 2; while(j < n && s[j] != '>') j++;
            p->i = j + 1;
            continue;
        }
        int close = i + 1 < n && s[i + 1] == '/';
        int j = i + 1 + close;
        if(j >= n || !((s[j] >= 'a' && s[j] <= 'z') || (s[j] >= 'A' && s[j] <= 'Z'))){
            if(close){                      /* "</ >" ou "</3": ignora ate '>' */
                while(j < n && s[j] != '>') j++;
                p->i = j + 1; continue;
            }
            buf_putc(&p->text, '<'); p->i++;
            continue;
        }
        flush_text(p);
        char tag[64]; int tn = 0;
        while(j < n && !is_ws((u8)s[j]) && s[j] != '>' && s[j] != '/'){ if(tn < 63) tag[tn++] = (char)lower((u8)s[j]); j++; }
        tag[tn] = 0;
        /* atributos */
        int na = 0, selfclose = 0;
        Buf vb = {0};
        for(;;){
            while(j < n && (is_ws((u8)s[j]) || (s[j] == '/' && !(j + 1 < n && s[j + 1] == '>')))) j++;
            if(j >= n) break;
            if(s[j] == '>'){ j++; break; }
            if(s[j] == '/' && j + 1 < n && s[j + 1] == '>'){ selfclose = 1; j += 2; break; }
            int a0 = j;
            while(j < n && !is_ws((u8)s[j]) && s[j] != '=' && s[j] != '>' && !(s[j] == '/' && j + 1 < n && s[j + 1] == '>')) j++;
            if(j == a0){ j++; continue; }
            const char* name = arena_strndup(&p->d->arena, s + a0, (size_t)(j - a0));
            while(j < n && is_ws((u8)s[j])) j++;
            const char* value = "";
            if(j < n && s[j] == '='){
                j++;
                while(j < n && is_ws((u8)s[j])) j++;
                int v0, v1;
                if(j < n && (s[j] == '"' || s[j] == '\'')){
                    char q = s[j++]; v0 = j;
                    while(j < n && s[j] != q) j++;
                    v1 = j; if(j < n) j++;
                } else {
                    v0 = j;
                    while(j < n && !is_ws((u8)s[j]) && s[j] != '>') j++;
                    v1 = j;
                }
                vb.n = 0; put_decoded(&vb, s + v0, v1 - v0);
                value = arena_strndup(&p->d->arena, vb.p ? vb.p : "", (size_t)vb.n);
            }
            if(na < 64 && !close){
                char* ln = (char*)name; for(int k = 0; ln[k]; k++) ln[k] = (char)lower((u8)ln[k]);
                int dup = 0; for(int k = 0; k < na; k++) if(!strcmp(attrs[k].name, name)) dup = 1;
                if(!dup){ attrs[na].name = name; attrs[na].value = value; na++; }
            }
        }
        buf_free(&vb);
        p->i = j;
        if(close){ end_tag(p, tag); continue; }
        start_tag(p, tag, attrs, na, selfclose);
        /* elementos de texto bruto */
        int raw = !strcmp(tag, "script") || !strcmp(tag, "style") || !strcmp(tag, "xmp") || !strcmp(tag, "iframe")
            || !strcmp(tag, "noembed") || !strcmp(tag, "noframes") || !strcmp(tag, "noscript");
        int rc = !strcmp(tag, "textarea") || !strcmp(tag, "title");
        if((raw || rc) && !selfclose){
            /* o elemento recem-criado e o ultimo filho do pai em uso */
            Node* e = NULL;
            Node* par = (!p->frag && !p->body) ? p->head : cur(p);
            if(par && dom_is(par, tag)) e = par;            /* ja empilhado */
            else if(par){
                Node* last = (p->frag && par == p->st[0] && p->before) ? p->before->prev : par->last;
                if(last && dom_is(last, tag)) e = last;
            }
            if(e){
                raw_text(p, e, rc);
                if(p->sp > 1 && cur(p) == e) p->sp--;
            }
        }
    }
    flush_text(p);
}

Doc* html_parse(const char* src, int len, const char* url){
    Doc* d = doc_new(url);
    /* modo quirks: o documento nao comeca com <!doctype html> */
    {
        int i = 0;
        if(len >= 3 && (u8)src[0] == 0xEF && (u8)src[1] == 0xBB && (u8)src[2] == 0xBF) i = 3;
        for(;;){
            while(i < len && is_ws((u8)src[i])) i++;
            if(i + 4 < len && !memcmp(src + i, "<!--", 4)){ const char* e = strstr(src + i, "-->"); if(!e) break; i = (int)(e - src) + 3; continue; }
            break;
        }
        d->quirks = !(i + 14 <= len && str_istarts(src + i, "<!doctype html"));
    }
    P* p = (P*)calloc(1, sizeof(P));
    p->d = d; p->s = src; p->n = len;
    p->html = dom_new(d, N_ELEM, "html");
    dom_append(d->root, p->html);
    p->st[0] = p->html; p->sp = 1;
    parse(p);
    ensure_head(p);
    ensure_body(p);
    buf_free(&p->text);
    free(p);
    /* <title> e <base href> */
    Node* t = dom_find(d->root, "title");
    if(t){
        Buf b = {0}; dom_text_content(t, &b);
        int k = 0, sp = 1;
        for(int i = 0; i < b.n && k < 250; i++){
            char c = b.p[i];
            if(is_ws((u8)c)){ if(!sp) d->title[k++] = ' '; sp = 1; }
            else { d->title[k++] = c; sp = 0; }
        }
        while(k > 0 && d->title[k - 1] == ' ') k--;
        d->title[k] = 0;
        buf_free(&b);
    }
    Node* base = dom_find(d->root, "base");
    const char* bh = base ? dom_attr(base, "href") : NULL;
    if(bh && *bh){ char tmp[URLMAX]; if(url_resolve(d->url, bh, tmp, sizeof tmp)) snprintf(d->url, sizeof d->url, "%s", tmp); }
    return d;
}

void html_parse_into(Doc* d, Node* parent, Node* before, const char* src, int len){
    P* p = (P*)calloc(1, sizeof(P));
    p->d = d; p->s = src; p->n = len; p->frag = 1; p->before = before;
    p->st[0] = parent; p->sp = 1;
    parse(p);
    buf_free(&p->text);
    free(p);
    d->dirty = 1;
}
