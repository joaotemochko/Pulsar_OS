#!/bin/sh
# build_draw_libs.sh — texto e desenho 2D para os programas Linux do Pulsar,
# dos fontes ORIGINAIS em user/third_party, com o build de cada projeto:
#
#   zlib 1.3.1       (configure)       compressao (PNG, PDF, fontes WOFF)
#   libpng 1.6.44    (configure)       imagens PNG
#   pixman 0.44.2    (meson)           rasterizacao e composicao de pixels
#   harfbuzz 10.1.0  (meson, C++)      shaping: glifos e posicoes de um texto
#   cairo 1.18.2     (meson)           desenho 2D vetorial (o que GTK usa)
#
# Uso: build_draw_libs.sh <arch> <sysroot> <rtlib> <dir-de-build>
# Tudo estatico, instalado no sysroot (lib/, include/, lib/pkgconfig/).
set -e
ARCH=$1; SYSROOT=$(cd "$2" && pwd); RTLIB=$(cd "$(dirname "$3")" && pwd)/$(basename "$3"); B=$4
TP=$(cd "$(dirname "$0")/../user/third_party" && pwd)
TOOLS=$(cd "$(dirname "$0")" && pwd)
mkdir -p "$B"; B=$(cd "$B" && pwd)
HOST=$ARCH-linux-musl
LLVM_AR=$(command -v llvm-ar)

# cross.ini, pkg-config e musl-cc (e libgcc/crtbegin no sysroot)
sh "$TOOLS/meson-cross.sh" "$ARCH" "$SYSROOT" "$RTLIB" "$B"

mesonbuild() {  # <nome> <opcoes...>
    n=$1; shift
    rm -rf "${B:?}/${n:?}" && meson setup "$B/$n" "$TP/$n" --cross-file "$B/cross.ini" "$@" >"$B/$n.log" 2>&1 \
        || { tail -30 "$B/$n.log"; exit 1; }
    ninja -C "$B/$n" >>"$B/$n.log" 2>&1 && ninja -C "$B/$n" install >>"$B/$n.log" 2>&1 \
        || { grep -B2 -A12 "FAILED" "$B/$n.log" | head -60; exit 1; }
}

# ---------------------------------------------------------------- zlib
if [ ! -f "$SYSROOT/lib/libz.a" ]; then
    rm -rf "${B:?}/zlib" && cp -r "$TP/zlib" "$B/zlib" && cd "$B/zlib"   # configure de zlib so roda na arvore
    CC="$B/musl-cc" CFLAGS="-O2 -fPIC" AR="$LLVM_AR" CHOST=$HOST LDSHARED="$B/musl-cc -shared -Wl,-soname,libz.so.1" \
        ./configure --prefix="$SYSROOT" >/dev/null
    make -j8 >/dev/null && make install >/dev/null
fi

# ---------------------------------------------------------------- libpng
if [ ! -f "$SYSROOT/lib/libpng16.a" ]; then
    rm -rf "${B:?}/libpng" && mkdir -p "$B/libpng" && cd "$B/libpng"
    CC="$B/musl-cc" CFLAGS="-O2 -fPIC" AR="$LLVM_AR" RANLIB="$LLVM_AR s" PKG_CONFIG="$B/pkg-config" \
    CPPFLAGS="-I$SYSROOT/include" LDFLAGS="-L$SYSROOT/lib" \
        "$TP/libpng/configure" --host=$HOST --prefix="$SYSROOT" --enable-shared --enable-static \
        --disable-tests --disable-tools >/dev/null
    make -j8 >/dev/null && make install >/dev/null
fi

# ---------------------------------------------------------------- pixman
[ -f "$SYSROOT/lib/libpixman-1.a" ] || mesonbuild pixman -Dtests=disabled -Ddemos=disabled \
    -Dgtk=disabled -Dlibpng=disabled -Dopenmp=disabled

# ---------------------------------------------------------------- harfbuzz
[ -f "$SYSROOT/lib/libharfbuzz.a" ] || mesonbuild harfbuzz -Dfreetype=enabled -Dglib=disabled \
    -Dgobject=disabled -Dcairo=disabled -Dicu=disabled -Dgraphite2=disabled -Dchafa=disabled \
    -Dtests=disabled -Dintrospection=disabled -Ddocs=disabled -Dutilities=disabled -Dbenchmark=disabled

# ---------------------------------------------------------------- cairo
[ -f "$SYSROOT/lib/libcairo.a" ] || mesonbuild cairo -Dfreetype=enabled -Dfontconfig=enabled \
    -Dpng=enabled -Dzlib=enabled -Dxlib=disabled -Dxcb=disabled -Dquartz=disabled -Dtee=disabled \
    -Dglib=disabled -Dspectre=disabled -Dsymbol-lookup=disabled -Dtests=disabled -Dgtk_doc=false -Dxlib-xcb=disabled \
    -Ddwrite=disabled

echo "[draw-libs] $ARCH: zlib, libpng, pixman, harfbuzz, cairo em $SYSROOT/lib"
