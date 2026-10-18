#!/bin/sh
# musl-cc.sh <arch> <sysroot> <rtlib> [argumentos do cc...]
#
# "Compilador" C para programas Linux do Pulsar, no formato que scripts
# configure (autotools) esperam: compila com o clang contra os cabecalhos da
# musl do sysroot e, quando liga, faz um executavel ESTATICO com crt1/crti/
# crtn, a libc.a da musl e as rotinas do compilador (libcompiler_rt.a).
# Assim os configure das bibliotecas (expat, freetype, fontconfig...) rodam
# sem modificacao, cruzando para <arch>-linux-musl.
ARCH=$1; SYSROOT=$2; RTLIB=$3; shift 3
RES=$(clang -print-resource-dir)
CC="clang --target=$ARCH-linux-musl -nostdinc -isystem $SYSROOT/include -isystem $RES/include"
link=1
for a in "$@"; do
    case $a in -c|-S|-E|-M|-MM) link=0 ;; esac
done
if [ $link = 0 ]; then exec $CC "$@"; fi
exec $CC -fuse-ld=lld -static -nostdlib -L"$SYSROOT/lib" \
    "$SYSROOT/lib/crt1.o" "$SYSROOT/lib/crti.o" "$@" \
    -lc "$RTLIB" "$SYSROOT/lib/crtn.o"
