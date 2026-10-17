/* malloc.c — alocador de memoria dinamica da libc do Pulsar.
 *
 * O heap cresce pela syscall SBRK (paginas novas a partir de 0x10_0000_0000,
 * devolvidas ao sistema quando o processo termina). Cada bloco tem um
 * cabecalho de 16 bytes com o seu tamanho total e alinhamento de 16 bytes.
 *
 * - Blocos pequenos (ate SMALL_MAX) usam classes de 16 em 16 bytes: cada
 *   classe tem uma lista livre propria, entao malloc/free sao O(1). Eles
 *   saem de "arenas" de 64 KiB e nunca voltam ao heap grande (motores como
 *   o QuickJS fazem milhoes dessas alocacoes curtas).
 * - Blocos grandes ficam numa lista ordenada por endereco, com fusao de
 *   vizinhos no free; a busca e first-fit e blocos grandes sao divididos.
 *
 * Com threads (thread.c) as operacoes ficam sob um mutex; antes da primeira
 * thread o mutex nem e tocado.
 */
#include <stdlib.h>
#include <string.h>
#include "pulsar.h"

typedef struct Block { size_t size; struct Block* next; } Block;   /* size inclui o cabecalho */

#define HDR        ((size_t)sizeof(Block))
#define ALIGN16(n) (((n) + 15) & ~(size_t)15)
#define MIN_GROW   ((size_t)256 * 1024)
#define SMALL_MAX  ((size_t)1024)                 /* maior bloco pequeno (com cabecalho) */
#define NCLASS     (SMALL_MAX / 16)
#define ARENA      ((size_t)64 * 1024)

static Block* free_list = NULL;                   /* blocos grandes livres */
static Block* bins[NCLASS + 1];                   /* bins[k]: blocos pequenos de k*16 bytes */
static u8* arena_cur = NULL;
static u8* arena_end = NULL;

/* insere na lista (ordem de endereco) fundindo com os vizinhos */
static void insert_free(Block* b){
    Block* prev = NULL;
    Block* cur = free_list;
    while(cur && cur < b){ prev = cur; cur = cur->next; }
    b->next = cur;
    if(prev) prev->next = b; else free_list = b;
    if(cur && (u8*)b + b->size == (u8*)cur){
        b->size += cur->size;
        b->next = cur->next;
    }
    if(prev && (u8*)prev + prev->size == (u8*)b){
        prev->size += b->size;
        prev->next = b->next;
    }
}

static int grow(size_t need){
    size_t n = need < MIN_GROW ? MIN_GROW : ALIGN16(need);
    i64 old = sys_sbrk(n);
    if(old < 0) return 0;
    Block* b = (Block*)(u64)old;
    b->size = n;
    insert_free(b);
    return 1;
}

static void* large_alloc(size_t need){
    for(int attempt = 0; attempt < 2; attempt++){
        Block* prev = NULL;
        for(Block* b = free_list; b; prev = b, b = b->next){
            if(b->size < need) continue;
            if(b->size - need >= HDR + SMALL_MAX){
                /* divide: a sobra continua livre no lugar do bloco */
                Block* rest = (Block*)((u8*)b + need);
                rest->size = b->size - need;
                rest->next = b->next;
                if(prev) prev->next = rest; else free_list = rest;
                b->size = need;
            } else {
                if(prev) prev->next = b->next; else free_list = b->next;
            }
            return b;
        }
        if(!grow(need)) return NULL;
    }
    return NULL;
}

static void* malloc_unlocked(size_t n);
static void free_unlocked(void* p);
static Mutex heap_lock = MUTEX_INIT;

void* malloc(size_t n){
    if(!threads_started) return malloc_unlocked(n);
    mutex_lock(&heap_lock);
    void* p = malloc_unlocked(n);
    mutex_unlock(&heap_lock);
    return p;
}
void free(void* p){
    if(!p) return;
    if(!threads_started){ free_unlocked(p); return; }
    mutex_lock(&heap_lock);
    free_unlocked(p);
    mutex_unlock(&heap_lock);
}

static void* malloc_unlocked(size_t n){
    if(n == 0) n = 1;
    if(n > ((size_t)1 << 40)) return NULL;
    size_t need = ALIGN16(n) + HDR;
    if(need <= SMALL_MAX){
        size_t k = need / 16;
        Block* b = bins[k];
        if(b){ bins[k] = b->next; return (u8*)b + HDR; }
        if(arena_cur + need > arena_end){
            /* o resto da arena velha vira blocos das classes menores */
            while(arena_end - arena_cur >= 32){
                size_t r = (size_t)(arena_end - arena_cur);
                if(r > SMALL_MAX) r = SMALL_MAX;
                r &= ~(size_t)15;
                Block* f = (Block*)arena_cur; f->size = r; f->next = bins[r / 16]; bins[r / 16] = f;
                arena_cur += r;
            }
            Block* a = large_alloc(ARENA);
            if(!a) return NULL;
            arena_cur = (u8*)a; arena_end = (u8*)a + a->size;
        }
        b = (Block*)arena_cur; arena_cur += need;
        b->size = need;
        return (u8*)b + HDR;
    }
    Block* b = large_alloc(need);
    return b ? (u8*)b + HDR : NULL;
}

static void free_unlocked(void* p){
    if(!p) return;
    Block* b = (Block*)((u8*)p - HDR);
    if(b->size <= SMALL_MAX){ b->next = bins[b->size / 16]; bins[b->size / 16] = b; return; }
    insert_free(b);
}

size_t malloc_usable_size(void* p){
    return p ? ((Block*)((u8*)p - HDR))->size - HDR : 0;
}

void* calloc(size_t n, size_t sz){
    if(sz && n > ((size_t)-1) / sz) return NULL;
    void* p = malloc(n * sz);
    if(p) memset(p, 0, n * sz);
    return p;
}

void* realloc(void* p, size_t n){
    if(!p) return malloc(n);
    if(n == 0){ free(p); return NULL; }
    size_t have = malloc_usable_size(p);
    if(have >= n) return p;
    void* q = malloc(n);
    if(!q) return NULL;
    memcpy(q, p, have);
    free(p);
    return q;
}
