/* vela.h — tipos e interfaces compartilhados pelos modulos do Vela.
 *
 * Fluxo de uma pagina:
 *   net (HTTP/HTTPS) -> dom (parser HTML) -> css (cascata, estilo computado)
 *   -> layout (caixas -> display list) -> paint (pixels na janela)
 *   e o js (QuickJS) altera o DOM, o que dispara novo estilo + layout.
 */
#ifndef VELA_H
#define VELA_H
#include <stddef.h>
#include <stdint.h>
#include <stdarg.h>
#include <string.h>
#include <stdlib.h>
#include <stdio.h>
#include "pulsar.h"

#define URLMAX 2048

/* ------------------------------------------------------------ util.c */
typedef struct ArenaChunk ArenaChunk;
typedef struct { ArenaChunk* head; size_t total; } Arena;
void* arena_alloc(Arena* a, size_t n);              /* zerado, alinhado a 16 */
char* arena_strndup(Arena* a, const char* s, size_t n);
char* arena_strdup(Arena* a, const char* s);
void  arena_reset(Arena* a);

typedef struct { char* p; int n, cap; } Buf;
void  buf_put(Buf* b, const void* s, int n);
void  buf_putc(Buf* b, char c);
void  buf_puts(Buf* b, const char* s);
void  buf_printf(Buf* b, const char* fmt, ...) __attribute__((format(printf, 2, 3)));
char* buf_cstr(Buf* b);                             /* garante o NUL final */
void  buf_free(Buf* b);

static inline int lower(int c){ return c >= 'A' && c <= 'Z' ? c + 32 : c; }
static inline int is_ws(int c){ return c == ' ' || c == '\t' || c == '\n' || c == '\r' || c == '\f'; }
int   str_ieq(const char* a, const char* b);
int   str_ieqn(const char* a, const char* b, int n);   /* a tem n bytes, b e C string */
int   str_istarts(const char* s, const char* prefix);
const char* str_icontains(const char* hay, const char* needle);
u32   utf8_dec(const char* s, int n, int* i);          /* com limite n */
char* utf8_from_latin1(const char* s, int n, int* outn);
u32   hash_str(const char* s, int n);

/* ------------------------------------------------------------- url.c */
typedef struct {
    int  https;
    char host[256];
    int  port;
    char path[URLMAX];      /* caminho + query, sem o fragmento */
    char frag[256];
} Url;
int  url_parse(const char* s, Url* u);                 /* so http(s) absolutas */
void url_format(const Url* u, char* out, int cap);     /* sem o fragmento */
int  url_resolve(const char* base, const char* ref, char* out, int cap);
void url_from_input(const char* typed, char* out, int cap);
void url_encode_component(Buf* b, const char* s, int n);

/* ------------------------------------------------------------- net.c */
typedef struct {
    int   status;
    char* body; int len;
    char  ctype[128];
    char  url[URLMAX];       /* URL final (apos redirecionamentos) */
    char  err[200];
} Resp;
int  net_fetch(const char* url, const char* method, const char* body, int body_len,
               const char* body_ctype, Resp* r);       /* 1 ok (qualquer status), 0 erro */
/* varias requisicoes em paralelo (conexoes simultaneas + keep-alive) */
typedef struct { const char* url; const char* method; const char* body; int body_len; const char* body_ctype; Resp r; int ok; } Fetch;
void net_fetch_many(Fetch* f, int n);

/* ------------------------------------------------- work.c (threads) */
typedef struct Task {
    void (*run)(struct Task*);      /* etapa pesada num worker (opcional) */
    void (*done)(struct Task*);     /* entrega na thread principal */
    struct Task* next;
} Task;
void work_init(void);
void work_submit(Task* t);          /* run num worker, depois done na principal */
void work_submit_front(Task* t);       /* na frente da fila (trabalho que a principal espera) */
void work_post_main(Task* t);
void work_hold(void);               /* conta uma entrega pendente */
int  work_busy(void);               /* ha entregas pendentes? */
int  work_poll(void);               /* principal: executa os `done` prontos */

