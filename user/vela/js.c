/* js.c — JavaScript das paginas com o QuickJS.
 *
 * O DOM e exposto ao JS por um objeto __vela com funcoes nativas que falam
 * em ids de nodos (inteiros). Todo o resto da API do navegador (Node,
 * Element, document, window, eventos, timers, fetch, URL...) e escrito em
 * JS no prelude.js, por cima dessas funcoes. Cada pagina ganha um runtime
 * novo; ao navegar, o anterior e destruido inteiro. */
#include "vela.h"
#include "quickjs.h"

extern const unsigned char vela_prelude[];
extern const unsigned int vela_prelude_len;

char js_status[160];
static JSRuntime* rt = NULL;
static JSContext* ctx = NULL;
static u64 deadline = 0;
static Node* cur_script = NULL;
static Node* write_after = NULL;          /* document.write durante a carga */
static int loading = 0;

/* scripts inseridos pelo JS (fila) */
static int* script_q = NULL; static int nscript_q = 0, cscript_q = 0;

int js_enabled(void){ return ctx != NULL; }

static int interrupt(JSRuntime* r, void* o){
    (void)r; (void)o;
    return deadline && sys_time_ms() > deadline;
}

static void report(JSContext* c){
    JSValue ex = JS_GetException(c);
    const char* m = JS_ToCString(c, ex);
    JSValue stack = JS_IsObject(ex) ? JS_GetPropertyStr(c, ex, "stack") : JS_UNDEFINED;
    const char* st = JS_IsUndefined(stack) ? NULL : JS_ToCString(c, stack);
    printf("[vela/js] erro: %s\n%s", m ? m : "?", st ? st : "");
    snprintf(js_status, sizeof js_status, "Erro de script: %s", m ? m : "?");
    if(m) JS_FreeCString(c, m);
    if(st) JS_FreeCString(c, st);
    JS_FreeValue(c, stack);
    JS_FreeValue(c, ex);
}

/* promessa rejeitada sem .catch(): vai para o log como erro */
static void rejection(JSContext* c, JSValueConst promise, JSValueConst reason, JS_BOOL handled, void* o){
    (void)promise; (void)o;
    if(handled) return;
    const char* m = JS_ToCString(c, reason);
    JSValue st = JS_IsObject(reason) ? JS_GetPropertyStr(c, reason, "stack") : JS_UNDEFINED;
    const char* sm = JS_IsString(st) ? JS_ToCString(c, st) : NULL;
    printf("[vela/js] promessa rejeitada: %s\n%s", m ? m : "?", sm ? sm : "");
    if(m) JS_FreeCString(c, m);
    if(sm) JS_FreeCString(c, sm);
    JS_FreeValue(c, st);
}

static void run_jobs(void){
    JSContext* c2;
    for(int i = 0; i < 20000 && JS_IsJobPending(rt); i++){
        int r = JS_ExecutePendingJob(rt, &c2);
        if(r < 0){ report(c2); }
        if(r == 0) break;
    }
}

/* ---------------------------------------------------------- utilidades */
static Node* N(JSContext* c, JSValueConst v){
    int32_t id = 0;
    if(JS_ToInt32(c, &id, v)) return NULL;
    return dom_get(doc, id);
}
static JSValue ID(Node* n){ return JS_NewInt32(ctx, n ? n->id : 0); }
static JSValue STR(JSContext* c, const char* s, int n){ return JS_NewStringLen(c, s ? s : "", (size_t)(s ? n : 0)); }
#define ARGS(nm) static JSValue nm(JSContext* c, JSValueConst this_val, int argc, JSValueConst* argv)
#define CSTR(var, i) size_t var##_len = 0; const char* var = argc > (i) ? JS_ToCStringLen(c, &var##_len, argv[i]) : NULL; \
    if(argc > (i) && !var) return JS_EXCEPTION
#define FREE(var) do{ if(var) JS_FreeCString(c, var); }while(0)
#define UNUSED (void)this_val; (void)argc

static void dirty(void){ if(doc) doc->dirty = 1; }

static int connected(Node* n){
    for(; n; n = n->parent) if(n->type == N_DOC) return 1;
    return 0;
}
/* um <script> entrou no documento pelo JS: entra na fila para executar */
static void check_scripts(Node* root){
    for(Node* n = root; n; n = dom_next_in_tree(n, root)){
        if(dom_is(n, "script") && !(n->flags & NF_SCRIPT_DONE)){
            n->flags |= NF_SCRIPT_DONE;
            if(nscript_q == cscript_q){ cscript_q = cscript_q ? cscript_q * 2 : 16; script_q = (int*)realloc(script_q, sizeof(int) * (size_t)cscript_q); }
            script_q[nscript_q++] = n->id;
        }
    }
}
Node* js_next_script(void){
    while(nscript_q){
        int id = script_q[0];
        memmove(script_q, script_q + 1, sizeof(int) * (size_t)--nscript_q);
        Node* n = dom_get(doc, id);
        if(n && connected(n)) return n;
    }
    return NULL;
}

