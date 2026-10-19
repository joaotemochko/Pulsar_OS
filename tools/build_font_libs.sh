#!/bin/sh
# build_font_libs.sh — fontes do sistema para a camada de compatibilidade
# Linux, a partir dos fontes ORIGINAIS em user/third_party, com os proprios
# scripts configure (cruzando para <arch>-linux-musl via tools/musl-cc.sh):
#
#   expat 2.6.4        (XML: a configuracao do fontconfig)
#   freetype 2.13.3    (rasterizacao TrueType/OpenType/CFF; zlib interna)
#   fontconfig 2.15.0  (descoberta e escolha de fontes; fc-list, fc-match,
#                       fc-cache... como programas Linux)
#
# Uso: build_font_libs.sh <arch> <sysroot> <rtlib> <dir-de-build> <dir-raiz>
# Bibliotecas e cabecalhos vao para o sysroot; os programas fc-* e a
# configuracao (/etc/fonts) vao para <dir-raiz> (copiado para o disco).
# Em tempo de execucao os caminhos sao os do Linux: /etc/fonts,
# /usr/share/fonts e /var/cache/fontconfig.
set -e
ARCH=$1; SYSROOT=$(cd "$2" && pwd); RTLIB=$(cd "$(dirname "$3")" && pwd)/$(basename "$3"); B=$4; ROOT=$5
TP=$(cd "$(dirname "$0")/../user/third_party" && pwd)
TOOLS=$(cd "$(dirname "$0")" && pwd)
mkdir -p "$B" "$ROOT"
B=$(cd "$B" && pwd); ROOT=$(cd "$ROOT" && pwd)
# musl-cc, pkg-config e o sysroot com libgcc/crtbegin (tools/meson-cross.sh).
# CC e um caminho so (o libtool copiaria para os .a qualquer .a citado no CC)
sh "$TOOLS/meson-cross.sh" "$ARCH" "$SYSROOT" "$RTLIB" "$B"
export CC="$B/musl-cc"
export CFLAGS="-O2 -w"
export AR=${AR:-llvm-ar} RANLIB="${AR:-llvm-ar} s"
# pkg-config so ve o sysroot (nunca as bibliotecas do host)
export PKG_CONFIG_LIBDIR="$SYSROOT/lib/pkgconfig" PKG_CONFIG_PATH=
HOST=$ARCH-linux-musl

# ---------------------------------------------------------------- expat
if [ ! -f "$SYSROOT/lib/libexpat.a" ]; then
    rm -rf "$B/expat" && mkdir -p "$B/expat" && cd "$B/expat"
    "$TP/expat/configure" --host=$HOST --prefix="$SYSROOT" --enable-shared --enable-static \
        --without-docbook --without-examples --without-tests --without-xmlwf >/dev/null
    make -j8 >/dev/null && make install >/dev/null
fi

# ---------------------------------------------------------------- freetype
if [ ! -f "$SYSROOT/lib/libfreetype.a" ]; then
    rm -rf "$B/freetype" && mkdir -p "$B/freetype" && cd "$B/freetype"
    "$TP/freetype/configure" --host=$HOST --prefix="$SYSROOT" --enable-shared --enable-static \
        --with-zlib=no --with-bzip2=no --with-png=no --with-harfbuzz=no --with-brotli=no >/dev/null
    make -j8 >/dev/null && make install >/dev/null
fi

# ---------------------------------------------------------------- fontconfig
if [ ! -f "$SYSROOT/lib/libfontconfig.a" ]; then
    rm -rf "$B/fontconfig" "$B/fc-dest" && mkdir -p "$B/fontconfig" && cd "$B/fontconfig"
    "$TP/fontconfig/configure" --host=$HOST --prefix=/usr --sysconfdir=/etc --localstatedir=/var \
        --enable-shared --enable-static --disable-docs --disable-nls --disable-cache-build \
        --with-default-fonts=/usr/share/fonts --with-cache-dir=/var/cache/fontconfig \
        FREETYPE_CFLAGS="-I$SYSROOT/include/freetype2" FREETYPE_LIBS="-L$SYSROOT/lib -lfreetype" \
        EXPAT_CFLAGS="-I$SYSROOT/include" EXPAT_LIBS="-L$SYSROOT/lib -lexpat" >/dev/null
    make -j8 >/dev/null
    make install DESTDIR="$B/fc-dest" >/dev/null
    cp -P "$B/fc-dest/usr/lib/libfontconfig.a" "$B/fc-dest/usr/lib/libfontconfig.so"* "$SYSROOT/lib/"
    rm -rf "$SYSROOT/include/fontconfig" && cp -r "$B/fc-dest/usr/include/fontconfig" "$SYSROOT/include/"
    sed "s|^prefix=.*|prefix=$SYSROOT|" "$B/fc-dest/usr/lib/pkgconfig/fontconfig.pc" > "$SYSROOT/lib/pkgconfig/fontconfig.pc"
fi
# programas e configuracao para o disco
rm -rf "$ROOT" && mkdir -p "$ROOT/bin" "$ROOT/etc" "$ROOT/var/cache/fontconfig"
for p in fc-cache fc-cat fc-conflist fc-list fc-match fc-pattern fc-query fc-scan fc-validate; do
    cp "$B/fc-dest/usr/bin/$p" "$ROOT/bin/"
done
cp -r "$B/fc-dest/etc/fonts" "$ROOT/etc/"
mkdir -p "$ROOT/usr/share" && cp -r "$B/fc-dest/usr/share/fontconfig" "$ROOT/usr/share/" 2>/dev/null || true
echo "[font-libs] $ARCH: expat, freetype, fontconfig em $SYSROOT/lib; fc-* em $ROOT/bin"
