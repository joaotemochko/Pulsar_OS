/* math.c — funcoes de math.h que o compiler_builtins do Rust nao exporta.
 * Trabalham direto nos bits do double (sem FPU: tudo e por software). */
#include <math.h>
#include <stdint.h>

static inline uint64_t bits(double x){ union { double d; uint64_t u; } v = { x }; return v.u; }
static inline double from_bits(uint64_t u){ union { uint64_t u; double d; } v = { u }; return v.d; }

double frexp(double x, int* e){
    uint64_t u = bits(x);
    int ee = (int)((u >> 52) & 0x7FF);
    if(ee == 0){
        if(x == 0){ *e = 0; return x; }
        x = frexp(x * 0x1p64, e); *e -= 64; return x;
    }
    if(ee == 0x7FF){ *e = 0; return x; }
    *e = ee - 1022;
    u &= 0x800FFFFFFFFFFFFFULL; u |= 0x3FE0000000000000ULL;
    return from_bits(u);
}
double scalbn(double x, int n){ return ldexp(x, n); }
double modf(double x, double* ip){
    double t = trunc(x);
    *ip = t;
    if(isinf(x)) return copysign(0.0, x);
    return copysign(x - t, x);
}
double nearbyint(double x){ return rint(x); }
long lrint(double x){ return (long)rint(x); }
long lround(double x){ return (long)round(x); }
long long llrint(double x){ return (long long)rint(x); }
long long llround(double x){ return (long long)round(x); }
double atanh(double x){ return 0.5 * log1p(2 * x / (1 - x)); }
