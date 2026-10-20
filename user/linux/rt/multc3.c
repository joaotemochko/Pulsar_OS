/* __multc3 — multiplicacao de complexos long double (128 bits), exigida
 * pelo libc.so da musl e ausente no compiler_builtins do Rust. Segue o
 * anexo G do C99 (como o compiler-rt): recupera infinitos quando o
 * resultado ingenuo da NaN. */
typedef long double ld;
typedef _Complex long double cld;

static int is_nan(ld x) { return x != x; }
static int is_inf(ld x) { return !is_nan(x) && is_nan(x - x); }
static ld copysign_ld(ld m, ld s) { return __builtin_copysignl(m, s); }

cld __multc3(ld a, ld b, ld c, ld d) {
    ld ac = a * c, bd = b * d, ad = a * d, bc = b * c;
    ld re = ac - bd, im = ad + bc;
    if (is_nan(re) && is_nan(im)) {
        int recalc = 0;
        if (is_inf(a) || is_inf(b)) {
            a = copysign_ld(is_inf(a) ? 1 : 0, a);
            b = copysign_ld(is_inf(b) ? 1 : 0, b);
            if (is_nan(c)) c = copysign_ld(0, c);
            if (is_nan(d)) d = copysign_ld(0, d);
            recalc = 1;
        }
        if (is_inf(c) || is_inf(d)) {
            c = copysign_ld(is_inf(c) ? 1 : 0, c);
            d = copysign_ld(is_inf(d) ? 1 : 0, d);
            if (is_nan(a)) a = copysign_ld(0, a);
            if (is_nan(b)) b = copysign_ld(0, b);
            recalc = 1;
        }
        if (!recalc && (is_inf(ac) || is_inf(bd) || is_inf(ad) || is_inf(bc))) {
            if (is_nan(a)) a = copysign_ld(0, a);
            if (is_nan(b)) b = copysign_ld(0, b);
            if (is_nan(c)) c = copysign_ld(0, c);
            if (is_nan(d)) d = copysign_ld(0, d);
            recalc = 1;
        }
        if (recalc) {
            ld inf = __builtin_infl();
            re = inf * (a * c - b * d);
            im = inf * (a * d + b * c);
        }
    }
    cld z;
    __real__ z = re;
    __imag__ z = im;
    return z;
}

cld __mulxc3(ld a, ld b, ld c, ld d) {
    return __multc3(a, b, c, d);
}