/* pedido assincrono a thread de rede */
typedef struct Req {
    Task task;                      /* deve ser o primeiro campo (Task* -> Req*) */
    Fetch f;
    char url[URLMAX];
    int prio;                       /* 0 pagina/CSS/scripts, 1 imagens */
    u32 gen;                        /* geracao da pagina (net_gen) */
    volatile u32 fin;               /* 1 quando a rede terminou */
    void* user; int kind;
    struct Req* qnext;
} Req;
extern volatile u32 net_gen;        /* muda a cada navegacao: pedidos velhos na fila sao descartados */
Req* req_new(const char* url, int prio);
void req_free(Req* r);
void net_submit(Req* r);
void net_wait(Req* r);
void net_init(void);
void resp_free(Resp* r);
void cookies_get(const char* host, const char* path, Buf* out);
void cookies_set(const char* host, const char* header);
int  dns_resolve(const char* name, u32* ip);

/* ------------------------------------------------------------- tls.c */
typedef struct Tls Tls;
Tls* tls_new(const char* host);                        /* sem I/O ainda */
int  tls_io(Tls* t, i64 sock, const u8* out, int outn, int* done, Buf* in, int* prog);  /* 0 segue, 1 fim, -1 erro */
int  tls_ready(Tls* t);                       /* handshake concluido */
int  tls_has_session(const char* host);        /* ha sessao para retomar */
int  tls_error(Tls* t);
void tls_close(Tls* t);
const char* tls_error_text(int code);

/* ------------------------------------------------------------- dom.c */
enum { N_ELEM = 1, N_TEXT = 3, N_COMMENT = 8, N_DOC = 9, N_FRAG = 11 };
enum { NF_SCRIPT_DONE = 1, NF_JS_INSERTED = 2, NF_SHEET_TRIED = 4, NF_SHEET_DUP = 8, NF_PSEUDO = 16 };
typedef struct { const char* name; const char* value; } Attr;
typedef struct Style Style;
typedef struct Node Node;
struct Node {
    u8    type;
    u8    hover;
    u8    flags;                        /* NF_* */
    int   id;
    const char* tag;                    /* minusculo (elementos) */
    char* text; int tlen;               /* texto/comentario */
    Attr* attrs; int nattr, cattr;
    Node *parent, *first, *last, *prev, *next;
    Style* cs;                          /* estilo computado (NULL = sem caixa) */
    i32   bx, by, bw, bh;               /* caixa da borda no ultimo layout */
    u32   mgen; i32 mmin, mmax;         /* cache de medidas (min/max-content) */
    u32   lgen;                         /* passo de layout que preencheu bx..bh */
    char* value;                        /* <input>/<textarea>: valor atual */
    int   checked;
    struct Image* img;
    struct Sheet* sheet;                /* <style>/<link>: folha ja analisada */
    u32   sheet_hash;
    Node* pse[2];                       /* ::before / ::after gerados (vivem so ate o proximo estilo) */
};
typedef struct {
    Node*  root;                        /* nodo documento */
    Node** byid; int nnodes, cnodes;
    Arena  arena;
    char   url[URLMAX];                 /* base para URLs relativas */
    char   title[256];
    int    dirty;                       /* DOM mudou: refazer estilo e layout */
    struct Sheet* sheets;               /* folhas do autor */
    Node*  focus;                       /* campo de texto com foco */
    int    quirks;                      /* sem <!doctype html>: modo de compatibilidade */
} Doc;
extern Doc* doc;