/* ------------------------------------------------------------- nativas */
ARGS(n_node_type){ UNUSED; Node* n = N(c, argv[0]); return JS_NewInt32(c, n ? n->type : 0); }
ARGS(n_tag){
    UNUSED; Node* n = N(c, argv[0]);
    if(!n || n->type != N_ELEM) return JS_NewString(c, "");
    char t[64]; int k = 0;
    for(; n->tag[k] && k < 63; k++) t[k] = (char)(n->tag[k] >= 'a' && n->tag[k] <= 'z' ? n->tag[k] - 32 : n->tag[k]);
    return STR(c, t, k);
}
ARGS(n_parent){ UNUSED; Node* n = N(c, argv[0]); return ID(n ? n->parent : NULL); }
ARGS(n_first){ UNUSED; Node* n = N(c, argv[0]); return ID(n ? n->first : NULL); }
ARGS(n_last){ UNUSED; Node* n = N(c, argv[0]); return ID(n ? n->last : NULL); }
ARGS(n_next){ UNUSED; Node* n = N(c, argv[0]); return ID(n ? n->next : NULL); }
ARGS(n_prev){ UNUSED; Node* n = N(c, argv[0]); return ID(n ? n->prev : NULL); }
ARGS(n_attr){
    UNUSED; Node* n = N(c, argv[0]);
    CSTR(name, 1);
    char ln[64]; int k = 0; for(; name && name[k] && k < 63; k++) ln[k] = (char)lower((u8)name[k]); ln[k] = 0;
    FREE(name);
    const char* v = dom_attr(n, ln);
    return v ? JS_NewString(c, v) : JS_NULL;
}
ARGS(n_set_attr){
    UNUSED; Node* n = N(c, argv[0]);
    CSTR(name, 1); CSTR(val, 2);
    if(n && n->type == N_ELEM && name && val){
        dom_set_attr(doc, n, name, val);
        if(str_ieq(name, "value") && dom_is(n, "input")) n->value = NULL;
        if(dom_is(n, "img") && str_ieq(name, "src")) n->img = NULL;
        dirty();
    }
    FREE(name); FREE(val);
    return JS_UNDEFINED;
}
ARGS(n_del_attr){
    UNUSED; Node* n = N(c, argv[0]);
    CSTR(name, 1);
    if(n && name){ dom_del_attr(n, name); dirty(); }
    FREE(name);
    return JS_UNDEFINED;
}
ARGS(n_attrs){
    UNUSED; Node* n = N(c, argv[0]);
    JSValue a = JS_NewArray(c);
    if(n) for(int i = 0; i < n->nattr; i++){
        JS_SetPropertyUint32(c, a, (u32)(2 * i), JS_NewString(c, n->attrs[i].name));
        JS_SetPropertyUint32(c, a, (u32)(2 * i + 1), JS_NewString(c, n->attrs[i].value));
    }
    return a;
}
ARGS(n_text){
    UNUSED; Node* n = N(c, argv[0]);
    if(!n) return JS_NewString(c, "");
    Buf b = {0}; dom_text_content(n, &b);
    JSValue r = STR(c, b.p, b.n);
    buf_free(&b);
    return r;
}
ARGS(n_set_text){
    UNUSED; Node* n = N(c, argv[0]);
    CSTR(s, 1);
    if(n && s){ dom_set_text(doc, n, s, (int)s_len); dirty(); }
    FREE(s);
    return JS_UNDEFINED;
}
ARGS(n_create){
    UNUSED;
    CSTR(tag, 0);
    Node* n = tag ? dom_new(doc, N_ELEM, tag) : NULL;
    FREE(tag);
    if(n && dom_is(n, "script")) n->flags |= NF_JS_INSERTED;
    return ID(n);
}
ARGS(n_create_text){ UNUSED; CSTR(s, 0); Node* n = dom_new_text(doc, s ? s : "", s ? (int)s_len : 0); FREE(s); return ID(n); }
ARGS(n_create_comment){
    UNUSED; CSTR(s, 0);
    Node* n = dom_new(doc, N_COMMENT, NULL);
    n->text = arena_strndup(&doc->arena, s ? s : "", s ? s_len : 0); n->tlen = s ? (int)s_len : 0;
    FREE(s); return ID(n);
}
ARGS(n_create_frag){ UNUSED; (void)argv; return ID(dom_new(doc, N_FRAG, "#fragment")); }
ARGS(n_insert){
    UNUSED;
    Node* p = N(c, argv[0]); Node* k = N(c, argv[1]); Node* before = argc > 2 ? N(c, argv[2]) : NULL;
    if(!p || !k) return JS_ThrowTypeError(c, "insert: nodo invalido");
    for(Node* a = p; a; a = a->parent) if(a == k) return JS_ThrowTypeError(c, "HierarchyRequestError");
    Node* first = k->type == N_FRAG ? k->first : k;
    Node* last = k->type == N_FRAG ? k->last : k;
    dom_insert(p, k, before);
    if(connected(p)) for(Node* x = first; x; x = x->next){ check_scripts(x); if(x == last) break; }
    dirty();
    return JS_UNDEFINED;
}
ARGS(n_remove){ UNUSED; Node* n = N(c, argv[0]); if(n){ if(doc->focus && (doc->focus == n)) doc->focus = NULL; dom_remove(n); dirty(); } return JS_UNDEFINED; }
ARGS(n_html){
    UNUSED; Node* n = N(c, argv[0]);
    int outer = argc > 1 && JS_ToBool(c, argv[1]);
    if(!n) return JS_NewString(c, "");
    Buf b = {0}; dom_serialize(n, &b, outer);
    JSValue r = STR(c, b.p, b.n); buf_free(&b);
    return r;
}
ARGS(n_set_html){
    UNUSED; Node* n = N(c, argv[0]);
    CSTR(s, 1);
    if(n && s){
        while(n->first) dom_remove(n->first);
        html_parse_into(doc, n, NULL, s, (int)s_len);
        /* scripts via innerHTML nao executam (como nos navegadores) */
        for(Node* x = n->first; x; x = dom_next_in_tree(x, n)) if(dom_is(x, "script")) x->flags |= NF_SCRIPT_DONE;
        dirty();
    }
    FREE(s);
    return JS_UNDEFINED;
}
ARGS(n_insert_html){
    UNUSED; Node* n = N(c, argv[0]);
    CSTR(pos, 1); CSTR(s, 2);
    if(n && pos && s){
        Node *parent = NULL, *before = NULL;
        if(str_ieq(pos, "beforebegin")){ parent = n->parent; before = n; }
        else if(str_ieq(pos, "afterbegin")){ parent = n; before = n->first; }
        else if(str_ieq(pos, "beforeend")){ parent = n; before = NULL; }
        else if(str_ieq(pos, "afterend")){ parent = n->parent; before = n->next; }
        if(parent){
            Node* mark = before ? before->prev : parent->last;
            html_parse_into(doc, parent, before, s, (int)s_len);
            Node* x = mark ? mark->next : parent->first;
            for(; x && x != before; x = x->next) for(Node* y = x; y; y = dom_next_in_tree(y, x)) if(dom_is(y, "script")) y->flags |= NF_SCRIPT_DONE;
        }
        dirty();
    }
    FREE(pos); FREE(s);
    return JS_UNDEFINED;
}
ARGS(n_qsa){
    UNUSED; Node* root = N(c, argv[0]);
    CSTR(sel, 1);
    int first = argc > 2 && JS_ToBool(c, argv[2]);
    if(!root || !sel){ FREE(sel); return JS_NewArray(c); }
    int cap = first ? 1 : 100000;
    Node** out = (Node**)malloc(sizeof(Node*) * (size_t)(first ? 1 : (doc->nnodes + 1)));
    int n = css_query(root, sel, out, cap);
    FREE(sel);
    if(n < 0){ free(out); return JS_NULL; }
    JSValue a = JS_NewArray(c);
    for(int i = 0; i < n; i++) JS_SetPropertyUint32(c, a, (u32)i, JS_NewInt32(c, out[i]->id));
    free(out);
    return a;
}
ARGS(n_matches){
    UNUSED; Node* n = N(c, argv[0]);
    CSTR(sel, 1);
    int r = n && sel && n->type == N_ELEM ? css_match_list(n, sel) : 0;
    FREE(sel);
    return JS_NewInt32(c, r);
}
ARGS(n_by_id){
    UNUSED; CSTR(id, 0);
    Node* n = id ? dom_by_id(doc->root, id) : NULL;
    FREE(id);
    return ID(n);
}
ARGS(n_rect){
    UNUSED; Node* n = N(c, argv[0]);
    JSValue a = JS_NewArray(c);
    i32 v[4] = {0, 0, 0, 0};
    if(n && n->cs){ v[0] = n->bx; v[1] = n->by; v[2] = n->bw; v[3] = n->bh; }
    for(int i = 0; i < 4; i++) JS_SetPropertyUint32(c, a, (u32)i, JS_NewInt32(c, v[i]));
    return a;
}
ARGS(n_computed){
    UNUSED; Node* n = N(c, argv[0]);
    CSTR(p, 1);
    Buf b = {0};
    if(n && p) css_computed(n, p, &b);
    FREE(p);
    JSValue r = STR(c, b.p, b.n); buf_free(&b);
    return r;
}
ARGS(n_value){
    UNUSED; Node* n = N(c, argv[0]);
    if(!n) return JS_NewString(c, "");
    if(n->value) return JS_NewString(c, n->value);
    if(dom_is(n, "textarea")){ Buf b = {0}; dom_text_content(n, &b); JSValue r = STR(c, b.p, b.n); buf_free(&b); return r; }
    const char* v = dom_attr(n, "value");
    if(!v && dom_is(n, "option")){ Buf b = {0}; dom_text_content(n, &b); JSValue r = STR(c, b.p, b.n); buf_free(&b); return r; }
    return JS_NewString(c, v ? v : "");
}
ARGS(n_set_value){
    UNUSED; Node* n = N(c, argv[0]);
    CSTR(s, 1);
    if(n && s){ n->value = arena_strndup(&doc->arena, s, s_len); dirty(); }
    FREE(s);
    return JS_UNDEFINED;
}
ARGS(n_checked){ UNUSED; Node* n = N(c, argv[0]); return JS_NewBool(c, n && (n->checked == 1 || (n->checked == 0 && dom_attr(n, "checked")))); }
ARGS(n_set_checked){
    UNUSED; Node* n = N(c, argv[0]);
    if(n){ n->checked = JS_ToBool(c, argv[1]) ? 1 : -1; dirty(); }
    return JS_UNDEFINED;
}
ARGS(n_clone){ UNUSED; Node* n = N(c, argv[0]); return ID(n ? dom_clone(doc, n, JS_ToBool(c, argv[1])) : NULL); }
ARGS(n_doc){ UNUSED; (void)argv; return ID(doc->root); }
ARGS(n_url){ UNUSED; (void)argv; return JS_NewString(c, vela_url()); }
ARGS(n_set_url){ UNUSED; CSTR(u, 0); if(u) vela_set_url(u); FREE(u); return JS_UNDEFINED; }
ARGS(n_resolve){
    UNUSED; CSTR(base, 0); CSTR(ref, 1);
    char out[URLMAX];
    int ok = ref && url_resolve(base ? base : vela_url(), ref, out, sizeof out);
    FREE(base); FREE(ref);
    return ok ? JS_NewString(c, out) : JS_NULL;
}
ARGS(n_navigate){
    UNUSED; CSTR(u, 0);
    if(u){
        char out[URLMAX];
        if(str_istarts(u, "javascript:")) js_eval_url(u + 11);
        else if(url_resolve(vela_url(), u, out, sizeof out)) vela_navigate(out, NULL, NULL, 0, NULL);
    }
    FREE(u);
    return JS_UNDEFINED;
}
ARGS(n_back){ UNUSED; (void)argv; vela_navigate("vela:back", NULL, NULL, 0, NULL); return JS_UNDEFINED; }
ARGS(n_title){ UNUSED; (void)argv; return JS_NewString(c, doc->title); }
ARGS(n_set_title){
    UNUSED; CSTR(t, 0);
    if(t){ snprintf(doc->title, sizeof doc->title, "%s", t); vela_title_changed(); }
    FREE(t);
    return JS_UNDEFINED;
}
ARGS(n_log){
    UNUSED; int32_t lvl = 0; JS_ToInt32(c, &lvl, argv[0]);
    CSTR(m, 1);
    printf("[vela/js] %s%s\n", lvl >= 2 ? "erro: " : lvl == 1 ? "aviso: " : "", m ? m : "");
    if(lvl >= 2 && m) snprintf(js_status, sizeof js_status, "Console: %s", m);
    FREE(m);
    return JS_UNDEFINED;
}
ARGS(n_alert){
    UNUSED; CSTR(m, 0);
    vela_status(m ? m : "");          /* aparece num balao na janela */
    printf("[vela/js] alert: %s\n", m ? m : "");
    FREE(m);
    return JS_UNDEFINED;
}
ARGS(n_now){ UNUSED; (void)argv; return JS_NewInt64(c, (i64)sys_time_ms()); }
ARGS(n_fetch){
    UNUSED; CSTR(u, 0); CSTR(m, 1); CSTR(body, 2); CSTR(ct, 3);
    JSValue r = JS_NULL;
    if(u){
        Resp rs;
        if(net_fetch(u, m ? m : "GET", body, body ? (int)body_len : 0, ct, &rs)){
            r = JS_NewArray(c);
            JS_SetPropertyUint32(c, r, 0, JS_NewInt32(c, rs.status));
            JS_SetPropertyUint32(c, r, 1, STR(c, rs.body, rs.len));
            JS_SetPropertyUint32(c, r, 2, JS_NewString(c, rs.url));
            JS_SetPropertyUint32(c, r, 3, JS_NewString(c, rs.ctype));
        }
        resp_free(&rs);
        deadline = sys_time_ms() + 5000;          /* a rede nao conta no tempo do script */
    }
    FREE(u); FREE(m); FREE(body); FREE(ct);
    return r;
}
ARGS(n_cookie){
    UNUSED; (void)argv;
    Url u; Buf b = {0};
    if(url_parse(vela_url(), &u)) cookies_get(u.host, u.path, &b);
    JSValue r = STR(c, b.p, b.n); buf_free(&b);
    return r;
}
ARGS(n_set_cookie){
    UNUSED; CSTR(s, 0);
    Url u; if(s && url_parse(vela_url(), &u)) cookies_set(u.host, s);
    FREE(s);
    return JS_UNDEFINED;
}
ARGS(n_viewport){
    UNUSED; (void)argv;
    JSValue a = JS_NewArray(c);
    JS_SetPropertyUint32(c, a, 0, JS_NewInt32(c, view_w));
    JS_SetPropertyUint32(c, a, 1, JS_NewInt32(c, view_h));
    JS_SetPropertyUint32(c, a, 2, JS_NewInt32(c, scroll_y));
    return a;
}
ARGS(n_scroll_to){ UNUSED; int32_t y = 0; JS_ToInt32(c, &y, argv[0]); vela_scroll_to(y); return JS_UNDEFINED; }
ARGS(n_media){ UNUSED; CSTR(q, 0); int r = q ? css_media_ok(q, (int)q_len) : 0; FREE(q); return JS_NewBool(c, r); }
ARGS(n_focus){ UNUSED; Node* n = N(c, argv[0]); if(n){ doc->focus = (dom_is(n, "input") || dom_is(n, "textarea")) ? n : NULL; dirty(); } return JS_UNDEFINED; }
ARGS(n_focused){ UNUSED; (void)argv; return ID(doc->focus); }
ARGS(n_hit){
    UNUSED; int32_t x = 0, y = 0; JS_ToInt32(c, &x, argv[0]); JS_ToInt32(c, &y, argv[1]);
    return ID(vela_hit(x, y));
}
ARGS(n_submit){ UNUSED; Node* f = N(c, argv[0]); if(f) vela_submit(f, NULL); return JS_UNDEFINED; }
ARGS(n_current_script){ UNUSED; (void)argv; return ID(cur_script); }
ARGS(n_write){
    UNUSED; CSTR(s, 0);
    if(s && loading && cur_script && cur_script->parent){
        Node* parent = cur_script->parent;
        Node* after = write_after ? write_after : cur_script;
        Node* before = after->next;
        html_parse_into(doc, parent, before, s, (int)s_len);
        Node* last = before ? before->prev : parent->last;
        for(Node* x = after->next; x && x != before; x = x->next) check_scripts(x);
        write_after = last;
        dirty();
    }
    FREE(s);
    return JS_UNDEFINED;
}
ARGS(n_random){
    UNUSED; int32_t n = 0; JS_ToInt32(c, &n, argv[0]);
    if(n < 0 || n > 4096) n = 0;
    u8 buf[4096]; sys_getrandom(buf, (u64)n);
    JSValue a = JS_NewArray(c);
    for(int i = 0; i < n; i++) JS_SetPropertyUint32(c, a, (u32)i, JS_NewInt32(c, buf[i]));
    return a;
}

