#!/bin/sh
# musl-cc.sh <arch> <sysroot> <rtlib> [argumentos do cc...]
#
# "Compilador" C para programas Linux do Pulsar, no formato que scripts
# configure (autotools) esperam: compila com o clang contra os cabecalhos da
# musl do sysroot e, quando liga:
#   - uma biblioteca compartilhada (-shared): pelo driver do clang com
#     --sysroot (libc.so da musl, compiler-rt, crtbeginS/crtendS);
#   - um executavel ligado a alguma .so: dinamico, pelo mesmo driver
#     (interpretador /lib/ld-musl-<arch>.so.1, bibliotecas em /usr/lib);
#   - os demais executaveis: ESTATICOS, com crt1/crti/crtn, a libc.a da musl
#     e as rotinas do compilador (<rtlib>).
# Assim os configure das bibliotecas (expat, freetype, fontconfig...) rodam
# sem modificacao, cruzando para <arch>-linux-musl.
[ -f "$HOME/pulsar-env.sh" ] && . "$HOME/pulsar-env.sh"
ARCH=$1; SYSROOT=$2; RTLIB=$3; shift 3
RES=$(clang -print-resource-dir)
CC="clang --target=$ARCH-linux-musl -nostdinc -isystem $SYSROOT/include -isystem $RES/include"
link=1; shared=0; dyn=0
for a in "$@"; do
    case $a in
        -c|-S|-E|-M|-MM) link=0 ;;
        -shared) shared=1 ;;
        *.so|*.so.*) dyn=1 ;;      # executavel ligado a uma biblioteca compartilhada
    esac
done
if [ $link = 0 ]; then exec $CC "$@"; fi
if [ $shared = 1 ] || [ $dyn = 1 ]; then  # .so, ou executavel dinamico (ld-musl em /lib)
    exec clang --target=$ARCH-linux-musl --sysroot="$SYSROOT" -fuse-ld=lld -Wl,--eh-frame-hdr "$@"
fi
exec $CC -fuse-ld=lld -static -nostdlib -Wl,--eh-frame-hdr -L"$SYSROOT/lib" \
    "$SYSROOT/lib/crt1.o" "$SYSROOT/lib/crti.o" "$@" \
    -lc "$RTLIB" "$SYSROOT/lib/crtn.o"
