/* main.c — Vela, o navegador do Pulsar OS (C).
 *
 * Interface (barra de endereco, rolagem), o laco de eventos e a sequencia
 * de carga de uma pagina:
 *   HTTP(S) -> parser HTML -> folhas de estilo (todas em paralelo) ->
 *   primeira pintura -> scripts (pre-baixados em paralelo, executados um
 *   por volta do laco, sem travar a interface) -> DOMContentLoaded/load.
 *   Imagens chegam em lotes paralelos no tempo ocioso; timers no laco.
 * Toda navegacao pedida durante um script (location.href = ...) e adiada
 * para o laco, pois o runtime do JS e destruido ao trocar de pagina. */
#include "vela.h"

#define TOP 56
#define SCROLLBAR 10
#define HIST 32
#define MAX_SHEETS 40
#define MAX_SCRIPTS 80

static App app;
static DList dl;
static char addr[URLMAX] = "";          /* texto da barra de endereco */
static int addr_n = 0, addr_cur = 0, editing = 0, addr_all = 0;   /* addr_all: tudo selecionado */
static char shown[URLMAX] = "";         /* URL da pagina exibida */
static char hist[HIST][URLMAX];
static int nhist = 0;
i32 view_w = 800, view_h = 600, scroll_y = 0;
static int need_layout = 0;
static u64 last_tick = 0, last_layout = 0, layout_cost = 0;
static int loading = 0;                 /* pagina ainda chegando (linha sob a barra) */
static int page_dirty = 1;              /* o conteudo precisa ser repintado */
static char bubble[URLMAX];             /* balao no canto: link sob o mouse ou aviso */
static Rect bubble_r;                   /* onde o balao foi desenhado (para apagar) */
static char notice[256];                /* aviso temporario (alert(), endereco invalido) */
static u64 notice_until = 0;

/* navegacao pendente (pedida por clique, formulario ou JS) */
static struct { int on; char url[URLMAX]; char method[8]; char* body; int blen; char ctype[96]; } pend;

static void draw(i32 mx, i32 my);
static void relayout(void);
static int sync_styles(void);

/* ------------------------------------------------------- utilidades da UI */
void vela_status(const char* m){
    snprintf(notice, sizeof notice, "%s", m);
    notice_until = sys_time_ms() + 5000;
}
const char* vela_url(void){ return shown[0] ? shown : "about:blank"; }
void vela_set_url(const char* u){
    snprintf(shown, sizeof shown, "%s", u);
    if(!editing){ snprintf(addr, sizeof addr, "%s", shown); addr_n = addr_cur = (int)strlen(addr); }
}
void vela_title_changed(void){
    char t[64];
    if(doc && doc->title[0]) snprintf(t, sizeof t, "Vela: %s", doc->title); else snprintf(t, sizeof t, "Vela");
    app_title(&app, t);
}
void vela_scroll_to(i32 y){
    i32 maxs = dl.height - view_h; if(maxs < 0) maxs = 0;
    i32 ny = y < 0 ? 0 : y > maxs ? maxs : y;
    if(ny != scroll_y){ scroll_y = ny; page_dirty = 1; }
}
Node* vela_hit(i32 x, i32 y){ return doc ? layout_hit(&dl, x, y, scroll_y) : NULL; }

void vela_navigate(const char* url, const char* method, const char* body, int blen, const char* ctype){
    pend.on = 1;
    snprintf(pend.url, sizeof pend.url, "%s", url);
    snprintf(pend.method, sizeof pend.method, "%s", method ? method : "GET");
    free(pend.body); pend.body = NULL; pend.blen = 0;
    if(body){ pend.body = (char*)malloc((size_t)blen + 1); memcpy(pend.body, body, (size_t)blen); pend.body[blen] = 0; pend.blen = blen; }
    snprintf(pend.ctype, sizeof pend.ctype, "%s", ctype ? ctype : "");
}

/* --------------------------------------------------------------- layout */
static void scan_images(void){
    for(Node* n = doc->root; n; n = dom_next_in_tree(n, doc->root)){
        if(n->type != N_ELEM || n->img || !n->cs) continue;
        if(dom_is(n, "svg")){
            /* SVG inline: serializa e rasteriza (cor atual para currentColor) */
            Buf b = {0}; svg_serialize(n, &b);
            char key[64]; snprintf(key, sizeof key, "svg:%p:%d:%06x", (void*)doc, n->id, n->cs->color & 0xFFFFFF);
            if(b.n) n->img = image_svg_inline(key, b.p, b.n, n->cs->color);
            buf_free(&b);
            continue;
        }
        int is_img = dom_is(n, "img") || (dom_is(n, "input") && dom_attr(n, "type") && str_ieq(dom_attr(n, "type"), "image"));
        if(!is_img) continue;
        const char* src = dom_attr(n, "src");
        if(!src || !*src || str_istarts(src, "data:")) src = dom_attr(n, "data-src");
        if(!src || !*src){
            /* srcset: primeira candidata */
            const char* ss = dom_attr(n, "srcset");
            if(!ss) ss = dom_attr(n, "data-srcset");
            static char first[URLMAX];
            if(ss){ int k = 0; while(*ss == ' ') ss++; while(ss[k] && ss[k] != ' ' && ss[k] != ',' && k < URLMAX - 1){ first[k] = ss[k]; k++; } first[k] = 0; src = first; }
        }
        if(!src || !*src) continue;
        char u[URLMAX];
        if(url_resolve(doc->url, src, u, sizeof u)) n->img = image_get(u);
    }
}

/* o estilo so e refeito quando algo que o afeta mudou: DOM, folhas, foco,
 * tamanho da janela. Imagens e fontes que chegam so refazem o layout. */
static Doc* styled_doc = NULL;
static i32 styled_w = -1, styled_h = -1;
static void relayout(void){
    if(!doc) return;
    arena_reset(&frame_arena);
    view_w = app.w - SCROLLBAR; view_h = app.h - TOP;
    u64 t0 = sys_time_ms();
    int sheets_changed = sync_styles();
    u64 t1 = sys_time_ms();
    if(sheets_changed || doc->dirty || styled_doc != doc || styled_w != view_w || styled_h != view_h){
        for(int i = 0; i < doc->nnodes; i++) doc->byid[i]->cs = NULL;
        css_style_doc(doc, view_w, view_h);
        styled_doc = doc; styled_w = view_w; styled_h = view_h;
    }
    u64 t2 = sys_time_ms();
    scan_images();
    layout_doc(doc, &dl, view_w, view_h);
    u64 t3 = sys_time_ms();
    if(t3 - t0 > 50) printf("[vela] relayout: folhas %u ms, estilo %u ms, layout %u ms (%d nodos, %d itens)\n",
        (unsigned)(t1 - t0), (unsigned)(t2 - t1), (unsigned)(t3 - t2), doc->nnodes, dl.n);
    doc->dirty = 0;
    need_layout = 0;
    last_layout = t3;
    layout_cost = t3 - t0;
    page_dirty = 1;
    vela_scroll_to(scroll_y);
}