Doc*  doc_new(const char* url);
void  doc_free(Doc* d);
Node* dom_new(Doc* d, int type, const char* tag);
Node* dom_new_text(Doc* d, const char* s, int n);
Node* dom_get(Doc* d, int id);
void  dom_append(Node* parent, Node* child);
void  dom_insert(Node* parent, Node* child, Node* before);
void  dom_remove(Node* child);
const char* dom_attr(const Node* n, const char* name);
void  dom_set_attr(Doc* d, Node* n, const char* name, const char* value);
void  dom_del_attr(Node* n, const char* name);
int   dom_is(const Node* n, const char* tag);
Node* dom_find(Node* root, const char* tag);
Node* dom_by_id(Node* root, const char* id);
Node* dom_body(Doc* d);
Node* dom_head(Doc* d);
Node* dom_next_in_tree(Node* n, Node* stop);
void  dom_text_content(const Node* n, Buf* out);
void  dom_set_text(Doc* d, Node* n, const char* s, int len);
void  dom_serialize(const Node* n, Buf* out, int outer);
Node* dom_clone(Doc* d, Node* n, int deep);
Doc*  html_parse(const char* src, int len, const char* url);
void  html_parse_into(Doc* d, Node* parent, Node* before, const char* src, int len);
int   html_is_void(const char* tag);

/* ------------------------------------------------------------- css.c */
/* comprimento: px + pct% do bloco que contem (calc() vira essa soma) */
typedef struct { i32 px; i32 pct; u8 kind; } Len;      /* pct em centesimos de % */
enum { L_AUTO = 0, L_VAL = 1, L_NONE = 2 };
static inline Len len_px(i32 v){ Len l = { v, 0, L_VAL }; return l; }
static inline i32 len_res(Len l, i32 base){ return l.px + (i32)((i64)l.pct * base / 10000); }

enum { D_NONE, D_INLINE, D_BLOCK, D_INLINE_BLOCK, D_LIST_ITEM, D_FLEX, D_INLINE_FLEX,
       D_GRID, D_INLINE_GRID, D_TABLE, D_INLINE_TABLE, D_ROW_GROUP, D_ROW, D_CELL,
       D_CAPTION, D_CONTENTS };
enum { TA_LEFT, TA_RIGHT, TA_CENTER, TA_JUSTIFY };
enum { WS_NORMAL, WS_NOWRAP, WS_PRE, WS_PRE_WRAP, WS_PRE_LINE };
enum { POS_STATIC, POS_RELATIVE, POS_ABSOLUTE, POS_FIXED, POS_STICKY };
enum { LS_DISC, LS_CIRCLE, LS_SQUARE, LS_DECIMAL, LS_NONE, LS_ALPHA, LS_ROMAN };
enum { JC_START, JC_END, JC_CENTER, JC_BETWEEN, JC_AROUND, JC_EVENLY };
enum { AI_STRETCH, AI_START, AI_END, AI_CENTER, AI_BASELINE };
enum { VA_BASELINE, VA_TOP, VA_MIDDLE, VA_BOTTOM, VA_SUB, VA_SUPER };

typedef struct Var { const char* name; const char* value; u32 hash; } Var;
/* escopo de propriedades --custom: tabela hash propria + o escopo do pai */
typedef struct VarScope { struct VarScope* parent; Var* tab; int n, cap; } VarScope;
const char* css_var(const Style* st, const char* name, int n);

