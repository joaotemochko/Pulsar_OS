/* string.c — memória e strings mínimas. */
#include "pulsar.h"

void* memcpy(void* d, const void* s, size_t n){
    u8* dd=(u8*)d; const u8* ss=(const u8*)s;
    /* cópia por palavras quando alinhado (rápido) */
    while(n>=8 && ((u64)dd&7)==0 && ((u64)ss&7)==0){
        *(u64*)dd=*(const u64*)ss; dd+=8; ss+=8; n-=8;
    }
    while(n--) *dd++=*ss++;
    return d;
}
void* memset(void* d, int c, size_t n){
    u8* dd=(u8*)d; u8 v=(u8)c;
    while(n--) *dd++=v;
    return d;
}
size_t strlen(const char* s){ const char* p=s; while(*p) p++; return (size_t)(p-s); }
void puts_(const char* s){ sys_write(s, strlen(s)); }
