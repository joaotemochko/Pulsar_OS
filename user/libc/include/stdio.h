/* stdio.h — libc do Pulsar: saida formatada (stb_sprintf) para a serial.
 * Nao ha arquivos: stdout/stderr escrevem na serial, stdin nao existe. */
#ifndef _STDIO_H
#define _STDIO_H
#include <stddef.h>
#include <stdarg.h>

typedef struct _PFILE FILE;
extern FILE* stdin;
extern FILE* stdout;
extern FILE* stderr;
#define EOF (-1)
#define BUFSIZ 1024

int printf(const char* fmt, ...) __attribute__((format(printf, 1, 2)));
int fprintf(FILE* f, const char* fmt, ...) __attribute__((format(printf, 2, 3)));
int sprintf(char* out, const char* fmt, ...) __attribute__((format(printf, 2, 3)));
int snprintf(char* out, size_t n, const char* fmt, ...) __attribute__((format(printf, 3, 4)));
int vprintf(const char* fmt, va_list ap);
int vfprintf(FILE* f, const char* fmt, va_list ap);
int vsprintf(char* out, const char* fmt, va_list ap);
int vsnprintf(char* out, size_t n, const char* fmt, va_list ap);
int puts(const char* s);
int putchar(int c);
int fputs(const char* s, FILE* f);
int fputc(int c, FILE* f);
int putc(int c, FILE* f);
size_t fwrite(const void* p, size_t sz, size_t n, FILE* f);
int fflush(FILE* f);
/* nao ha sistema de arquivos no stdio: fopen sempre falha */
#define SEEK_SET 0
#define SEEK_CUR 1
#define SEEK_END 2
FILE* fopen(const char* path, const char* mode);
int fclose(FILE* f);
size_t fread(void* p, size_t sz, size_t n, FILE* f);
int fseek(FILE* f, long off, int whence);
long ftell(FILE* f);
int sscanf(const char* s, const char* fmt, ...);
int vsscanf(const char* s, const char* fmt, va_list ap);
#endif