/* --------------------------------------------------------------- pagina */
static int media_ok(Node* n){
    const char* m = dom_attr(n, "media");
    return !m || css_media_ok(m, (int)strlen(m));
}
/* 1 = classico, 2 = modulo, 0 = nao e JS (JSON, templates...) */
static int js_kind(Node* s){
    const char* t = dom_attr(s, "type");
    if(!t || !*t) return 1;
    if(str_ieq(t, "module")) return 2;
    return str_ieq(t, "text/javascript") || str_ieq(t, "application/javascript") || str_ieq(t, "application/x-javascript")
        || str_ieq(t, "text/ecmascript") || str_ieq(t, "application/ecmascript") || str_ieq(t, "text/jscript");
}

/* ---- folhas de estilo: cada <link> e pedido a thread de rede e analisado
 * num worker assim que chega; aqui so se juntam as prontas, na ordem do
 * documento. `page_gen` muda quando uma pagina nova assume: entregas
 * atrasadas da pagina anterior sao descartadas. */
static u32 page_gen = 1;
typedef struct { char url[URLMAX]; Sheet* sheet; Arena* arena; int state; } SheetEntry;   /* 0 chegando, 1 ok, 2 falhou */
static SheetEntry sheet_cache[64];
static int nsheet_cache = 0, css_pending = 0;
typedef struct { Req rq; int idx; u32 gen; Arena* arena; Sheet* sheet; } CssReq;

static void css_run(Task* t){                      /* worker */
    CssReq* q = (CssReq*)t;
    Resp* r = &q->rq.f.r;
    if(r->status == 200 && r->len < 4 * 1024 * 1024){
        q->arena = (Arena*)calloc(1, sizeof(Arena));
        q->sheet = css_parse_sheet(q->arena, r->body, r->len, 1);
        css_sheet_set_base(q->sheet, q->arena, r->url);
    }
    resp_free(r);
}
static void css_done(Task* t){                     /* principal */
    CssReq* q = (CssReq*)t;
    if(q->gen == page_gen && q->idx < nsheet_cache){
        SheetEntry* e = &sheet_cache[q->idx];
        e->sheet = q->sheet; e->arena = q->arena; e->state = q->sheet ? 1 : 2;
        css_pending--;
        if(doc) doc->dirty = 1;
    } else if(q->arena){ arena_reset(q->arena); free(q->arena); }
    req_free(&q->rq);
}

static void sheets_reset(void){
    for(int i = 0; i < nsheet_cache; i++) if(sheet_cache[i].arena){ arena_reset(sheet_cache[i].arena); free(sheet_cache[i].arena); }
    nsheet_cache = 0; css_pending = 0;
}

static int link_sheet_url(Node* x, char* u){
    if(!dom_is(x, "link")) return 0;
    const char* rel = dom_attr(x, "rel"); const char* href = dom_attr(x, "href");
    if(!rel || !href || !str_icontains(rel, "stylesheet") || str_icontains(rel, "alternate") || !media_ok(x) || dom_attr(x, "disabled")) return 0;
    return url_resolve(doc->url, href, u, URLMAX);
}

/* pede as folhas novas (sem esperar) */
static void request_styles(void){
    for(Node* x = doc->root; x; x = dom_next_in_tree(x, doc->root)){
        if(x->type != N_ELEM || (x->flags & NF_SHEET_TRIED) || !dom_is(x, "link")) continue;
        char u[URLMAX];
        if(!link_sheet_url(x, u)) continue;
        x->flags |= NF_SHEET_TRIED;
        int known = 0;
        for(int i = 0; i < nsheet_cache; i++) if(!strcmp(sheet_cache[i].url, u)) known = 1;
        if(known || nsheet_cache >= MAX_SHEETS) continue;
        SheetEntry* e = &sheet_cache[nsheet_cache];
        memset(e, 0, sizeof *e);
        snprintf(e->url, URLMAX, "%s", u);
        CssReq* q = (CssReq*)calloc(1, sizeof *q);
        Req* r = req_new(u, 0); q->rq = *r; free(r); q->rq.f.url = q->rq.url;
        q->rq.task.run = css_run; q->rq.task.done = css_done;
        q->idx = nsheet_cache; q->gen = page_gen;
        nsheet_cache++; css_pending++;
        net_submit(&q->rq);
    }
}

static int sync_styles(void){
    request_styles();
    Sheet* list[256]; int n = 0;
    for(Node* x = doc->root; x && n < 256; x = dom_next_in_tree(x, doc->root)){
        if(x->type != N_ELEM) continue;
        if(dom_is(x, "style")){
            if(!media_ok(x)) continue;
            Buf b = {0}; dom_text_content(x, &b);
            u32 h = hash_str(b.p ? b.p : "", b.n) | 1;
            if(!x->sheet || x->sheet_hash != h){ x->sheet = css_parse_sheet(&doc->arena, b.p ? b.p : "", b.n, 1); x->sheet_hash = h; css_sheet_set_base(x->sheet, &doc->arena, doc->url); }
            buf_free(&b);
            list[n++] = x->sheet;
        } else if(dom_is(x, "link")){
            char u[URLMAX];
            if(!link_sheet_url(x, u)) continue;
            Sheet* sh = NULL;
            for(int i = 0; i < nsheet_cache; i++) if(sheet_cache[i].state == 1 && !strcmp(sheet_cache[i].url, u)){ sh = sheet_cache[i].sheet; break; }
            int dup = 0;
            for(int i = 0; i < n; i++) if(list[i] == sh) dup = 1;
            if(sh && !dup) list[n++] = sh;
        }
    }
    return css_set_sheets(doc, list, n);
}

/* ---- scripts: todos pedidos de uma vez quando o HTML chega; executados na
 * ordem do documento, um por volta do laco, conforme ficam prontos */
typedef struct { Req rq; int ready, orphan; } ScriptReq;
static struct { int active, loaded, count; Node* cur; ScriptReq* pre[MAX_SCRIPTS]; int npre; } sc;

static void script_done(Task* t){
    ScriptReq* q = (ScriptReq*)t;
    if(q->orphan) req_free(&q->rq); else q->ready = 1;
}
static void scripts_free(void){
    for(int i = 0; i < sc.npre; i++){
        if(sc.pre[i]->ready) req_free(&sc.pre[i]->rq);
        else sc.pre[i]->orphan = 1;                 /* ainda em voo: libera na entrega */
    }
    memset(&sc, 0, sizeof sc);
}

static int script_url(Node* s, char* u){
    const char* src = dom_attr(s, "src");
    const char* t = dom_attr(s, "type");
    if(!src || !*src || !js_kind(s) || (t && str_ieq(t, "importmap"))) return 0;
    if(js_kind(s) == 1 && dom_attr(s, "nomodule")) return 0;
    return url_resolve(doc->url, src, u, URLMAX);
}

static ScriptReq* script_request(const char* u){
    for(int i = 0; i < sc.npre; i++) if(!strcmp(sc.pre[i]->rq.url, u)) return sc.pre[i];
    if(sc.npre >= MAX_SCRIPTS) return NULL;
    ScriptReq* q = (ScriptReq*)calloc(1, sizeof *q);
    Req* r = req_new(u, 0); q->rq = *r; free(r); q->rq.f.url = q->rq.url;
    q->rq.task.done = script_done;
    sc.pre[sc.npre++] = q;
    net_submit(&q->rq);
    return q;
}

static void scripts_prefetch(void){
    for(Node* x = doc->root; x; x = dom_next_in_tree(x, doc->root)){
        if(!dom_is(x, "script") || (x->flags & NF_SCRIPT_DONE)) continue;
        char u[URLMAX];
        if(script_url(x, u)) script_request(u);
    }
}

