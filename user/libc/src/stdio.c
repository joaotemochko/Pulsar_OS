/* stdio.c — printf e companhia sobre o stb_sprintf (dominio publico).
 * Toda saida vai para a serial (SYS_WRITE); nao ha arquivos. */
#include <stdio.h>
#include <string.h>
#include <stdlib.h>
#include "pulsar.h"

#define STB_SPRINTF_IMPLEMENTATION
#define STB_SPRINTF_NOUNALIGNED
#include "stb_sprintf.h"

struct _PFILE { int fd; };
static FILE f_in = {0}, f_out = {1}, f_err = {2};
FILE* stdin = &f_in;
FILE* stdout = &f_out;
FILE* stderr = &f_err;

int vsnprintf(char* out, size_t n, const char* fmt, va_list ap){
    return stbsp_vsnprintf(out, (int)(n > 0x7FFFFFFF ? 0x7FFFFFFF : n), fmt, ap);
}
int vsprintf(char* out, const char* fmt, va_list ap){ return stbsp_vsprintf(out, fmt, ap); }
int snprintf(char* out, size_t n, const char* fmt, ...){
    va_list ap; va_start(ap, fmt);
    int r = vsnprintf(out, n, fmt, ap);
    va_end(ap); return r;
}
int sprintf(char* out, const char* fmt, ...){
    va_list ap; va_start(ap, fmt);
    int r = stbsp_vsprintf(out, fmt, ap);
    va_end(ap); return r;
}

static char* flush_cb(const char* buf, void* user, int len){
    sys_write(buf, (u64)len);
    *(int*)user += len;
    return (char*)buf;
}
int vfprintf(FILE* f, const char* fmt, va_list ap){
    (void)f;
    char buf[STB_SPRINTF_MIN];
    int total = 0;
    stbsp_vsprintfcb(flush_cb, &total, buf, fmt, ap);
    return total;
}
int vprintf(const char* fmt, va_list ap){ return vfprintf(stdout, fmt, ap); }
int printf(const char* fmt, ...){
    va_list ap; va_start(ap, fmt);
    int r = vfprintf(stdout, fmt, ap);
    va_end(ap); return r;
}
int fprintf(FILE* f, const char* fmt, ...){
    va_list ap; va_start(ap, fmt);
    int r = vfprintf(f, fmt, ap);
    va_end(ap); return r;
}
int fputs(const char* s, FILE* f){ (void)f; sys_write(s, strlen(s)); return 0; }
int puts(const char* s){ sys_write(s, strlen(s)); sys_write("\n", 1); return 0; }
int fputc(int c, FILE* f){ (void)f; char ch = (char)c; sys_write(&ch, 1); return c; }
int putc(int c, FILE* f){ return fputc(c, f); }
int putchar(int c){ return fputc(c, stdout); }
size_t fwrite(const void* p, size_t sz, size_t n, FILE* f){ (void)f; sys_write(p, sz * n); return n; }
int fflush(FILE* f){ (void)f; return 0; }

FILE* fopen(const char* path, const char* mode){ (void)path; (void)mode; return NULL; }
int fclose(FILE* f){ (void)f; return 0; }
size_t fread(void* p, size_t sz, size_t n, FILE* f){ (void)p; (void)sz; (void)n; (void)f; return 0; }
int fseek(FILE* f, long off, int whence){ (void)f; (void)off; (void)whence; return -1; }
long ftell(FILE* f){ (void)f; return -1; }

/* sscanf minimo: %d %i %u %x %o %f %g %e %s %c %n %%, largura e '*' */
int vsscanf(const char* s, const char* fmt, va_list ap){
    const char* p = s;
    int count = 0;
    while(*fmt){
        char f = *fmt;
        if(f == ' ' || f == '\t' || f == '\n'){ while(*p == ' ' || (*p >= '\t' && *p <= '\r')) p++; fmt++; continue; }
        if(f != '%'){ if(*p != f) break; p++; fmt++; continue; }
        fmt++;
        if(*fmt == '%'){ if(*p != '%') break; p++; fmt++; continue; }
        int skip = 0, width = 0, lng = 0;
        if(*fmt == '*'){ skip = 1; fmt++; }
        while(*fmt >= '0' && *fmt <= '9') width = width * 10 + (*fmt++ - '0');
        while(*fmt == 'l' || *fmt == 'h' || *fmt == 'z'){ if(*fmt == 'l') lng++; fmt++; }
        char c = *fmt++;
        if(c == 'n'){ if(!skip) *va_arg(ap, int*) = (int)(p - s); continue; }
        if(c != 'c') while(*p == ' ' || (*p >= '\t' && *p <= '\r')) p++;
        if(!*p) break;
        char buf[128]; int k = 0, lim = width ? width : 127;
        if(lim > 127) lim = 127;
        if(c == 'd' || c == 'i' || c == 'u' || c == 'x' || c == 'X' || c == 'o'){
            int base = c == 'x' || c == 'X' ? 16 : c == 'o' ? 8 : 10;
            if((*p == '-' || *p == '+') && k < lim) buf[k++] = *p++;
            while(*p && k < lim){
                char ch = *p; int d = ch >= '0' && ch <= '9' ? ch - '0' : (ch | 32) >= 'a' && (ch | 32) <= 'f' ? (ch | 32) - 'a' + 10 : 99;
                if(d >= base) break;
                buf[k++] = *p++;
            }
            buf[k] = 0;
            if(!k || (k == 1 && (buf[0] == '-' || buf[0] == '+'))) break;
            long long v = strtoll(buf, NULL, base);
            if(!skip){
                if(lng) *va_arg(ap, long*) = (long)v;
                else *va_arg(ap, int*) = (int)v;
                count++;
            }
        } else if(c == 'f' || c == 'g' || c == 'e'){
            while(*p && k < lim && ((*p >= '0' && *p <= '9') || *p == '.' || *p == '-' || *p == '+' || *p == 'e' || *p == 'E')) buf[k++] = *p++;
            buf[k] = 0;
            if(!k) break;
            double v = strtod(buf, NULL);
            if(!skip){ if(lng) *va_arg(ap, double*) = v; else *va_arg(ap, float*) = (float)v; count++; }
        } else if(c == 's'){
            char* out = skip ? NULL : va_arg(ap, char*);
            while(*p && !(*p == ' ' || (*p >= '\t' && *p <= '\r')) && k < lim){ if(out) out[k] = *p; k++; p++; }
            if(out){ out[k] = 0; count++; }
        } else if(c == 'c'){
            int n = width ? width : 1;
            char* out = skip ? NULL : va_arg(ap, char*);
            for(int i = 0; i < n && *p; i++){ if(out) out[i] = *p; p++; }
            if(out) count++;
        } else break;
    }
    return count;
}
int sscanf(const char* s, const char* fmt, ...){
    va_list ap; va_start(ap, fmt);
    int r = vsscanf(s, fmt, ap);
    va_end(ap); return r;
}
