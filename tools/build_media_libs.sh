#!/bin/sh
# build_media_libs.sh — bibliotecas de midia dos programas Linux do Pulsar,
# dos fontes ORIGINAIS em user/third_party, com o configure de cada projeto:
#
#   alsa-lib 1.2.16.1   audio/MIDI do Linux. O Firefox a exige (Web MIDI e
#                       o backend ALSA do cubeb); sem /dev/snd ela so
#                       responde que nao ha placa de som.
#
# Uso: build_media_libs.sh <arch> <sysroot> <rtlib> <dir-de-build> <dir-raiz>
# Biblioteca e cabecalhos no sysroot; a configuracao (/usr/share/alsa) em
# <dir-raiz>, copiado para o disco.
set -e
ARCH=$1; SYSROOT=$(cd "$2" && pwd); RTLIB=$(cd "$(dirname "$3")" && pwd)/$(basename "$3"); B=$4; ROOT=$5
TP=$(cd "$(dirname "$0")/../user/third_party" && pwd)
TOOLS=$(cd "$(dirname "$0")" && pwd)
mkdir -p "$B" "$ROOT"; B=$(cd "$B" && pwd); ROOT=$(cd "$ROOT" && pwd)
HOST=$ARCH-linux-musl
sh "$TOOLS/meson-cross.sh" "$ARCH" "$SYSROOT" "$RTLIB" "$B"
LLVM_AR=$(command -v llvm-ar)

# ---------------------------------------------------------------- alsa-lib
if [ ! -f "$SYSROOT/lib/libasound.so" ]; then
    rm -rf "${B:?}/alsa" "${B:?}/alsa-dest" && mkdir -p "$B/alsa" && cd "$B/alsa"
    CC="$B/musl-cc" CFLAGS="-O2 -fPIC" AR="$LLVM_AR" RANLIB="$LLVM_AR s" \
        "$TP/alsa-lib/configure" --host=$HOST --prefix=/usr --sysconfdir=/etc \
        --enable-shared --disable-static --disable-python --without-debug \
        --with-configdir=/usr/share/alsa >/dev/null
    make -j8 >/dev/null
    make install DESTDIR="$B/alsa-dest" >/dev/null
    cp -P "$B/alsa-dest/usr/lib/libasound.so"* "$SYSROOT/lib/"
    rm -rf "${SYSROOT:?}/include/alsa" && cp -r "$B/alsa-dest/usr/include/alsa" "$SYSROOT/include/"
    sed "s|^prefix=.*|prefix=$SYSROOT|" "$B/alsa-dest/usr/lib/pkgconfig/alsa.pc" > "$SYSROOT/lib/pkgconfig/alsa.pc"
fi
rm -rf "${ROOT:?}/usr/share/alsa" && mkdir -p "$ROOT/usr/share"
cp -r "$B/alsa-dest/usr/share/alsa" "$ROOT/usr/share/"
echo "[media-libs] $ARCH: alsa-lib em $SYSROOT/lib; configuracao em $ROOT/usr/share/alsa"