struct Style {
    u8  display, position, floating, text_align, white_space, list_style;
    u8  bold, italic, mono, underline, strike, upper, lower, capitalize;
    u8  fam, ffb;                             /* familia da fonte e a embutida de reserva (FAM_*) */
    u16 weight;                               /* font-weight 100..900 (bold = weight >= 600) */
    u8  visibility, overflow_hidden, box_border, flex_dir_col, flex_wrap, justify, align_items;
    u8  align_self, valign, hidden_clip, break_words, ellipsis, cursor_pointer, masked;
    i16 font_size, line_height;               /* px; line_height 0 = normal */
    u32 color;                                /* 0xAARRGGBB */
    u32 bg;                                   /* alpha 0 = transparente */
    struct Image* bg_img; const char* bg_url;
    /* camada de imagem de fundo e de mascara: tamanho (szk 0 auto, 1 cover,
     * 2 contain, 3 explicito), posicao (pct = da sobra caixa - imagem), repeticao (bit0 x, bit1 y) */
    u8  bg_szk, bg_rep, mask_szk, mask_rep;
    Len bg_sw, bg_sh, bg_px, bg_py;
    struct Image* mask_img; const char* mask_url;
    const char* bg_grad;                      /* linear-/radial-gradient() de fundo (texto) */
    Len mask_sw, mask_sh, mask_px, mask_py;
    Len margin[4], padding[4];                /* top right bottom left */
    i16 border_w[4]; u32 border_c[4];
    i16 radius;
    Len width, height, min_w, max_w, min_h, max_h;
    Len inset[4];                             /* top right bottom left */
    Len gap_row, gap_col, flex_basis;
    Len tx, ty;                               /* transform: translate (pct = da propria caixa) */
    i16 flex_grow, flex_shrink;               /* x100 */
    i16 order, z_index;
    const char* grid_cols;                    /* grid-template-columns cru */
    i16 grid_span;                            /* grid-column: 1 / -1 -> -1 */
    i16 opacity;                              /* 0..255 */
    i16 indent, letter_spacing;
    VarScope* vars;                           /* propriedades --custom */
    const char* content;                      /* ::before/::after: texto gerado (NULL = sem caixa) */
    u8  clear;                                /* 1 left, 2 right, 3 both */
    u8  flow_root;                            /* display: flow-root / overflow != visible: novo contexto de floats */
};

typedef struct Sheet Sheet;
void   css_init(void);                        /* folha do navegador (UA) */
Sheet* css_parse_sheet(Arena* a, const char* src, int len, int origin);
void   css_add_sheet(Doc* d, Sheet* s);
int    css_set_sheets(Doc* d, Sheet** list, int n);  /* troca a lista; 1 se mudou */
void   css_forget_doc(void);                  /* pagina trocada: indice refeito do zero */
void   css_sheet_set_base(Sheet* sh, Arena* a, const char* url);   /* URL da folha (para @font-face) */
void   css_style_doc(Doc* d, i32 vw, i32 vh); /* cascata em toda a arvore */
u32    css_parse_color(const char* s, int n, int* ok);
int    css_match_list(Node* el, const char* selector);   /* -1 seletor invalido */
int    css_query(Node* root, const char* selector, Node** out, int max);
int    css_media_ok(const char* q, int n);
void   css_computed(Node* n, const char* prop, Buf* out);
extern Arena frame_arena;                     /* estilos + layout: refeito a cada passo */
extern i32 css_vw, css_vh;

/* ------------------------------------------------------------ font.c */
typedef struct FontInst FontInst;
/* familias embutidas; as fontes web (@font-face) vem depois */
enum { FAM_SANS, FAM_MONO, FAM_SERIF, FAM_ARIAL, FAM_TIMES, FAM_BUILTIN };
FontInst* font_get(int fam, int fallback_fam, int weight, int italic, int px);   /* weight 100..900 */
int  font_family_id(const char* name);                  /* nome minusculo -> familia web, -1 se nao ha */
void font_face_add(const char* family, const char* url, int wlo, int whi, int italic);   /* @font-face (wlo..whi: fonte variavel) */
int  font_pump(void);                                   /* 1 se alguma fonte web chegou (refazer layout) */
int  font_pending(void);
i32  font_line_gap(FontInst* f);
i32  font_width(FontInst* f, const char* s, int n);     /* px */
i32  font_ascent(FontInst* f);
i32  font_descent(FontInst* f);                         /* positivo */
i32  font_px(FontInst* f);
void font_draw(Fb* fb, FontInst* f, i32 x, i32 baseline, const char* s, int n, u32 color, i32 ls);   /* ls: letter-spacing */
void font_reset_cache(void);