static const struct { const char* name; JSCFunction* fn; int n; } NATIVES[] = {
    {"nodeType", n_node_type, 1}, {"tag", n_tag, 1}, {"parent", n_parent, 1}, {"first", n_first, 1},
    {"last", n_last, 1}, {"next", n_next, 1}, {"prev", n_prev, 1}, {"attr", n_attr, 2},
    {"setAttr", n_set_attr, 3}, {"delAttr", n_del_attr, 2}, {"attrs", n_attrs, 1}, {"text", n_text, 1},
    {"setText", n_set_text, 2}, {"create", n_create, 1}, {"createText", n_create_text, 1},
    {"createComment", n_create_comment, 1}, {"createFrag", n_create_frag, 0}, {"insert", n_insert, 3},
    {"remove", n_remove, 1}, {"html", n_html, 2}, {"setHtml", n_set_html, 2}, {"insertHtml", n_insert_html, 3},
    {"qsa", n_qsa, 3}, {"matches", n_matches, 2}, {"byId", n_by_id, 1}, {"rect", n_rect, 1},
    {"computed", n_computed, 2}, {"value", n_value, 1}, {"setValue", n_set_value, 2},
    {"checked", n_checked, 1}, {"setChecked", n_set_checked, 2}, {"clone", n_clone, 2}, {"doc", n_doc, 0},
    {"url", n_url, 0}, {"setUrl", n_set_url, 1}, {"resolve", n_resolve, 2}, {"navigate", n_navigate, 1},
    {"back", n_back, 0}, {"title", n_title, 0}, {"setTitle", n_set_title, 1}, {"log", n_log, 2},
    {"alert", n_alert, 1}, {"now", n_now, 0}, {"fetch", n_fetch, 4}, {"cookie", n_cookie, 0},
    {"setCookie", n_set_cookie, 1}, {"viewport", n_viewport, 0}, {"scrollTo", n_scroll_to, 1},
    {"media", n_media, 1}, {"focus", n_focus, 1}, {"focused", n_focused, 0}, {"hit", n_hit, 2},
    {"submit", n_submit, 1}, {"currentScript", n_current_script, 0}, {"write", n_write, 1},
    {"random", n_random, 1},
};