/* 0 = o script ainda esta chegando (tente depois) */
static int run_one_script(Node* s, int idx){
    const char* t = dom_attr(s, "type");
    if(t && str_ieq(t, "importmap")){
        s->flags |= NF_SCRIPT_DONE;
        Buf b = {0}; dom_text_content(s, &b);
        if(b.n) js_set_importmap(buf_cstr(&b));
        buf_free(&b);
        return 1;
    }
    int kind = js_kind(s);
    if(!kind || (kind == 1 && dom_attr(s, "nomodule"))){ s->flags |= NF_SCRIPT_DONE; return 1; }
    const char* src = dom_attr(s, "src");
    if(src && *src){
        char u[URLMAX];
        if(!script_url(s, u)){ s->flags |= NF_SCRIPT_DONE; return 1; }
        ScriptReq* q = script_request(u);
        if(q && !q->ready) return 0;
        s->flags |= NF_SCRIPT_DONE;
        Resp* r = q ? &q->rq.f.r : NULL;
        if(q && q->rq.f.ok && r->status == 200){
            u64 t0 = sys_time_ms();
            if(kind == 2) js_run_module(r->body, r->len, r->url, s);
            else js_run_script(r->body, r->len, u, s);
            printf("[vela] script %s: %u ms\n", u, (unsigned)(sys_time_ms() - t0));
            js_fire(s, "load");
        } else js_fire(s, "error");
        return 1;
    }
    s->flags |= NF_SCRIPT_DONE;
    Buf b = {0}; dom_text_content(s, &b);
    if(b.n){
        if(kind == 2){ char name[URLMAX + 16]; snprintf(name, sizeof name, "%s#modulo-%d", vela_url(), idx); js_run_module(b.p, b.n, name, s); }
        else js_run_script(b.p, b.n, "inline", s);
    }
    buf_free(&b);
    return 1;
}

static void meta_refresh(void){
    for(Node* n = doc->root; n && !pend.on; n = dom_next_in_tree(n, doc->root)){
        if(!dom_is(n, "meta")) continue;
        const char* he = dom_attr(n, "http-equiv"); const char* c = dom_attr(n, "content");
        if(!he || !c || !str_ieq(he, "refresh") || atoi(c) > 5) continue;
        const char* u = str_icontains(c, "url=");
        if(u){ u += 4; while(*u == '\'' || *u == '"' || *u == ' ') u++; char t[URLMAX]; snprintf(t, sizeof t, "%s", u);
            int l = (int)strlen(t); while(l && (t[l - 1] == '\'' || t[l - 1] == '"')) t[--l] = 0;
            char out[URLMAX]; if(url_resolve(doc->url, t, out, sizeof out)) vela_navigate(out, NULL, NULL, 0, NULL); }
    }
}

static Node* queued_script = NULL;                 /* inserido pelo JS, esperando chegar */

/* executa o proximo script pronto; 0 quando a fase de scripts acabou */
static int script_step(void){
    if(!sc.active || !doc || pend.on) return 0;
    while(sc.cur && !(dom_is(sc.cur, "script") && !(sc.cur->flags & NF_SCRIPT_DONE))) sc.cur = dom_next_in_tree(sc.cur, doc->root);
    if(sc.cur && sc.count < MAX_SCRIPTS){
        Node* n = sc.cur;
        if(!run_one_script(n, sc.count + 1)) return 1;     /* ainda chegando */
        sc.count++;
        int ok = 0; for(Node* a = n; a; a = a->parent) if(a == doc->root){ ok = 1; break; }
        sc.cur = ok ? dom_next_in_tree(n, doc->root) : doc->root;    /* removido: recomeca */
        return 1;
    }
    if(!queued_script) queued_script = js_next_script();          /* inseridos pelo JS */
    if(queued_script){
        if(run_one_script(queued_script, sc.count + 1)){ queued_script = NULL; sc.count++; }
        return 1;
    }
    if(!sc.loaded){
        sc.loaded = 1;
        js_fire_loaded();
        vela_title_changed();
        meta_refresh();
        return 1;
    }
    sc.active = 0;
    return 0;
}

/* scripts inseridos depois da carga (timers, cliques) */
static void run_queued_scripts(void){
    if(sc.active) return;
    if(!queued_script) queued_script = js_next_script();
    if(queued_script && run_one_script(queued_script, ++sc.count)) queued_script = NULL;
}

/* pede todas as <img> ja no HTML (as imagens sao decodificadas por workers) */
static void request_images(void){
    for(Node* n = doc->root; n; n = dom_next_in_tree(n, doc->root)){
        if(!dom_is(n, "img") || n->img) continue;
        const char* src = dom_attr(n, "src");
        if(!src || !*src || str_istarts(src, "data:")) src = dom_attr(n, "data-src");
        if(!src || !*src) continue;
        char u[URLMAX];
        if(url_resolve(doc->url, src, u, sizeof u)) n->img = image_get(u);
    }
}

static void error_page(const char* url, const char* msg){
    Buf b = {0};
    buf_puts(&b, "<html><head><title>Erro</title></head><body style=\"font-family:sans-serif;margin:0;background:#f6f6f8\">"
                 "<div style=\"max-width:640px;margin:60px auto;padding:28px 32px;background:white;border:1px solid #e2e2e7;border-radius:12px\">"
                 "<h1 style=\"font-size:26px;color:#d93025;margin:0 0 12px\">Não foi possível abrir a página</h1><p style=\"color:#333;font-size:16px\">");
    for(const char* p = msg; *p; p++){ if(*p == '<') buf_puts(&b, "&lt;"); else if(*p == '&') buf_puts(&b, "&amp;"); else buf_putc(&b, *p); }
    buf_puts(&b, "</p><p style=\"color:#888;font-size:13px;word-break:break-all\">");
    for(const char* p = url; *p; p++){ if(*p == '<') buf_puts(&b, "&lt;"); else if(*p == '&') buf_puts(&b, "&amp;"); else buf_putc(&b, *p); }
    buf_puts(&b, "</p></div></body></html>");
    doc = html_parse(b.p, b.n, url);
    buf_free(&b);
}

static void scroll_to_fragment(const char* frag){
    if(!doc || !frag || !*frag) return;
    Node* t = dom_by_id(doc->root, frag);
    if(!t){
        for(Node* n = doc->root; n; n = dom_next_in_tree(n, doc->root))
            if(dom_is(n, "a") && dom_attr(n, "name") && !strcmp(dom_attr(n, "name"), frag)){ t = n; break; }
    }
    if(t && t->cs) vela_scroll_to(t->by);
}

static int charset_latin1(const char* ctype, const char* body, int len){
    const char* cs = str_icontains(ctype, "charset=");
    char buf[2048]; int n = len < 2047 ? len : 2047;
    if(!cs){ memcpy(buf, body, (size_t)n); buf[n] = 0; cs = str_icontains(buf, "charset="); if(cs) cs += 8; } else cs += 8;
    if(!cs) return 0;
    while(*cs == '"' || *cs == '\'' || *cs == ' ') cs++;
    return str_istarts(cs, "iso-8859") || str_istarts(cs, "windows-125") || str_istarts(cs, "latin1") || str_istarts(cs, "us-ascii");
}

