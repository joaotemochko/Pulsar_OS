#!/bin/sh
# build_gtk_libs.sh — GTK 3 (backend Wayland) para os programas Linux do
# Pulsar, dos fontes ORIGINAIS em user/third_party, todos com meson:
#
#   wayland 1.23.1          libwayland-client/-cursor/-egl com .pc (o build
#                           manual de tools/build_linux_libs.sh nao tem .pc)
#   wayland-protocols 1.38  os XML dos protocolos (xdg-shell...) com .pc
#   libxkbcommon 1.7.0      idem, com .pc
#   atk 2.38.0              acessibilidade (ultima versao separada; a do
#                           at-spi2-core exige libxml2 mesmo so com a ATK)
#   khronos (EGL-Registry)  cabecalhos EGL/KHR
#   libepoxy 1.5.10         despacho de OpenGL/EGL (sem libEGL no Pulsar, o
#                           GTK desenha tudo pelo cairo, em software)
#   cairo 1.18.2            refeito com glib: cairo-gobject (o GTK exige)
#   gtk 3.24.52             so o backend Wayland
#
# Uso: build_gtk_libs.sh <arch> <sysroot> <rtlib> <dir-de-build> <wayland-scanner-do-host>
set -e
ARCH=$1; SYSROOT=$(cd "$2" && pwd); RTLIB=$(cd "$(dirname "$3")" && pwd)/$(basename "$3"); B=$4
SCANNER=$(cd "$(dirname "$5")" && pwd)/$(basename "$5")
TP=$(cd "$(dirname "$0")/../user/third_party" && pwd)
TOOLS=$(cd "$(dirname "$0")" && pwd)
mkdir -p "$B"; B=$(cd "$B" && pwd)
sh "$TOOLS/meson-cross.sh" "$ARCH" "$SYSROOT" "$RTLIB" "$B"

# o wayland-scanner roda no HOST: o meson o acha por um .pc "nativo" e o
# GTK por find_program (secao [binaries] do arquivo de cross)
mkdir -p "$B/native-pc"
cat > "$B/native-pc/wayland-scanner.pc" <<EOF
wayland_scanner=$SCANNER
Name: Wayland Scanner
Description: gerador de codigo dos protocolos Wayland (host)
Version: 1.23.1
EOF
sed -i "s|^\\[binaries\\]|[binaries]\\nwayland-scanner = '$SCANNER'|" "$B/cross.ini"
NATIVE="-Dbuild.pkg_config_path=$B/native-pc"

mesonbuild() {  # <nome-do-dir-em-third_party> <opcoes...>
    n=$1; shift
    rm -rf "${B:?}/${n:?}" && meson setup "$B/$n" "$TP/$n" --cross-file "$B/cross.ini" "$@" >"$B/$n.log" 2>&1 \
        || { tail -40 "$B/$n.log"; exit 1; }
    ninja -C "$B/$n" >>"$B/$n.log" 2>&1 && ninja -C "$B/$n" install >>"$B/$n.log" 2>&1 \
        || { grep -B2 -A14 "FAILED\|ERROR" "$B/$n.log" | head -60; exit 1; }
}

[ -f "$SYSROOT/lib/pkgconfig/wayland-client.pc" ] || mesonbuild wayland $NATIVE -Dlibraries=true \
    -Dscanner=false -Dtests=false -Ddocumentation=false -Ddtd_validation=false
[ -f "$SYSROOT/share/pkgconfig/wayland-protocols.pc" ] || mesonbuild wayland-protocols $NATIVE -Dtests=false
[ -f "$SYSROOT/lib/pkgconfig/xkbcommon.pc" ] || mesonbuild libxkbcommon $NATIVE -Denable-x11=false \
    -Denable-wayland=false -Denable-docs=false -Denable-tools=false -Denable-xkbregistry=false \
    -Dxkb-config-root=/usr/share/X11/xkb -Dx-locale-root=/usr/share/X11/locale
[ -f "$SYSROOT/lib/libatk-1.0.a" ] || mesonbuild atk -Dintrospection=false -Ddocs=false
# cabecalhos EGL/KHR da Khronos (so interface): a libepoxy os inclui
mkdir -p "$SYSROOT/include/EGL" "$SYSROOT/include/KHR"
cp "$TP/khronos/EGL/"*.h "$SYSROOT/include/EGL/" && cp "$TP/khronos/KHR/"*.h "$SYSROOT/include/KHR/"
[ -f "$SYSROOT/lib/libepoxy.a" ] || mesonbuild libepoxy -Degl=yes -Dglx=no -Dx11=false -Dtests=false -Ddocs=false
[ -f "$SYSROOT/lib/libcairo-gobject.a" ] || mesonbuild cairo -Dfreetype=enabled -Dfontconfig=enabled \
    -Dpng=enabled -Dzlib=enabled -Dxlib=disabled -Dxcb=disabled -Dxlib-xcb=disabled -Dquartz=disabled \
    -Dtee=disabled -Dglib=enabled -Dspectre=disabled -Dsymbol-lookup=disabled -Dtests=disabled \
    -Dgtk_doc=false -Ddwrite=disabled
[ -f "$SYSROOT/lib/libgtk-3.a" ] || mesonbuild gtk3 $NATIVE -Dx11_backend=false -Dwayland_backend=true \
    -Dbroadway_backend=false -Dintrospection=false -Ddemos=false -Dexamples=false -Dtests=false \
    -Dinstalled_tests=false -Dprint_backends=file -Dcolord=no -Dcloudproviders=false -Dtracker3=false \
    -Dgtk_doc=false -Dman=false -Dbuiltin_immodules=all -Dprofiler=false

echo "[gtk-libs] $ARCH: wayland, xkbcommon, atk, epoxy, cairo-gobject, gtk3 em $SYSROOT/lib"