/* ------------------------------------------------------------ modulos */
/* import maps: "especificador" -> URL (prefixos terminados em '/') */
static struct { char* key; char* url; } imap[64];
static int nimap = 0;

void js_set_importmap(const char* json){
    /* procura o objeto "imports": { "k": "v", ... } (JSON simples) */
    const char* p = strstr(json, "\"imports\"");
    if(!p) return;
    p = strchr(p + 9, '{');
    if(!p) return;
    p++;
    while(*p && *p != '}' && nimap < 64){
        const char* k0 = strchr(p, '"'); if(!k0) break;
        const char* k1 = strchr(k0 + 1, '"'); if(!k1) break;
        const char* v0 = strchr(k1 + 1, '"'); if(!v0) break;
        const char* v1 = strchr(v0 + 1, '"'); if(!v1) break;
        const char* brace = strchr(k1, '}');
        if(brace && brace < v0) break;
        imap[nimap].key = strndup(k0 + 1, (size_t)(k1 - k0 - 1));
        char u[URLMAX], raw[URLMAX];
        snprintf(raw, sizeof raw, "%.*s", (int)(v1 - v0 - 1), v0 + 1);
        imap[nimap].url = strdup(url_resolve(vela_url(), raw, u, sizeof u) ? u : raw);
        nimap++;
        p = v1 + 1;
    }
}