static const char* START_PAGE = "about:vela";

/* ---- navegacao assincrona: o HTML e pedido a thread de rede e a pagina
 * atual continua na tela (e respondendo) ate ele chegar */
typedef struct { Req rq; u32 nav; int push; char* body; } PageReq;
static u32 nav_seq = 0;                 /* a navegacao mais recente */
static PageReq* page_req = NULL;        /* HTML a caminho */
static int waiting_css = 0;             /* pagina nova esperando as folhas para pintar */
static u64 css_wait_since = 0, nav_t0 = 0;

static void page_teardown(void){
    scripts_free();
    queued_script = NULL;
    js_stop();
    if(doc){ doc_free(doc); doc = NULL; }
    styled_doc = NULL;                    /* (um Doc novo pode ter o mesmo endereco) */
    css_forget_doc();
    sheets_reset();
    image_reset();
    arena_reset(&frame_arena);
    dl.n = 0; dl.height = 0;
    scroll_y = 0;
    page_dirty = 1;
    js_status[0] = 0;
    waiting_css = 0;
    page_gen++;
}

/* primeira pintura: folhas prontas (ou demorando demais) */
static void first_paint(void){
    waiting_css = 0;
    relayout();
    const char* h = strchr(shown, '#');
    scroll_to_fragment(h ? h + 1 : NULL);
    printf("[vela] primeira pintura de %s em %u ms\n", shown, (unsigned)(sys_time_ms() - nav_t0));
    if(js_enabled()){ sc.active = 1; sc.cur = doc->root; }
}

static void html_done(Task* t){
    PageReq* q = (PageReq*)t;
    if(q != page_req || q->nav != nav_seq){ free(q->body); req_free(&q->rq); return; }  /* superada */
    page_req = NULL;
    Resp* r = &q->rq.f.r;
    page_teardown();
    if(!q->rq.f.ok){
        vela_set_url(q->rq.url);
        error_page(q->rq.url, r->err);
        relayout(); vela_title_changed();
        free(q->body); req_free(&q->rq);
        return;
    }
    const char* hash = strchr(q->rq.url, '#');
    char final[URLMAX];
    snprintf(final, sizeof final, "%s%s", r->url, hash ? hash : "");
    vela_set_url(final);
    int is_html = str_icontains(r->ctype, "html") || (!r->ctype[0] && r->len && memchr(r->body, '<', (size_t)(r->len < 512 ? r->len : 512)));
    if(!is_html){
        Buf b = {0};
        if(str_istarts(r->ctype, "image/")){
            buf_puts(&b, "<html><body style=\"margin:0;background:#202124;text-align:center\"><img style=\"max-width:100%\" src=\"");
            buf_puts(&b, r->url); buf_puts(&b, "\"></body></html>");
        } else if(str_istarts(r->ctype, "text/") || str_icontains(r->ctype, "json") || str_icontains(r->ctype, "javascript") || str_icontains(r->ctype, "xml")){
            buf_puts(&b, "<html><body><pre style=\"white-space:pre-wrap;font-size:14px\">");
            for(int i = 0; i < r->len; i++){ char c = r->body[i]; if(c == '<') buf_puts(&b, "&lt;"); else if(c == '&') buf_puts(&b, "&amp;"); else buf_putc(&b, c); }
            buf_puts(&b, "</pre></body></html>");
        } else {
            char m[300]; snprintf(m, sizeof m, "O Vela não sabe exibir este tipo de arquivo (%s).", r->ctype[0] ? r->ctype : "desconhecido");
            error_page(r->url, m);
            relayout(); vela_title_changed();
            free(q->body); req_free(&q->rq); buf_free(&b);
            return;
        }
        doc = html_parse(b.p, b.n, r->url);
        buf_free(&b);
    } else if(charset_latin1(r->ctype, r->body, r->len)){
        int n; char* u = utf8_from_latin1(r->body, r->len, &n);
        doc = html_parse(u, n, r->url);
        free(u);
    } else doc = html_parse(r->body, r->len, r->url);
    free(q->body); req_free(&q->rq);
    vela_title_changed();
    /* tudo de uma vez: folhas, scripts e imagens comecam a chegar em paralelo */
    request_styles();
    if(is_html){ js_start(doc); scripts_prefetch(); }
    request_images();
    if(css_pending){ waiting_css = 1; css_wait_since = sys_time_ms(); }
    else first_paint();
}

static void load(const char* target, const char* method, const char* body, int blen, const char* ctype, int push){
    editing = 0;
    char url[URLMAX]; snprintf(url, sizeof url, "%s", target);
    /* mesmo documento, so muda o fragmento */
    const char* hash = strchr(url, '#');
    if(doc && hash && !body && strncmp(url, shown, (size_t)(hash - url)) == 0 && (shown[hash - url] == 0 || shown[hash - url] == '#')){
        vela_set_url(url);
        scroll_to_fragment(hash + 1);
        return;
    }
    if(push && shown[0] && strcmp(shown, url) != 0){
        if(nhist == HIST){ memmove(hist, hist + 1, sizeof(hist[0]) * (HIST - 1)); nhist--; }
        snprintf(hist[nhist++], URLMAX, "%s", shown);
    }
    /* o que estava na fila para a pagina anterior nao precisa mais ser baixado */
    __atomic_add_fetch(&net_gen, 1, __ATOMIC_ACQ_REL);
    nav_seq++;
    nav_t0 = sys_time_ms();
    if(!strcmp(url, START_PAGE) || !strcmp(url, "about:blank")){
        page_req = NULL;
        page_teardown();
        shown[0] = 0; addr[0] = 0; addr_n = addr_cur = 0;
        app_title(&app, "Vela");
        return;
    }
    vela_set_url(url);
    PageReq* q = (PageReq*)calloc(1, sizeof *q);
    Req* r = req_new(url, 0); q->rq = *r; free(r); q->rq.f.url = q->rq.url;
    q->rq.gen = 0;                                  /* a pagina em si nao e cancelada pela fila */
    q->rq.task.done = html_done;
    q->nav = nav_seq; q->push = push;
    if(method){ q->rq.f.method = !strcmp(method, "POST") ? "POST" : "GET"; }
    if(body){
        q->body = (char*)malloc((size_t)blen + 1); memcpy(q->body, body, (size_t)blen);
        q->rq.f.body = q->body; q->rq.f.body_len = blen;
        q->rq.f.body_ctype = ctype && !strcmp(ctype, "multipart/form-data") ? ctype : "application/x-www-form-urlencoded";
    }
    page_req = q;
    net_submit(&q->rq);
}

static void go_back(void){
    if(!nhist) return;
    char u[URLMAX]; snprintf(u, sizeof u, "%s", hist[--nhist]);
    load(u, "GET", NULL, 0, NULL, 0);
}

static void do_pending(void){
    while(pend.on){
        pend.on = 0;
        char u[URLMAX]; snprintf(u, sizeof u, "%s", pend.url);
        char method[8]; snprintf(method, sizeof method, "%s", pend.method);
        char* body = pend.body; int blen = pend.blen; pend.body = NULL;
        char ctype[96]; snprintf(ctype, sizeof ctype, "%s", pend.ctype);
        if(!strcmp(u, "vela:back")) go_back();
        else if(str_istarts(u, "http://") || str_istarts(u, "https://") || str_istarts(u, "about:"))
            load(u, method, body, blen, ctype[0] ? ctype : NULL, 1);
        else { char m[300]; snprintf(m, sizeof m, "Endereço não suportado: %s", u); vela_status(m); }
        free(body);
    }
}

