/* stdlib.c — conversoes numericas, ordenacao, aleatorios e saida. */
#include <stdlib.h>
#include <string.h>
#include <ctype.h>
#include <errno.h>
#include <stdio.h>
#include "pulsar.h"

int errno;

_Noreturn void exit(int code){ (void)code; sys_exit(); }
_Noreturn void abort(void){
    puts_("[libc] abort()\n");
    sys_exit();
}
_Noreturn void __assert_fail(const char* expr, const char* file, int line){
    fprintf(stderr, "[libc] assert falhou: %s (%s:%d)\n", expr, file, line);
    sys_exit();
}
char* getenv(const char* name){ (void)name; return NULL; }
int abs(int v){ return v < 0 ? -v : v; }
long labs(long v){ return v < 0 ? -v : v; }
long long llabs(long long v){ return v < 0 ? -v : v; }

static unsigned long long parse_u(const char* s, char** end, int base, int* neg, int* ovf){
    const char* p = s;
    while(isspace((u8)*p)) p++;
    *neg = 0; *ovf = 0;
    if(*p == '+' || *p == '-'){ *neg = *p == '-'; p++; }
    if((base == 0 || base == 16) && p[0] == '0' && (p[1] | 32) == 'x' && isxdigit((u8)p[2])){ p += 2; base = 16; }
    else if(base == 0) base = p[0] == '0' ? 8 : 10;
    unsigned long long v = 0; const char* start = p;
    for(;; p++){
        int c = (u8)*p, d;
        if(isdigit(c)) d = c - '0';
        else if(isalpha(c)) d = (c | 32) - 'a' + 10;
        else break;
        if(d >= base) break;
        if(v > (~0ULL - (unsigned)d) / (unsigned)base) *ovf = 1;
        v = v * (unsigned)base + (unsigned)d;
    }
    if(end) *end = (char*)(p == start ? s : p);
    return v;
}
long long strtoll(const char* s, char** end, int base){
    int neg, ovf; unsigned long long v = parse_u(s, end, base, &neg, &ovf);
    if(ovf || v > (neg ? 0x8000000000000000ULL : 0x7FFFFFFFFFFFFFFFULL)){
        errno = ERANGE; return neg ? (long long)0x8000000000000000ULL : 0x7FFFFFFFFFFFFFFFLL;
    }
    return neg ? -(long long)v : (long long)v;
}
unsigned long long strtoull(const char* s, char** end, int base){
    int neg, ovf; unsigned long long v = parse_u(s, end, base, &neg, &ovf);
    if(ovf){ errno = ERANGE; return ~0ULL; }
    return neg ? -v : v;
}
long strtol(const char* s, char** end, int base){ return (long)strtoll(s, end, base); }
unsigned long strtoul(const char* s, char** end, int base){ return (unsigned long)strtoull(s, end, base); }
int atoi(const char* s){ return (int)strtol(s, NULL, 10); }
long atol(const char* s){ return strtol(s, NULL, 10); }

double pow(double, double);
/* decimal -> double: mantissa de ate 19 digitos escalada por 10^exp
 * (pode errar o ultimo bit; o QuickJS usa o seu proprio js_atod). */
double strtod(const char* s, char** end){
    const char* p = s;
    while(isspace((u8)*p)) p++;
    int neg = 0;
    if(*p == '+' || *p == '-'){ neg = *p == '-'; p++; }
    if(strncasecmp(p, "inf", 3) == 0){
        p += 3; if(strncasecmp(p, "inity", 5) == 0) p += 5;
        if(end) *end = (char*)p;
        return neg ? -__builtin_inf() : __builtin_inf();
    }
    if(strncasecmp(p, "nan", 3) == 0){ if(end) *end = (char*)p + 3; return __builtin_nan(""); }
    unsigned long long m = 0; int digits = 0, exp10 = 0, any = 0;
    for(; isdigit((u8)*p); p++){ any = 1; if(digits < 19){ m = m * 10 + (unsigned)(*p - '0'); if(m) digits++; } else exp10++; }
    if(*p == '.'){
        p++;
        for(; isdigit((u8)*p); p++){ any = 1; if(digits < 19){ m = m * 10 + (unsigned)(*p - '0'); if(m) digits++; exp10--; } }
    }
    if(!any){ if(end) *end = (char*)s; return 0; }
    if((*p | 32) == 'e'){
        const char* q = p + 1; int eneg = 0, e = 0;
        if(*q == '+' || *q == '-'){ eneg = *q == '-'; q++; }
        if(isdigit((u8)*q)){
            for(; isdigit((u8)*q); q++) if(e < 10000) e = e * 10 + (*q - '0');
            exp10 += eneg ? -e : e; p = q;
        }
    }
    if(end) *end = (char*)p;
    double v = (double)m;
    if(exp10) v = exp10 < 0 ? v / pow(10.0, -exp10) : v * pow(10.0, exp10);
    return neg ? -v : v;
}
float strtof(const char* s, char** end){ return (float)strtod(s, end); }
double atof(const char* s){ return strtod(s, NULL); }

/* ordenacao: insercao para trechos curtos, senao quicksort (mediana de 3) */
static void swap_bytes(u8* a, u8* b, size_t n){ while(n--){ u8 t = *a; *a++ = *b; *b++ = t; } }
void qsort(void* base, size_t n, size_t sz, int (*cmp)(const void*, const void*)){
    u8* b = (u8*)base;
    while(n > 8){
        size_t mid = n / 2;
        if(cmp(b + mid * sz, b) < 0) swap_bytes(b + mid * sz, b, sz);
        if(cmp(b + (n - 1) * sz, b) < 0) swap_bytes(b + (n - 1) * sz, b, sz);
        if(cmp(b + (n - 1) * sz, b + mid * sz) < 0) swap_bytes(b + (n - 1) * sz, b + mid * sz, sz);
        swap_bytes(b + mid * sz, b + (n - 1) * sz, sz);        /* pivo no fim */
        u8* piv = b + (n - 1) * sz;
        size_t i = 0;
        for(size_t j = 0; j < n - 1; j++)
            if(cmp(b + j * sz, piv) < 0){ swap_bytes(b + i * sz, b + j * sz, sz); i++; }
        swap_bytes(b + i * sz, piv, sz);
        /* recursao no lado menor, laco no maior */
        if(i < n - i - 1){ qsort(b, i, sz, cmp); b += (i + 1) * sz; n -= i + 1; }
        else { qsort(b + (i + 1) * sz, n - i - 1, sz, cmp); n = i; }
    }
    for(size_t i = 1; i < n; i++)
        for(size_t j = i; j > 0 && cmp(b + (j - 1) * sz, b + j * sz) > 0; j--)
            swap_bytes(b + (j - 1) * sz, b + j * sz, sz);
}
void* bsearch(const void* key, const void* base, size_t n, size_t sz, int (*cmp)(const void*, const void*)){
    size_t lo = 0, hi = n;
    while(lo < hi){
        size_t mid = (lo + hi) / 2;
        const u8* e = (const u8*)base + mid * sz;
        int c = cmp(key, e);
        if(c == 0) return (void*)e;
        if(c < 0) hi = mid; else lo = mid + 1;
    }
    return NULL;
}

static unsigned long long rng_state = 0x9E3779B97F4A7C15ULL;
void srand(unsigned seed){ rng_state = seed * 0x9E3779B97F4A7C15ULL + 1; }
int rand(void){
    rng_state ^= rng_state << 13; rng_state ^= rng_state >> 7; rng_state ^= rng_state << 17;
    return (int)(rng_state >> 33);
}
