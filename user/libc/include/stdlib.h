/* stdlib.h — libc do Pulsar (malloc.c, stdlib.c). */
#ifndef _STDLIB_H
#define _STDLIB_H
#include <stddef.h>

#define EXIT_SUCCESS 0
#define EXIT_FAILURE 1
#define RAND_MAX 0x7FFFFFFF

void*  malloc(size_t n);
void   free(void* p);
void*  calloc(size_t n, size_t sz);
void*  realloc(void* p, size_t n);
size_t malloc_usable_size(void* p);

_Noreturn void abort(void);
_Noreturn void exit(int code);
char*  getenv(const char* name);
int    atoi(const char* s);
long   atol(const char* s);
double atof(const char* s);
long   strtol(const char* s, char** end, int base);
unsigned long strtoul(const char* s, char** end, int base);
long long strtoll(const char* s, char** end, int base);
unsigned long long strtoull(const char* s, char** end, int base);
double strtod(const char* s, char** end);
float  strtof(const char* s, char** end);
int    abs(int v);
long   labs(long v);
long long llabs(long long v);
void   qsort(void* base, size_t n, size_t sz, int (*cmp)(const void*, const void*));
void*  bsearch(const void* key, const void* base, size_t n, size_t sz, int (*cmp)(const void*, const void*));
int    rand(void);
void   srand(unsigned seed);

#define alloca(n) __builtin_alloca(n)
#endif
