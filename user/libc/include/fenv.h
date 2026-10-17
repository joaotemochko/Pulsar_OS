/* fenv.h — o Pulsar usa ponto flutuante por software: arredondamento fixo
 * ao mais proximo, sem excecoes. */
#ifndef _FENV_H
#define _FENV_H
#define FE_TONEAREST  0
#define FE_DOWNWARD   1
#define FE_UPWARD     2
#define FE_TOWARDZERO 3
static inline int fegetround(void){ return FE_TONEAREST; }
static inline int fesetround(int r){ return r == FE_TONEAREST ? 0 : -1; }
#endif
