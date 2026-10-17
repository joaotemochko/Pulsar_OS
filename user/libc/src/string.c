/* string.c — memoria, strings e formatacao minimas. */
#include <string.h>
#include <stdlib.h>
#include "pulsar.h"

/* As rotinas de memoria trabalham com palavras de 64 bits (4 por volta)
 * depois de alinhar; sob emulacao cada instrucao conta, e o QuickJS, o DOM
 * e os decodificadores de imagem chamam estas funcoes o tempo todo. */
typedef u64 __attribute__((may_alias)) w64;

void* memcpy(void* d, const void* s, size_t n){
    u8* dd=(u8*)d; const u8* ss=(const u8*)s;
    if(n>=16 && (((u64)dd^(u64)ss)&7)==0){
        while((u64)dd&7){ *dd++=*ss++; n--; }
        while(n>=32){
            w64 a=((const w64*)ss)[0], b=((const w64*)ss)[1], c=((const w64*)ss)[2], e=((const w64*)ss)[3];
            ((w64*)dd)[0]=a; ((w64*)dd)[1]=b; ((w64*)dd)[2]=c; ((w64*)dd)[3]=e;
            dd+=32; ss+=32; n-=32;
        }
        while(n>=8){ *(w64*)dd=*(const w64*)ss; dd+=8; ss+=8; n-=8; }
    } else if(n>=16 && (((u64)dd^(u64)ss)&3)==0){
        while((u64)dd&3){ *dd++=*ss++; n--; }
        while(n>=4){ *(u32*)dd=*(const u32*)ss; dd+=4; ss+=4; n-=4; }
    }
    while(n--) *dd++=*ss++;
    return d;
}
void* memmove(void* d, const void* s, size_t n){
    u8* dd=(u8*)d; const u8* ss=(const u8*)s;
    if(dd==ss || n==0) return d;
    if(dd<ss || dd>=ss+n) return memcpy(d,s,n);
    /* sobreposto com destino depois da origem: copia de tras para frente */
    dd+=n; ss+=n;
    if(n>=16 && (((u64)dd^(u64)ss)&7)==0){
        while((u64)dd&7){ *--dd=*--ss; n--; }
        while(n>=8){ dd-=8; ss-=8; *(w64*)dd=*(const w64*)ss; n-=8; }
    }
    while(n--) *--dd=*--ss;
    return d;
}
void* memset(void* d, int c, size_t n){
    u8* dd=(u8*)d; u8 v=(u8)c;
    if(n>=16){
        while((u64)dd&7){ *dd++=v; n--; }
        u64 w=v*0x0101010101010101ULL;
        while(n>=32){ ((w64*)dd)[0]=w; ((w64*)dd)[1]=w; ((w64*)dd)[2]=w; ((w64*)dd)[3]=w; dd+=32; n-=32; }
        while(n>=8){ *(w64*)dd=w; dd+=8; n-=8; }
    }
    while(n--) *dd++=v;
    return d;
}
int memcmp(const void* a, const void* b, size_t n){
    const u8* x=(const u8*)a; const u8* y=(const u8*)b;
    if(n>=16 && (((u64)x^(u64)y)&7)==0){
        while(((u64)x&7) && n){ if(*x!=*y) return *x-*y; x++; y++; n--; }
        while(n>=8 && *(const w64*)x==*(const w64*)y){ x+=8; y+=8; n-=8; }
    }
    for(size_t i=0;i<n;i++) if(x[i]!=y[i]) return x[i]-y[i];
    return 0;
}
size_t strlen(const char* s){
    const char* p=s;
    while((u64)p&7){ if(!*p) return (size_t)(p-s); p++; }
    /* uma palavra tem byte zero se (w - 0x01..) & ~w & 0x80.. != 0 */
    const w64* w=(const w64*)p;
    while(!((*w-0x0101010101010101ULL) & ~*w & 0x8080808080808080ULL)) w++;
    p=(const char*)w;
    while(*p) p++;
    return (size_t)(p-s);
}
int strcmp(const char* a, const char* b){ while(*a && *a==*b){a++;b++;} return (u8)*a-(u8)*b; }
int strncmp(const char* a, const char* b, size_t n){
    for(size_t i=0;i<n;i++){ if(a[i]!=b[i]||!a[i]) return (u8)a[i]-(u8)b[i]; }
    return 0;
}
char* strcpy(char* d, const char* s){ char* r=d; while((*d++=*s++)!=0){} return r; }
void* memchr(const void* s, int c, size_t n){
    const u8* p=(const u8*)s;
    for(size_t i=0;i<n;i++) if(p[i]==(u8)c) return (void*)(p+i);
    return NULL;
}
size_t strnlen(const char* s, size_t n){ size_t i=0; while(i<n && s[i]) i++; return i; }
int strcoll(const char* a, const char* b){ return strcmp(a,b); }
char* strncpy(char* d, const char* s, size_t n){
    size_t i=0;
    for(;i<n && s[i];i++) d[i]=s[i];
    for(;i<n;i++) d[i]=0;
    return d;
}
char* strcat(char* d, const char* s){ strcpy(d+strlen(d),s); return d; }
char* strncat(char* d, const char* s, size_t n){
    char* e=d+strlen(d); size_t i=0;
    for(;i<n && s[i];i++) e[i]=s[i];
    e[i]=0; return d;
}
char* strchr(const char* s, int c){
    for(;;s++){ if(*s==(char)c) return (char*)s; if(!*s) return NULL; }
}
char* strrchr(const char* s, int c){
    const char* r=NULL;
    for(;;s++){ if(*s==(char)c) r=s; if(!*s) return (char*)r; }
}
char* strstr(const char* h, const char* n){
    size_t k=strlen(n);
    if(!k) return (char*)h;
    for(;*h;h++) if(*h==*n && strncmp(h,n,k)==0) return (char*)h;
    return NULL;
}
size_t strspn(const char* s, const char* set){
    size_t i=0; while(s[i] && strchr(set,s[i])) i++; return i;
}
size_t strcspn(const char* s, const char* set){
    size_t i=0; while(s[i] && !strchr(set,s[i])) i++; return i;
}
char* strpbrk(const char* s, const char* set){
    s+=strcspn(s,set); return *s?(char*)s:NULL;
}
char* strndup(const char* s, size_t n){
    size_t k=strnlen(s,n);
    char* d=(char*)malloc(k+1);
    if(d){ memcpy(d,s,k); d[k]=0; }
    return d;
}
char* strdup(const char* s){ return strndup(s,(size_t)-1); }
static int lower_(int c){ return c>='A'&&c<='Z'?c+32:c; }
int strcasecmp(const char* a, const char* b){
    while(*a && lower_((u8)*a)==lower_((u8)*b)){a++;b++;}
    return lower_((u8)*a)-lower_((u8)*b);
}
int strncasecmp(const char* a, const char* b, size_t n){
    for(size_t i=0;i<n;i++){
        int x=lower_((u8)a[i]), y=lower_((u8)b[i]);
        if(x!=y||!x) return x-y;
    }
    return 0;
}
char* strerror(int e){ (void)e; return (char*)"erro"; }
void puts_(const char* s){ sys_write(s, strlen(s)); }

