/* math.h — funcoes vem do compiler_builtins do Rust (libm portada, ponto
 * flutuante por software); as que faltam la estao em libc/src/math.c. */
#ifndef _MATH_H
#define _MATH_H
#define NAN       __builtin_nan("")
#define INFINITY  __builtin_inf()
#define HUGE_VAL  __builtin_huge_val()
#define HUGE_VALF __builtin_huge_valf()
#define M_E        2.7182818284590452354
#define M_LOG2E    1.4426950408889634074
#define M_LOG10E   0.43429448190325182765
#define M_LN2      0.69314718055994530942
#define M_LN10     2.30258509299404568402
#define M_PI       3.14159265358979323846
#define M_PI_2     1.57079632679489661923
#define M_PI_4     0.78539816339744830962
#define M_SQRT2    1.41421356237309504880
#define M_SQRT1_2  0.70710678118654752440
#define FP_NAN       0
#define FP_INFINITE  1
#define FP_ZERO      2
#define FP_SUBNORMAL 3
#define FP_NORMAL    4
#define isnan(x)    __builtin_isnan(x)
#define isinf(x)    __builtin_isinf(x)
#define isfinite(x) __builtin_isfinite(x)
#define isnormal(x) __builtin_isnormal(x)
#define signbit(x)  __builtin_signbit(x)
#define fpclassify(x) __builtin_fpclassify(FP_NAN, FP_INFINITE, FP_NORMAL, FP_SUBNORMAL, FP_ZERO, x)

double acos(double), asin(double), atan(double), atan2(double, double);
double cos(double), sin(double), tan(double);
double acosh(double), asinh(double), atanh(double);
double cosh(double), sinh(double), tanh(double);
double exp(double), exp2(double), expm1(double);
double log(double), log10(double), log2(double), log1p(double);
double pow(double, double), sqrt(double), cbrt(double), hypot(double, double);
double ceil(double), floor(double), trunc(double), round(double), rint(double), nearbyint(double);
double fabs(double), fmod(double, double), fmin(double, double), fmax(double, double);
double copysign(double, double), ldexp(double, int), frexp(double, int*), modf(double, double*);
double scalbn(double, int), fma(double, double, double);
long   lrint(double), lround(double);
long long llrint(double), llround(double);
float  sqrtf(float), floorf(float), ceilf(float), fabsf(float), roundf(float), truncf(float);
float  sinf(float), cosf(float), tanf(float), atan2f(float, float), powf(float, float);
float  expf(float), logf(float), fmodf(float, float), fminf(float, float), fmaxf(float, float);
float  acosf(float), asinf(float), atanf(float), hypotf(float, float), cbrtf(float);
#endif