/* ---------------------------------------------------------- formularios */
void vela_submit(Node* form, Node* submitter){
    if(!doc || !form) return;
    const char* action = dom_attr(form, "action");
    if(submitter && dom_attr(submitter, "formaction")) action = dom_attr(submitter, "formaction");
    char target[URLMAX];
    if(!url_resolve(doc->url, action && *action ? action : vela_url(), target, sizeof target)) return;
    const char* m = dom_attr(form, "method");
    int post = m && str_ieq(m, "post");
    Buf q = {0};
    for(Node* n = form->first; n; n = dom_next_in_tree(n, form)){
        if(n->type != N_ELEM) continue;
        const char* name = dom_attr(n, "name");
        if(!name || !*name || dom_attr(n, "disabled")) continue;
        const char* val = NULL; Buf tmp = {0};
        if(dom_is(n, "input")){
            const char* t = dom_attr(n, "type"); if(!t) t = "text";
            if(str_ieq(t, "checkbox") || str_ieq(t, "radio")){
                int on = n->checked == 1 || (n->checked == 0 && dom_attr(n, "checked"));
                if(!on) continue;
                val = dom_attr(n, "value"); if(!val) val = "on";
            } else if(str_ieq(t, "submit") || str_ieq(t, "image") || str_ieq(t, "button")){
                if(n != submitter) continue;
                val = dom_attr(n, "value"); if(!val) val = "";
            } else if(str_ieq(t, "file") || str_ieq(t, "reset")) continue;
            else { val = n->value ? n->value : dom_attr(n, "value"); if(!val) val = ""; }
        } else if(dom_is(n, "button")){
            if(n != submitter) continue;
            val = dom_attr(n, "value"); if(!val) val = "";
        } else if(dom_is(n, "textarea")){
            if(n->value) val = n->value; else { dom_text_content(n, &tmp); val = tmp.p ? tmp.p : ""; }
        } else if(dom_is(n, "select")){
            Node* sel = NULL; Node* first = NULL;
            for(Node* o = n->first; o; o = dom_next_in_tree(o, n)){
                if(!dom_is(o, "option")) continue;
                if(!first) first = o;
                if(o->checked == 1 || (o->checked == 0 && dom_attr(o, "selected"))){ sel = o; break; }
            }
            if(!sel) sel = first;
            if(!sel) continue;
            val = dom_attr(sel, "value");
            if(!val){ dom_text_content(sel, &tmp); val = tmp.p ? tmp.p : ""; }
        } else continue;
        if(q.n) buf_putc(&q, '&');
        url_encode_component(&q, name, (int)strlen(name));
        buf_putc(&q, '=');
        url_encode_component(&q, val, (int)strlen(val));
        buf_free(&tmp);
    }
    if(post) vela_navigate(target, "POST", q.p ? q.p : "", q.n, "application/x-www-form-urlencoded");
    else {
        char* hsh = strchr(target, '#'); if(hsh) *hsh = 0;
        char* qm = strchr(target, '?'); if(qm) *qm = 0;
        char u[URLMAX]; snprintf(u, sizeof u, "%s?%s", target, q.p ? q.p : "");
        vela_navigate(u, NULL, NULL, 0, NULL);
    }
    buf_free(&q);
}

static Node* ancestor(Node* n, const char* tag){
    for(; n; n = n->parent) if(dom_is(n, tag)) return n;
    return NULL;
}
static void user_submit(Node* form, Node* submitter){
    if(js_enabled() && js_dispatch(form, "submit", 0, 0)) return;
    vela_submit(form, submitter);
}

static int is_text_input(Node* n){
    if(dom_is(n, "textarea")) return 1;
    if(!dom_is(n, "input")) return 0;
    const char* t = dom_attr(n, "type");
    if(!t) return 1;
    static const char* T[] = {"text", "search", "email", "url", "password", "tel", "number", "", NULL};
    for(int i = 0; T[i]; i++) if(str_ieq(t, T[i])) return 1;
    return 0;
}

/* clique na pagina: eventos do JS e depois a acao padrao */
static void page_click(i32 x, i32 y){
    Node* n = vela_hit(x, y);
    if(!n){ if(doc && doc->focus){ doc->focus = NULL; doc->dirty = 1; } return; }
    int prevented = 0;
    if(js_enabled()){
        js_dispatch(n, "mousedown", x, y);
        js_dispatch(n, "mouseup", x, y);
        prevented = js_dispatch(n, "click", x, y);
        if(pend.on) return;
    }
    Node* old_focus = doc->focus;
    doc->focus = is_text_input(n) ? n : NULL;
    if(old_focus != doc->focus) doc->dirty = 1;
    if(prevented) return;
    for(Node* a = n; a; a = a->parent){
        if(a->type != N_ELEM) continue;
        if(dom_is(a, "a") && dom_attr(a, "href")){
            const char* h = dom_attr(a, "href");
            if(str_istarts(h, "javascript:")){ js_eval_url(h + 11); return; }
            char u[URLMAX];
            if(url_resolve(doc->url, h, u, sizeof u)) vela_navigate(u, NULL, NULL, 0, NULL);
            return;
        }
        if(dom_is(a, "input")){
            const char* t = dom_attr(a, "type"); if(!t) t = "text";
            if(str_ieq(t, "submit") || str_ieq(t, "image")){ Node* f = ancestor(a, "form"); if(f) user_submit(f, a); return; }
            if(str_ieq(t, "checkbox")){ int on = a->checked == 1 || (a->checked == 0 && dom_attr(a, "checked")); a->checked = on ? -1 : 1; doc->dirty = 1; if(js_enabled()) js_dispatch(a, "change", 0, 0); return; }
            if(str_ieq(t, "radio")){
                const char* nm = dom_attr(a, "name");
                Node* scope = ancestor(a, "form"); if(!scope) scope = doc->root;
                for(Node* r = scope; r && nm; r = dom_next_in_tree(r, scope))
                    if(dom_is(r, "input") && r != a && dom_attr(r, "name") && !strcmp(dom_attr(r, "name"), nm)) r->checked = -1;
                a->checked = 1; doc->dirty = 1;
                if(js_enabled()) js_dispatch(a, "change", 0, 0);
                return;
            }
            return;
        }
        if(dom_is(a, "button")){
            const char* t = dom_attr(a, "type");
            if(!t || str_ieq(t, "submit")){ Node* f = ancestor(a, "form"); if(f) user_submit(f, a); }
            return;
        }
        if(dom_is(a, "select")){
            /* sem menu suspenso: cada clique passa para a proxima opcao */
            Node* opts[256]; int no = 0, cur = 0;
            for(Node* o = a->first; o && no < 256; o = dom_next_in_tree(o, a)) if(dom_is(o, "option")) opts[no++] = o;
            for(int i = 0; i < no; i++) if(opts[i]->checked == 1 || (opts[i]->checked == 0 && dom_attr(opts[i], "selected"))) cur = i;
            if(no){
                for(int i = 0; i < no; i++){ opts[i]->checked = -1; dom_del_attr(opts[i], "selected"); }
                opts[(cur + 1) % no]->checked = 1;
                dom_set_attr(doc, opts[(cur + 1) % no], "selected", "");
                doc->dirty = 1;
                if(js_enabled()) js_dispatch(a, "change", 0, 0);
            }
            return;
        }
        if(dom_is(a, "label")){
            const char* f = dom_attr(a, "for");
            Node* c = f ? dom_by_id(doc->root, f) : NULL;
            if(c && is_text_input(c)){ doc->focus = c; doc->dirty = 1; }
            return;
        }
        if(dom_is(a, "summary") && a->parent && dom_is(a->parent, "details")){
            if(dom_attr(a->parent, "open")) dom_del_attr(a->parent, "open"); else dom_set_attr(doc, a->parent, "open", "");
            doc->dirty = 1;
            return;
        }
    }
}

