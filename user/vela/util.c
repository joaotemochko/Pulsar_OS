/* util.c — arenas, buffers dinamicos e utilidades de texto. */
#include "vela.h"

/* ---- arena: blocos encadeados; tudo e liberado de uma vez ---- */
struct ArenaChunk { ArenaChunk* next; size_t used, cap; };
#define CHUNK_DEFAULT ((size_t)128 * 1024)

void* arena_alloc(Arena* a, size_t n){
    n = (n + 15) & ~(size_t)15;
    ArenaChunk* c = a->head;
    if(!c || c->used + n > c->cap){
        size_t cap = n > CHUNK_DEFAULT / 2 ? n : CHUNK_DEFAULT;
        c = (ArenaChunk*)malloc(sizeof(ArenaChunk) + 16 + cap);
        if(!c){ puts_("[vela] sem memoria\n"); abort(); }
        c->used = 0; c->cap = cap;
        /* um bloco grande avulso nao toma o lugar do bloco corrente */
        if(a->head && cap != CHUNK_DEFAULT){ c->next = a->head->next; a->head->next = c; }
        else { c->next = a->head; a->head = c; }
        a->total += cap;
    }
    u8* base = (u8*)(((u64)(c + 1) + 15) & ~(u64)15);
    void* p = base + c->used;
    c->used += n;
    memset(p, 0, n);
    return p;
}
char* arena_strndup(Arena* a, const char* s, size_t n){
    char* d = (char*)arena_alloc(a, n + 1);
    memcpy(d, s, n); d[n] = 0;
    return d;
}
char* arena_strdup(Arena* a, const char* s){ return arena_strndup(a, s, strlen(s)); }
void arena_reset(Arena* a){
    ArenaChunk* c = a->head;
    while(c){ ArenaChunk* n = c->next; free(c); c = n; }
    a->head = NULL; a->total = 0;
}

/* ---- buffer dinamico ---- */
static void buf_grow(Buf* b, int need){
    if(b->n + need + 1 <= b->cap) return;
    int cap = b->cap ? b->cap : 256;
    while(cap < b->n + need + 1) cap *= 2;
    char* p = (char*)realloc(b->p, (size_t)cap);
    if(!p){ puts_("[vela] sem memoria (buf)\n"); abort(); }
    b->p = p; b->cap = cap;
}
void buf_put(Buf* b, const void* s, int n){
    if(n <= 0) return;
    buf_grow(b, n);
    memcpy(b->p + b->n, s, (size_t)n); b->n += n; b->p[b->n] = 0;
}
void buf_putc(Buf* b, char c){ buf_grow(b, 1); b->p[b->n++] = c; b->p[b->n] = 0; }
void buf_puts(Buf* b, const char* s){ buf_put(b, s, (int)strlen(s)); }
void buf_printf(Buf* b, const char* fmt, ...){
    va_list ap; va_start(ap, fmt);
    char tmp[512];
    int n = vsnprintf(tmp, sizeof tmp, fmt, ap);
    va_end(ap);
    if(n < (int)sizeof tmp){ buf_put(b, tmp, n); return; }
    buf_grow(b, n);
    va_start(ap, fmt);
    vsnprintf(b->p + b->n, (size_t)n + 1, fmt, ap);
    va_end(ap);
    b->n += n;
}
char* buf_cstr(Buf* b){ buf_grow(b, 0); b->p[b->n] = 0; return b->p; }
void buf_free(Buf* b){ free(b->p); b->p = NULL; b->n = b->cap = 0; }

/* ---- texto ---- */
int str_ieq(const char* a, const char* b){
    while(*a && lower((u8)*a) == lower((u8)*b)){ a++; b++; }
    return *a == 0 && *b == 0;
}
int str_ieqn(const char* a, const char* b, int n){
    for(int i = 0; i < n; i++){ if(!b[i] || lower((u8)a[i]) != lower((u8)b[i])) return 0; }
    return b[n] == 0;
}
int str_istarts(const char* s, const char* p){
    while(*p){ if(lower((u8)*s) != lower((u8)*p)) return 0; s++; p++; }
    return 1;
}
const char* str_icontains(const char* h, const char* n){
    for(; *h; h++) if(str_istarts(h, n)) return h;
    return NULL;
}
u32 utf8_dec(const char* s, int n, int* i){
    u8 b = (u8)s[*i]; (*i)++;
    if(b < 0x80) return b;
    int len; u32 cp;
    if((b & 0xE0) == 0xC0){ len = 1; cp = b & 0x1F; }
    else if((b & 0xF0) == 0xE0){ len = 2; cp = b & 0x0F; }
    else if((b & 0xF8) == 0xF0){ len = 3; cp = b & 0x07; }
    else return 0xFFFD;
    for(int k = 0; k < len; k++){
        if(*i >= n) return 0xFFFD;
        u8 c = (u8)s[*i];
        if((c & 0xC0) != 0x80) return 0xFFFD;
        cp = (cp << 6) | (c & 0x3F); (*i)++;
    }
    return cp;
}
/* windows-1252/latin-1 -> UTF-8 (para paginas antigas) */
char* utf8_from_latin1(const char* s, int n, int* outn){
    static const u16 w1252[32] = {
        0x20AC, 0x81, 0x201A, 0x192, 0x201E, 0x2026, 0x2020, 0x2021, 0x2C6, 0x2030, 0x160, 0x2039, 0x152, 0x8D, 0x17D, 0x8F,
        0x90, 0x2018, 0x2019, 0x201C, 0x201D, 0x2022, 0x2013, 0x2014, 0x2DC, 0x2122, 0x161, 0x203A, 0x153, 0x9D, 0x17E, 0x178 };
    char* o = (char*)malloc((size_t)n * 3 + 1);
    int k = 0;
    for(int i = 0; i < n; i++){
        u32 c = (u8)s[i];
        if(c >= 0x80 && c < 0xA0) c = w1252[c - 0x80];
        k += utf8_encode(c, o + k);
    }
    o[k] = 0; *outn = k;
    return o;
}
u32 hash_str(const char* s, int n){
    u32 h = 2166136261u;
    for(int i = 0; i < n; i++){ h ^= (u8)s[i]; h *= 16777619u; }
    return h;
}
