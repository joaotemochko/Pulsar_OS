#ifndef _SETJMP_H
#define _SETJMP_H
typedef long jmp_buf[32];
#define setjmp(b) __builtin_setjmp(b)
#define longjmp(b, v) __builtin_longjmp(b, 1)
#endif