/* teclado num campo de texto da pagina */
static void field_key(WmEvent* e){
    Node* f = doc->focus;
    const char* v = f->value;
    Buf b = {0};
    if(!v){
        if(dom_is(f, "textarea")){ dom_text_content(f, &b); v = b.p ? b.p : ""; }
        else { v = dom_attr(f, "value"); if(!v) v = ""; }
    }
    char* s = (char*)malloc(strlen(v) + 8);
    strcpy(s, v);
    buf_free(&b);
    int changed = 0;
    char keyname[8] = "";
    if(e->ch >= 32){ int n = utf8_encode(e->ch, keyname); keyname[n] = 0; }
    if(js_enabled()){
        const char* kn = keyname[0] ? keyname : e->key == KEY_ENTER || e->key == KEY_KPENTER ? "Enter" : e->key == KEY_BACKSPACE ? "Backspace"
            : e->key == KEY_ESC ? "Escape" : e->key == KEY_TAB ? "Tab" : "Unidentified";
        int code = e->key == KEY_ENTER || e->key == KEY_KPENTER ? 13 : e->key == KEY_BACKSPACE ? 8 : e->key == KEY_ESC ? 27 : (e->ch < 128 ? (int)(e->ch >= 'a' && e->ch <= 'z' ? e->ch - 32 : e->ch) : 0);
        if(js_key(f, kn, code) || pend.on){ free(s); return; }
        if(!doc->focus){ free(s); return; }
    }
    if(e->key == KEY_BACKSPACE){
        int l = (int)strlen(s);
        while(l > 0 && ((u8)s[l - 1] & 0xC0) == 0x80) l--;
        if(l > 0){ s[l - 1] = 0; changed = 1; }
    } else if(e->key == KEY_ENTER || e->key == KEY_KPENTER){
        if(dom_is(f, "textarea")){ strcat(s, "\n"); changed = 1; }
        else { Node* form = ancestor(f, "form"); if(form){ f->value = arena_strdup(&doc->arena, s); free(s); user_submit(form, NULL); return; } }
    } else if(e->key == KEY_ESC){ doc->focus = NULL; doc->dirty = 1; }
    else if(e->ch >= 32 && strlen(s) < 2000){
        s = (char*)realloc(s, strlen(s) + 8);
        strcat(s, keyname); changed = 1;
    }
    if(changed){
        f->value = arena_strdup(&doc->arena, s);
        doc->dirty = 1;
        if(js_enabled()) js_dispatch(f, "input", 0, 0);
    }
    free(s);
}

/* ------------------------------------------------------------- desenho */
static Rect addr_rect(void){ return rect(96, 12, app.w - 200, 32); }
static Rect go_rect(void){ return rect(app.w - 92, 12, 76, 32); }
static Rect reload_rect(void){ return rect(54, 12, 32, 32); }
static Rect back_rect(void){ return rect(16, 12, 32, 32); }

static void draw_start(Fb* fb, const Theme* th){
    i32 top = TOP, cx = app.w / 2, cy = top + 120;
    fb_fill(fb, rect(0, top, app.w, app.h - top), th->win_bg);
    fb_disc(fb, cx, cy, 42, 0x1A84FF, 255);
    fb_disc(fb, cx, cy, 36, 0xFFFFFF, 255);
    static const i32 dx[12] = {0, 15, 26, 30, 26, 15, 0, -15, -26, -30, -26, -15};
    static const i32 dy[12] = {-30, -26, -15, 0, 15, 26, 30, 26, 15, 0, -15, -26};
    for(int k = 0; k < 12; k++) fb_line(fb, cx + dx[k] * 9 / 10, cy + dy[k] * 9 / 10, cx + dx[k], cy + dy[k], 2, 0xA0A0A8, 255);
    fb_line(fb, cx, cy, cx + 16, cy - 16, 7, 0xFF3B30, 255);
    fb_line(fb, cx, cy, cx - 16, cy + 16, 7, 0xC7C7CC, 255);
    fb_disc(fb, cx, cy, 4, 0x3A3A3C, 255);
    text_center(fb, &FONT_TITLE, rect(0, top + 166, app.w, 30), "Vela", th->text);
    text_center(fb, &FONT_UI, rect(0, top + 202, app.w, 20), "Digite um endereço ou uma pesquisa e pressione Enter.", th->text2);
    text_center(fb, &FONT_SMALL, rect(0, top + 232, app.w, 18), "Ex.: wikipedia.org,  news.ycombinator.com,  github.com", th->text3);
    text_center(fb, &FONT_SMALL, rect(0, top + 252, app.w, 18), "Servidor local: no host, rode  python3 -m http.server 8080  e abra 10.0.2.2:8080", th->text3);
}

/* balao no canto inferior esquerdo: link sob o mouse ou aviso */
static void bubble_text(i32 mx, i32 my, char* out){
    out[0] = 0;
    if(notice[0] && sys_time_ms() < notice_until){ snprintf(out, URLMAX, "%s", notice); return; }
    notice[0] = 0;
    if(!doc || my < TOP || mx >= view_w) return;
    Node* n = vela_hit(mx, my - TOP);
    for(Node* a = n; a; a = a->parent)
        if(dom_is(a, "a") && dom_attr(a, "href")){ url_resolve(doc->url, dom_attr(a, "href"), out, URLMAX); return; }
}