static char* mod_normalize(JSContext* c, const char* base, const char* name, void* o){
    (void)o;
    char out[URLMAX];
    int bare = !(name[0] == '.' || name[0] == '/' || str_istarts(name, "http://") || str_istarts(name, "https://"));
    if(bare){
        for(int i = 0; i < nimap; i++){
            int kl = (int)strlen(imap[i].key);
            if(!strcmp(name, imap[i].key)) return js_strdup(c, imap[i].url);
            if(kl && imap[i].key[kl - 1] == '/' && !strncmp(name, imap[i].key, (size_t)kl)){
                snprintf(out, sizeof out, "%s%s", imap[i].url, name + kl);
                return js_strdup(c, out);
            }
        }
    }
    const char* b = base && (str_istarts(base, "http://") || str_istarts(base, "https://")) ? base : vela_url();
    if(!url_resolve(b, name, out, sizeof out)) return js_strdup(c, name);
    char* h = strchr(out, '#'); if(h) *h = 0;
    return js_strdup(c, out);
}

static JSModuleDef* mod_loader(JSContext* c, const char* name, void* o){
    (void)o;
    Resp r;
    if(!net_fetch(name, "GET", NULL, 0, NULL, &r) || r.status != 200){
        resp_free(&r);
        JS_ThrowReferenceError(c, "nao foi possivel carregar o modulo '%s'", name);
        return NULL;
    }
    JSValue f = JS_Eval(c, r.body, (size_t)r.len, name, JS_EVAL_TYPE_MODULE | JS_EVAL_FLAG_COMPILE_ONLY);
    resp_free(&r);
    deadline = sys_time_ms() + 15000;
    if(JS_IsException(f)) return NULL;
    JSModuleDef* m = (JSModuleDef*)JS_VALUE_GET_PTR(f);
    JSValue meta = JS_GetImportMeta(c, m);
    JS_SetPropertyStr(c, meta, "url", JS_NewString(c, name));
    JS_FreeValue(c, meta);
    JS_FreeValue(c, f);
    return m;
}