/* ----------------------------------------------------------- image.c */
typedef struct Image {
    char url[URLMAX];
    int  w, h;                 /* tamanho intrinseco (0 enquanto nao carregou) */
    int  pw, ph;               /* tamanho dos pixels guardados (pode ser reduzido) */
    u32* px;                   /* ARGB */
    int  state;                /* 0 pendente, 1 ok, 2 erro */
    int  orphan;               /* pagina trocada com a imagem em voo: liberar na entrega */
    struct Image* next;
} Image;
Image* image_get(const char* url);            /* entra na fila se novo */
int    image_pump(void);                      /* 1 se alguma imagem chegou desde a ultima chamada */
int    image_pending(void);
void   image_reset(void);
void   image_draw(Fb* fb, Image* im, Rect dst, u8 alpha, int cover);
void   image_draw_tiled(Fb* fb, Image* im, Rect box, i32 ox, i32 oy, i32 tw, i32 th, int rep, u8 alpha, int mask, u32 color);
Image* image_svg_inline(const char* key, const char* svg, int len, u32 color);
void   svg_serialize(const Node* n, Buf* out);

/* ----------------------------------------------------------- layout.c */
enum { DL_RECT, DL_BORDER, DL_TEXT, DL_IMAGE, DL_CLIP, DL_UNCLIP, DL_HIT, DL_CARET, DL_BGIMAGE, DL_MASK, DL_GRAD };
typedef struct Grad Grad;
Grad* grad_build(Arena* ar, const char* spec, i32 w, i32 h);
void  grad_draw(Fb* fb, Grad* g, Rect box, i32 radius, u8 alpha);
typedef struct {
    u8  kind; u8 alpha; u8 fixed;        /* fixed: nao rola com a pagina */
    i32 x, y, w, h;
    u32 color;
    i16 radius;
    FontInst* font; const char* s; int n;
    Node* node;
    u8  bw[4]; u32 bc[4];
    Image* img;
    i16 ls;                              /* texto: letter-spacing (px) */
    i32 ix, iy, iw, ih; u8 rep;          /* DL_BGIMAGE/DL_MASK: origem e tamanho do ladrilho, repeticao */
    Grad* grad;                          /* DL_GRAD */
} DItem;
typedef struct { DItem* it; int n, cap; i32 width, height; u32 bg; Node* bg_node; } DList;
void layout_doc(Doc* d, DList* dl, i32 vw, i32 vh);
Node* layout_hit(DList* dl, i32 x, i32 y, i32 scroll);   /* elemento no ponto (coords da vista) */

/* ------------------------------------------------------------ paint.c */
void paint(Fb* fb, DList* dl, Rect view, Rect clip, i32 scroll_y, u32 page_bg);   /* so dentro de clip */

/* --------------------------------------------------------------- js.c */
void js_start(Doc* d);                        /* novo contexto para a pagina */
void js_stop(void);
int  js_enabled(void);
void js_run_script(const char* src, int len, const char* name, Node* script);
void js_run_module(const char* src, int len, const char* name, Node* script);
void js_set_importmap(const char* json);
void js_fire_loaded(void);                    /* DOMContentLoaded + load */
int  js_dispatch(Node* target, const char* type, i32 x, i32 y);  /* 1 = preventDefault */
int  js_tick(u64 now);                        /* roda timers; ret ms ate o proximo (-1 nenhum) */
int  js_eval_url(const char* code);           /* href="javascript:..." */
void js_fire(Node* target, const char* type);  /* evento simples (load/error), sem bolha */
int  js_key(Node* target, const char* key, int keycode);   /* keydown; 1 = preventDefault */
Node* js_next_script(void);                    /* <script> inserido pelo JS, a executar */
extern char js_status[160];                   /* ultima mensagem (alert/erro) */

/* ------------------------------------------------- main.c (navegacao) */
void vela_navigate(const char* url, const char* method, const char* body, int body_len, const char* ctype);
void vela_status(const char* msg);
const char* vela_url(void);                   /* URL da pagina exibida */
void vela_set_url(const char* url);           /* history.pushState */
void vela_title_changed(void);
Node* vela_hit(i32 x, i32 y);                 /* coords da vista */
void vela_submit(Node* form, Node* submitter);
extern i32 view_w, view_h, scroll_y;
void vela_scroll_to(i32 y);
#endif