static void draw(i32 mx, i32 my){
    Fb* fb = &app.fb;
    const Theme* th = &app.th;
    char bt[URLMAX]; bubble_text(mx, my, bt);
    int bubble_changed = strcmp(bt, bubble) != 0;
    if(!doc) draw_start(fb, th);
    else if(page_dirty){
        paint(fb, &dl, rect(0, TOP, view_w, app.h - TOP), rect(0, TOP, view_w, app.h - TOP), scroll_y, dl.bg);
        page_dirty = 0;
    } else if(bubble_changed && bubble_r.w){
        /* so apaga o balao antigo */
        paint(fb, &dl, rect(0, TOP, view_w, app.h - TOP), bubble_r, scroll_y, dl.bg);
    }
    if(doc){
        /* barra de rolagem */
        fb_fill(fb, rect(view_w, TOP, SCROLLBAR, app.h - TOP), th->dark ? 0x2A2B30 : 0xF0F0F3);
        if(dl.height > view_h){
            i32 th_h = view_h * view_h / dl.height; if(th_h < 24) th_h = 24;
            i32 ty = TOP + (i32)((i64)(view_h - th_h) * scroll_y / (dl.height - view_h));
            fb_round_rect(fb, rect(view_w + 2, ty + 2, SCROLLBAR - 4, th_h - 4), 3, th->dark ? 0x5A5B62 : 0xC4C4CA);
        }
        /* balao */
        snprintf(bubble, sizeof bubble, "%s", bt);
        bubble_r = rect(0, 0, 0, 0);
        if(bt[0]){
            i32 w = text_width(&FONT_SMALL, bt) + 16;
            if(w > view_w - 8) w = view_w - 8;
            bubble_r = rect(4, app.h - 26, w, 22);
            fb_round_rect(fb, bubble_r, 6, th->dark ? 0x2A2B30 : 0xF6F6F8);
            fb_round_border(fb, bubble_r, 6, th->ctrl_border, 255);
            Fb bf = *fb; fb_set_clip(&bf, rect(bubble_r.x + 8, bubble_r.y, bubble_r.w - 16, bubble_r.h));
            text_draw(&bf, &FONT_SMALL, bubble_r.x + 8, bubble_r.y + 3, bt, th->text2);
        }
    }
    /* barra de ferramentas */
    u32 bar = th->dark ? 0x2A2B30 : 0xF2F2F6;
    fb_fill(fb, rect(0, 0, app.w, 55), bar);
    fb_hline(fb, 0, 55, app.w, th->sep, 255);
    Rect br = back_rect();
    if(nhist && rect_has(br, mx, my)) fb_round_rect(fb, br, 8, th->hover);
    u32 bc = nhist ? th->text2 : th->text3;
    fb_line(fb, br.x + 19, br.y + 9, br.x + 12, br.y + 16, 2, bc, 255);
    fb_line(fb, br.x + 12, br.y + 16, br.x + 19, br.y + 23, 2, bc, 255);
    Rect rr = reload_rect();
    if(rect_has(rr, mx, my)) fb_round_rect(fb, rr, 8, th->hover);
    fb_ring(fb, rr.x + 16, rr.y + 16, 8, 2, th->text2, 255);
    fb_fill(fb, rect(rr.x + 18, rr.y + 4, 7, 6), rect_has(rr, mx, my) ? th->hover : bar);
    fb_line(fb, rr.x + 20, rr.y + 6, rr.x + 24, rr.y + 9, 2, th->text2, 255);
    Rect ar = addr_rect();
    fb_round_rect(fb, ar, 9, th->field_bg);
    fb_round_border(fb, ar, 9, editing ? th->accent : th->ctrl_border, 255);
    int secure = !editing && str_istarts(shown, "https://");
    if(secure){
        /* cadeado */
        u32 lc = 0x1E8E3E;
        fb_ring(fb, ar.x + 16, ar.y + 13, 4, 2, lc, 255);
        fb_round_rect(fb, rect(ar.x + 10, ar.y + 14, 12, 9), 2, lc);
    } else fb_disc(fb, ar.x + 16, ar.y + 16, 5, th->text3, 255);
    Fb af = *fb; fb_set_clip(&af, rect(ar.x + 28, ar.y, ar.w - 36, ar.h));
    const char* shown_txt = addr;
    if(!editing && secure) shown_txt = addr + 8;                 /* sem "https://" */
    i32 tw = text_width(&FONT_UI, shown_txt);
    i32 off = 0;
    if(editing){ i32 cx = text_width_n(&FONT_UI, addr, addr_cur); if(cx > ar.w - 50) off = cx - (ar.w - 50); }
    else if(tw > ar.w - 40) off = 0;
    text_draw(&af, &FONT_UI, ar.x + 30 - off, ar.y + 7, shown_txt, th->text);
    if(editing && addr_all && addr_n){
        fb_fill_a(&af, rect(ar.x + 29 - off, ar.y + 6, text_width(&FONT_UI, addr) + 2, 21), th->accent, 70);
        text_draw(&af, &FONT_UI, ar.x + 30 - off, ar.y + 7, addr, th->text);
    } else if(editing){ i32 cx = ar.x + 30 - off + text_width_n(&FONT_UI, addr, addr_cur); fb_fill(&af, rect(cx, ar.y + 8, 2, 17), th->accent); }
    Rect gr = go_rect();
    ui_button(fb, gr, "Ir", th, 1, rect_has(gr, mx, my));
    /* carregando: linha fina de destaque sob a barra */
    if(loading) fb_fill(fb, rect(0, 55, app.w, 2), th->accent);
}