char* fmt_s(char* out, const char* s){ while(*s) *out++=*s++; *out=0; return out; }
char* fmt_u(char* out, u64 v){
    char t[24]; int n=0;
    if(v==0) t[n++]='0';
    while(v){ t[n++]='0'+(int)(v%10); v/=10; }
    while(n) *out++=t[--n];
    *out=0; return out;
}
char* fmt_i(char* out, i64 v){
    if(v<0){ *out++='-'; return fmt_u(out,(u64)(-v)); }
    return fmt_u(out,(u64)v);
}
char* fmt_size(char* out, u64 b){
    if(b<1024){ out=fmt_u(out,b); return fmt_s(out," B"); }
    if(b<1024*1024){ out=fmt_u(out,(b+1023)/1024); return fmt_s(out," KB"); }
    u64 t=b*10/(1024*1024);
    out=fmt_u(out,t/10); *out++=','; out=fmt_u(out,t%10); return fmt_s(out," MB");
}
int utf8_encode(u32 cp, char* o){
    if(cp<0x80){ o[0]=(char)cp; return 1; }
    if(cp<0x800){ o[0]=(char)(0xC0|(cp>>6)); o[1]=(char)(0x80|(cp&0x3F)); return 2; }
    if(cp<0x10000){ o[0]=(char)(0xE0|(cp>>12)); o[1]=(char)(0x80|((cp>>6)&0x3F)); o[2]=(char)(0x80|(cp&0x3F)); return 3; }
    o[0]=(char)(0xF0|(cp>>18)); o[1]=(char)(0x80|((cp>>12)&0x3F)); o[2]=(char)(0x80|((cp>>6)&0x3F)); o[3]=(char)(0x80|(cp&0x3F)); return 4;
}
void civil_time(u64 secs, u32* y, u32* mo, u32* d, u32* h, u32* mi, u32* s, u32* wd){
    i64 days=(i64)(secs/86400); u64 rem=secs%86400;
    *wd=(u32)((days+4)%7);
    i64 z=days+719468;
    i64 era=(z>=0?z:z-146096)/146097;
    i64 doe=z-era*146097;
    i64 yoe=(doe-doe/1460+doe/36524-doe/146096)/365;
    i64 yy=yoe+era*400;
    i64 doy=doe-(365*yoe+yoe/4-yoe/100);
    i64 mp=(5*doy+2)/153;
    *d=(u32)(doy-(153*mp+2)/5+1);
    *mo=(u32)(mp<10?mp+3:mp-9);
    *y=(u32)(*mo<=2?yy+1:yy);
    *h=(u32)(rem/3600); *mi=(u32)((rem/60)%60); *s=(u32)(rem%60);
}