/* executa um <script type=module>; espera a promessa da avaliacao */
void js_run_module(const char* src, int len, const char* name, Node* script){
    if(!ctx) return;
    cur_script = NULL; write_after = NULL;
    if(script) script->flags |= NF_SCRIPT_DONE;
    deadline = sys_time_ms() + 15000;
    char* s = (char*)malloc((size_t)len + 1);
    memcpy(s, src, (size_t)len); s[len] = 0;
    JSValue r = JS_Eval(ctx, s, (size_t)len, name, JS_EVAL_TYPE_MODULE | JS_EVAL_FLAG_COMPILE_ONLY);
    free(s);
    if(!JS_IsException(r)){
        JSValue meta = JS_GetImportMeta(ctx, (JSModuleDef*)JS_VALUE_GET_PTR(r));
        char u[URLMAX]; snprintf(u, sizeof u, "%s", name);
        if(!script || !dom_attr(script, "src")){ char* h = strchr(u, '#'); if(h) *h = 0; }
        JS_SetPropertyStr(ctx, meta, "url", JS_NewString(ctx, u));
        JS_FreeValue(ctx, meta);
        r = JS_EvalFunction(ctx, r);
    }
    if(JS_IsException(r)) report(ctx);
    else {
        run_jobs();
        if(JS_PromiseState(ctx, r) == JS_PROMISE_REJECTED){
            JSValue e = JS_PromiseResult(ctx, r);
            JS_Throw(ctx, e);
            report(ctx);
        }
    }
    JS_FreeValue(ctx, r);
    run_jobs();
    deadline = 0;
}