/* ---------------------------------------------------------------- laco */
static void vela_main(void){
    app_init(&app, 7, "Vela", 900, 620);
    css_init();
    work_init();
    view_w = app.w - SCROLLBAR; view_h = app.h - TOP;
    i32 lmx = -1, lmy = -1;
    i32 lw = app.w, lh = app.h;
    draw(-1, -1);
    WmEvent e = app_commit(&app);
    for(;;){
        if(e.close) sys_exit();
        int dirty = e.changed, go = 0, user = 0, urgent = 0;   /* urgent: clique/tecla (pode ter mudado o DOM) */
        if(e.changed) page_dirty = 1;
        if(app.w != lw || app.h != lh){ lw = app.w; lh = app.h; if(doc) need_layout = 1; dirty = 1; page_dirty = 1; }
        if(e.click){
            user = urgent = 1;
            if(rect_has(addr_rect(), e.mx, e.my)){
                if(!editing){ editing = 1; addr_all = 1; snprintf(addr, sizeof addr, "%s", shown); addr_n = addr_cur = (int)strlen(addr); }
                else addr_all = 0;
                dirty = 1;
            } else { if(editing){ editing = 0; snprintf(addr, sizeof addr, "%s", shown); addr_n = addr_cur = (int)strlen(addr); } dirty = 1; }
            if(rect_has(go_rect(), e.mx, e.my)) go = 1;
            else if(rect_has(reload_rect(), e.mx, e.my)){ if(shown[0]) vela_navigate(shown, NULL, NULL, 0, NULL); }
            else if(rect_has(back_rect(), e.mx, e.my)) vela_navigate("vela:back", NULL, NULL, 0, NULL);
            else if(e.my >= TOP && e.mx < view_w && doc){ page_click(e.mx, e.my - TOP); dirty = 1; }
        }
        if(e.menu == APP_MENU_RELOAD && shown[0]) vela_navigate(shown, NULL, NULL, 0, NULL);
        if(e.menu == APP_MENU_CLEAR){ vela_navigate(START_PAGE, NULL, NULL, 0, NULL); }
        if(e.key){
            user = urgent = 1;
            if(editing){
                if(addr_all && (e.key == KEY_BACKSPACE || e.key == KEY_DELETE || (e.ch >= 32 && e.ch < 127))){
                    addr_n = addr_cur = 0; addr[0] = 0;
                    if(e.key == KEY_BACKSPACE || e.key == KEY_DELETE){ addr_all = 0; e.key = 0; e.ch = 0; }
                }
                if(e.key != KEY_ENTER && e.key != KEY_KPENTER) addr_all = 0;
                if(e.key == KEY_ENTER || e.key == KEY_KPENTER) go = 1;
                else if(e.key == KEY_BACKSPACE && addr_cur > 0){ memmove(addr + addr_cur - 1, addr + addr_cur, (size_t)(addr_n - addr_cur + 1)); addr_n--; addr_cur--; }
                else if(e.key == KEY_DELETE && addr_cur < addr_n){ memmove(addr + addr_cur, addr + addr_cur + 1, (size_t)(addr_n - addr_cur)); addr_n--; }
                else if(e.key == KEY_LEFT && addr_cur > 0) addr_cur--;
                else if(e.key == KEY_RIGHT && addr_cur < addr_n) addr_cur++;
                else if(e.key == KEY_HOME) addr_cur = 0;
                else if(e.key == KEY_END) addr_cur = addr_n;
                else if(e.key == KEY_ESC){ editing = 0; snprintf(addr, sizeof addr, "%s", shown); addr_n = addr_cur = (int)strlen(addr); }
                else if(e.ch >= 32 && e.ch < 127 && addr_n < URLMAX - 2){
                    memmove(addr + addr_cur + 1, addr + addr_cur, (size_t)(addr_n - addr_cur + 1));
                    addr[addr_cur++] = (char)e.ch; addr_n++;
                }
            } else if(doc && doc->focus) field_key(&e);
            else {
                int prevented = 0;
                if(js_enabled() && doc){
                    char kn[8] = "";
                    if(e.ch >= 32){ int n = utf8_encode(e.ch, kn); kn[n] = 0; }
                    const char* name = kn[0] ? kn : e.key == KEY_DOWN ? "ArrowDown" : e.key == KEY_UP ? "ArrowUp" : e.key == KEY_LEFT ? "ArrowLeft"
                        : e.key == KEY_RIGHT ? "ArrowRight" : e.key == KEY_ESC ? "Escape" : e.key == KEY_ENTER ? "Enter" : e.key == KEY_BACKSPACE ? "Backspace"
                        : e.key == KEY_PGDN ? "PageDown" : e.key == KEY_PGUP ? "PageUp" : e.key == KEY_HOME ? "Home" : e.key == KEY_END ? "End" : "Unidentified";
                    int code = e.key == KEY_DOWN ? 40 : e.key == KEY_UP ? 38 : e.key == KEY_LEFT ? 37 : e.key == KEY_RIGHT ? 39 : e.key == KEY_ESC ? 27
                        : e.key == KEY_ENTER ? 13 : (e.ch >= 'a' && e.ch <= 'z') ? (int)e.ch - 32 : (int)(e.ch < 128 ? e.ch : 0);
                    prevented = js_key(NULL, name, code);
                }
                if(!prevented){
                    if(e.key == KEY_DOWN) vela_scroll_to(scroll_y + 40);
                    else if(e.key == KEY_UP) vela_scroll_to(scroll_y - 40);
                    else if(e.key == KEY_PGDN || e.key == 57) vela_scroll_to(scroll_y + view_h - 40);
                    else if(e.key == KEY_PGUP) vela_scroll_to(scroll_y - view_h + 40);
                    else if(e.key == KEY_HOME) vela_scroll_to(0);
                    else if(e.key == KEY_END) vela_scroll_to(dl.height);
                    else if(e.key == KEY_BACKSPACE) vela_navigate("vela:back", NULL, NULL, 0, NULL);
                    else if(e.key == KEY_ENTER || e.key == KEY_KPENTER) go = !doc;
                    else if(e.ch > 32 && e.ch < 127 && !(e.mods & MOD_CTRL)){
                        editing = 1; addr_n = 0; addr[addr_n++] = (char)e.ch; addr[addr_n] = 0; addr_cur = 1;
                    }
                }
            }
            dirty = 1;
        }
        if(e.wheel){ vela_scroll_to(scroll_y - e.wheel * 60); dirty = 1; user = 1; }
        if(e.mx != lmx || e.my != lmy){
            /* so redesenha se mudar o que esta sob o mouse (botoes, balao do link) */
            if(e.my < TOP || lmy < TOP) dirty = 1;
            else if(doc){ char bt[URLMAX]; bubble_text(e.mx, e.my, bt); if(strcmp(bt, bubble)) dirty = 1; }
            lmx = e.mx; lmy = e.my;
        }
        if(notice[0] && sys_time_ms() >= notice_until) dirty = 1;
        if(go){
            char u[URLMAX];
            addr[addr_n] = 0;
            if(addr_n){ url_from_input(addr, u, sizeof u); vela_navigate(u, NULL, NULL, 0, NULL); }
            editing = 0;
        }
        if(pend.on){ do_pending(); dirty = 1; }
        if(e.more){ e = app_peek(&app); continue; }      /* eventos primeiro */
        /* entregas das outras threads: HTML, folhas analisadas, imagens decodificadas */
        if(work_poll()) dirty = 1;
        if(image_pump()) need_layout = 1;
        if(font_pump()) need_layout = 1;
        if(waiting_css && doc && (css_pending == 0 || sys_time_ms() - css_wait_since > 5000)){ first_paint(); dirty = 1; }
        /* trabalho na thread principal: um script por volta */
        int busy = 0;
        if(doc && !user && !waiting_css && sc.active){ busy = 1; script_step(); if(pend.on){ do_pending(); dirty = 1; } }
        int next = -1;
        if(js_enabled() && doc && !sc.active && !waiting_css){
            u64 now = sys_time_ms();
            if(now - last_tick >= 15){
                last_tick = now;
                next = js_tick(now);
                run_queued_scripts();
                if(pend.on){ do_pending(); dirty = 1; }
            } else next = 15;
        }
        /* relayout: na hora se foi o usuario ou uma imagem; mudancas vindas do
         * JS esperam um intervalo de pelo menos 3x o custo do ultimo relayout
         * (min. 300 ms), para a pagina nao passar o tempo todo se refazendo */
        if(doc && !waiting_css && (doc->dirty || need_layout)){
            u64 gap = layout_cost * 3 > 300 ? layout_cost * 3 : 300;
            if(urgent || need_layout || sys_time_ms() - last_layout >= gap || (!busy && next < 0)){ relayout(); dirty = 1; }
            else busy = 1;
        }
        int was_loading = loading;
        loading = page_req || waiting_css || (doc && (sc.active || image_pending() || css_pending));
        if(page_req || work_busy() || css_pending) busy = 1;
        if(loading != was_loading) dirty = 1;
        if(page_dirty) dirty = 1;
        if(dirty){ draw(e.mx, e.my); e = app_commit(&app); }
        else if(busy){ sys_sleep(2); e = app_peek(&app); }
        else if(next >= 0 && next < 250){ sys_sleep((u64)(next < 30 ? next : 30)); e = app_peek(&app); }
        else e = app_poll(&app);
    }
}

/* troca para uma pilha propria de 1 MiB: o QuickJS e o layout recursivo
 * precisam de mais que os 256 KiB que o loader mapeia */
static u8 big_stack[1 << 20] __attribute__((aligned(16)));
__attribute__((naked, noreturn)) static void run_on_stack(void (*fn)(void), void* top){
#if defined(__riscv)
    __asm__ volatile("mv sp, a1\n jr a0");
#elif defined(__x86_64__)
    __asm__ volatile("mov %rsi, %rsp\n push $0\n jmp *%rdi");
#else
    __asm__ volatile("mov sp, x1\n br x0");
#endif
}

int main(void){
    run_on_stack(vela_main, big_stack + sizeof big_stack);
}
