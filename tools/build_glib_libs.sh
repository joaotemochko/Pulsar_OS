#!/bin/sh
# build_glib_libs.sh — a base do GTK para os programas Linux do Pulsar, dos
# fontes ORIGINAIS em user/third_party, com o build de cada projeto:
#
#   pcre2 10.44        (configure)  expressoes regulares (GRegex)
#   glib 2.82.4        (meson)      glib, gobject, gio, gmodule, gthread
#   fribidi 1.0.16     (meson)      algoritmo bidirecional (arabe, hebraico)
#   pango 1.54.0       (meson)      layout de texto: paragrafos, quebra de
#                                   linha, troca de fonte por caractere
#   gdk-pixbuf 2.42.12 (meson)      imagens (PNG, GIF, BMP, ICO...; loaders
#                                   embutidos, ja que tudo e estatico)
#
# Uso: build_glib_libs.sh <arch> <sysroot> <rtlib> <dir-de-build>
# As ferramentas glib-compile-resources/glib-mkenums do HOST sao usadas
# durante o build (gdk-pixbuf; mais tarde o GTK).
set -e
ARCH=$1; SYSROOT=$(cd "$2" && pwd); RTLIB=$(cd "$(dirname "$3")" && pwd)/$(basename "$3"); B=$4
TP=$(cd "$(dirname "$0")/../user/third_party" && pwd)
TOOLS=$(cd "$(dirname "$0")" && pwd)
mkdir -p "$B"; B=$(cd "$B" && pwd)
HOST=$ARCH-linux-musl
LLVM_AR=$(command -v llvm-ar)
sh "$TOOLS/meson-cross.sh" "$ARCH" "$SYSROOT" "$RTLIB" "$B"

mesonbuild() {  # <nome> <opcoes...>
    n=$1; shift
    rm -rf "${B:?}/${n:?}" && meson setup "$B/$n" "$TP/$n" --cross-file "$B/cross.ini" "$@" >"$B/$n.log" 2>&1 \
        || { tail -40 "$B/$n.log"; exit 1; }
    ninja -C "$B/$n" >>"$B/$n.log" 2>&1 && ninja -C "$B/$n" install >>"$B/$n.log" 2>&1 \
        || { grep -B2 -A14 "FAILED\|ERROR" "$B/$n.log" | head -60; exit 1; }
}

# a libffi (tools/build_linux_libs.sh) nao instala .pc; o glib a procura assim
[ -f "$SYSROOT/lib/pkgconfig/libffi.pc" ] || cat > "$SYSROOT/lib/pkgconfig/libffi.pc" <<EOF
prefix=$SYSROOT
libdir=\${prefix}/lib
includedir=\${prefix}/include
Name: libffi
Description: Library supporting Foreign Function Interfaces
Version: 3.4.6
Libs: -L\${libdir} -lffi
Cflags: -I\${includedir}
EOF

# ---------------------------------------------------------------- pcre2
if [ ! -f "$SYSROOT/lib/libpcre2-8.a" ]; then
    rm -rf "${B:?}/pcre2" && mkdir -p "$B/pcre2" && cd "$B/pcre2"
    CC="$B/musl-cc" CFLAGS="-O2 -fPIC" AR="$LLVM_AR" RANLIB="$LLVM_AR s" \
        "$TP/pcre2/configure" --host=$HOST --prefix="$SYSROOT" --enable-shared --enable-static \
        --enable-pcre2-8 --disable-pcre2-16 --disable-pcre2-32 --enable-unicode >/dev/null
    make -j8 libpcre2-8.la >/dev/null && make install-libLTLIBRARIES install-includeHEADERS \
        install-nodist_includeHEADERS install-pkgconfigDATA >/dev/null
fi

# ---------------------------------------------------------------- glib
[ -f "$SYSROOT/lib/libglib-2.0.a" ] || mesonbuild glib -Dtests=false -Dinstalled_tests=false \
    -Dnls=disabled -Dselinux=disabled -Dxattr=false -Dlibmount=disabled -Dman-pages=disabled \
    -Ddtrace=disabled -Dsystemtap=disabled -Dsysprof=disabled -Ddocumentation=false \
    -Dintrospection=disabled -Dlibelf=disabled -Dglib_debug=disabled

# ---------------------------------------------------------------- fribidi
[ -f "$SYSROOT/lib/libfribidi.a" ] || mesonbuild fribidi -Ddocs=false -Dbin=false -Dtests=false

# ---------------------------------------------------------------- pango
[ -f "$SYSROOT/lib/libpango-1.0.a" ] || mesonbuild pango -Dintrospection=disabled \
    -Dbuild-testsuite=false -Dbuild-examples=false -Dfontconfig=enabled -Dfreetype=enabled \
    -Dcairo=enabled -Dxft=disabled -Dlibthai=disabled -Dsysprof=disabled -Ddocumentation=false

# ---------------------------------------------------------------- gdk-pixbuf
[ -f "$SYSROOT/lib/libgdk_pixbuf-2.0.a" ] || mesonbuild gdk-pixbuf -Dpng=enabled -Djpeg=disabled \
    -Dtiff=disabled -Dgif=enabled -Dothers=enabled -Dbuiltin_loaders=all -Dgio_sniffing=false \
    -Dman=false -Ddocs=false -Dintrospection=disabled -Dtests=false -Dinstalled_tests=false \
    "-Dc_link_args=['-fuse-ld=lld','-Wl,--eh-frame-hdr','-L$SYSROOT/lib','-lpng16','-lz']"
# (o meson nao repassa a libpng dos loaders embutidos para os executaveis do
# proprio gdk-pixbuf; o lld acha os simbolos em qualquer ordem)

echo "[glib-libs] $ARCH: pcre2, glib, fribidi, pango, gdk-pixbuf em $SYSROOT/lib"