/* ------------------------------------------------------------ controle */
static JSValue call_global(const char* fn, int argc, JSValueConst* argv){
    JSValue g = JS_GetGlobalObject(ctx);
    JSValue f = JS_GetPropertyStr(ctx, g, fn);
    JSValue r = JS_UNDEFINED;
    if(JS_IsFunction(ctx, f)){
        r = JS_Call(ctx, f, g, argc, argv);
        if(JS_IsException(r)){ report(ctx); r = JS_UNDEFINED; }
    }
    JS_FreeValue(ctx, f);
    JS_FreeValue(ctx, g);
    return r;
}

void js_start(Doc* d){
    js_stop();
    (void)d;
    js_status[0] = 0;
    rt = JS_NewRuntime();
    if(!rt) return;
    /* limite do heap do JS: ~60% da memoria livre (entre 24 e 160 MB) */
    SysInfo si; memset(&si, 0, sizeof si); sys_sysinfo(&si);
    u64 lim = (u64)si.mem_free_kb * 1024 * 6 / 10;
    if(lim < 24u * 1024 * 1024) lim = 24u * 1024 * 1024;
    if(lim > 160u * 1024 * 1024) lim = 160u * 1024 * 1024;
    JS_SetMemoryLimit(rt, (size_t)lim);
    printf("[vela] JS: limite de memoria %u MB (livre: %u MB)\n", (unsigned)(lim >> 20), si.mem_free_kb / 1024);
    JS_SetMaxStackSize(rt, 640 * 1024);
    JS_SetInterruptHandler(rt, interrupt, NULL);
    JS_SetModuleLoaderFunc(rt, mod_normalize, mod_loader, NULL);
    JS_SetHostPromiseRejectionTracker(rt, rejection, NULL);
    for(int i = 0; i < nimap; i++){ free(imap[i].key); free(imap[i].url); }
    nimap = 0;
    ctx = JS_NewContext(rt);
    if(!ctx){ JS_FreeRuntime(rt); rt = NULL; return; }
    JSValue g = JS_GetGlobalObject(ctx);
    JSValue v = JS_NewObject(ctx);
    for(size_t i = 0; i < sizeof NATIVES / sizeof NATIVES[0]; i++)
        JS_SetPropertyStr(ctx, v, NATIVES[i].name, JS_NewCFunction(ctx, NATIVES[i].fn, NATIVES[i].name, NATIVES[i].n));
    JS_SetPropertyStr(ctx, g, "__vela", v);
    JS_FreeValue(ctx, g);
    deadline = sys_time_ms() + 5000;
    JSValue r = JS_Eval(ctx, (const char*)vela_prelude, vela_prelude_len, "prelude.js", JS_EVAL_TYPE_GLOBAL);
    if(JS_IsException(r)) report(ctx);
    JS_FreeValue(ctx, r);
    deadline = 0;
    loading = 1;
    nscript_q = 0;
}

void js_stop(void){
    if(ctx){ JS_FreeContext(ctx); ctx = NULL; }
    if(rt){ JS_FreeRuntime(rt); rt = NULL; }
    cur_script = NULL; write_after = NULL; loading = 0;
    nscript_q = 0;
}

void js_run_script(const char* src, int len, const char* name, Node* script){
    if(!ctx) return;
    cur_script = script; write_after = NULL;
    if(script) script->flags |= NF_SCRIPT_DONE;
    deadline = sys_time_ms() + 15000;
    /* o QuickJS exige NUL no fim do fonte */
    char* s = (char*)malloc((size_t)len + 1);
    memcpy(s, src, (size_t)len); s[len] = 0;
    JSValue r = JS_Eval(ctx, s, (size_t)len, name, JS_EVAL_TYPE_GLOBAL);
    free(s);
    if(JS_IsException(r)) report(ctx);
    JS_FreeValue(ctx, r);
    run_jobs();
    deadline = 0;
    cur_script = NULL;
}

void js_fire_loaded(void){
    if(!ctx) return;
    loading = 0;
    deadline = sys_time_ms() + 5000;
    JSValue r = call_global("__vela_loaded", 0, NULL);
    JS_FreeValue(ctx, r);
    run_jobs();
    deadline = 0;
}

int js_dispatch(Node* target, const char* type, i32 x, i32 y){
    if(!ctx || !target) return 0;
    deadline = sys_time_ms() + 3000;
    JSValue a[4] = { JS_NewInt32(ctx, target->id), JS_NewString(ctx, type), JS_NewInt32(ctx, x), JS_NewInt32(ctx, y) };
    JSValue r = call_global("__vela_dispatch", 4, a);
    int prevented = JS_ToBool(ctx, r) > 0;
    JS_FreeValue(ctx, r);
    JS_FreeValue(ctx, a[1]);
    run_jobs();
    deadline = 0;
    return prevented;
}

void js_fire(Node* target, const char* type){
    if(!ctx || !target) return;
    deadline = sys_time_ms() + 3000;
    JSValue a[2] = { JS_NewInt32(ctx, target->id), JS_NewString(ctx, type) };
    JSValue r = call_global("__vela_fire", 2, a);
    JS_FreeValue(ctx, r);
    JS_FreeValue(ctx, a[1]);
    run_jobs();
    deadline = 0;
}

int js_key(Node* target, const char* key, int keycode){
    if(!ctx) return 0;
    deadline = sys_time_ms() + 3000;
    JSValue a[3] = { JS_NewInt32(ctx, target ? target->id : 0), JS_NewString(ctx, key), JS_NewInt32(ctx, keycode) };
    JSValue r = call_global("__vela_key", 3, a);
    int prevented = JS_ToBool(ctx, r) > 0;
    JS_FreeValue(ctx, r);
    JS_FreeValue(ctx, a[1]);
    run_jobs();
    deadline = 0;
    return prevented;
}

int js_tick(u64 now){
    if(!ctx) return -1;
    deadline = sys_time_ms() + 3000;
    JSValue a[1] = { JS_NewInt64(ctx, (i64)now) };
    JSValue r = call_global("__vela_tick", 1, a);
    int32_t next = -1;
    if(!JS_IsUndefined(r)) JS_ToInt32(ctx, &next, r);
    JS_FreeValue(ctx, r);
    run_jobs();
    deadline = 0;
    return next;
}

int js_eval_url(const char* code){
    if(!ctx) return 0;
    /* decodifica %XX */
    char* s = (char*)malloc(strlen(code) + 1); int k = 0;
    for(const char* p = code; *p; p++){
        if(*p == '%' && p[1] && p[2]){ char h[3] = {p[1], p[2], 0}; s[k++] = (char)strtol(h, NULL, 16); p += 2; }
        else s[k++] = *p;
    }
    s[k] = 0;
    deadline = sys_time_ms() + 3000;
    JSValue r = JS_Eval(ctx, s, (size_t)k, "javascript:", JS_EVAL_TYPE_GLOBAL);
    free(s);
    if(JS_IsException(r)) report(ctx);
    JS_FreeValue(ctx, r);
    run_jobs();
    deadline = 0;
    return 1;
}
