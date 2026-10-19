#!/bin/sh
# build_firefox.sh <arch> [mach-comando...]
#
# Compila o Firefox ESR para o Pulsar OS (camada de compatibilidade Linux):
# baixa o fonte oficial (verificado por SHA-256) em ~/firefox-src, aplica os
# patches de user/firefox/patches e roda o mach com user/firefox/mozconfig,
# cruzando para <arch>-unknown-linux-musl contra o sysroot do Pulsar
# (build/<arch>/sysroot: musl, libc++, GTK 3...). Sem comando: configure+build.
#
# Ferramentas do host: LLVM >= 19 (clang, lld, libclang para o bindgen; em
# ~/llvm22 ou $FIREFOX_LLVM), Rust estavel com o alvo <arch>-unknown-linux-musl,
# cbindgen e Node.js (em ~/node-local).
set -e
ARCH=$1; shift || true
VER=153.5.0esr
SHA=28714a7d4f9b38c7aafbe38022de7775a3d9cb3c1ebf19a567c26eb761e3410b
REPO=$(cd "$(dirname "$0")/.." && pwd)
FF=${FIREFOX_SRC:-$HOME/firefox-src}
SRC=$FF/firefox-${VER%esr}
SYSROOT=$REPO/build/$ARCH/sysroot
[ -f "$SYSROOT/lib/libgtk-3.so" ] || { echo "falta o sysroot com GTK: make ARCH=$ARCH musl"; exit 1; }

# ---------------------------------------------------------------- fonte
mkdir -p "$FF"
if [ ! -d "$SRC" ]; then
    T=$FF/firefox-$VER.source.tar.xz
    [ -f "$T" ] || curl -sfo "$T" https://archive.mozilla.org/pub/firefox/releases/$VER/source/firefox-$VER.source.tar.xz
    echo "$SHA  $T" | sha256sum -c --quiet
    tar xf "$T" -C "$FF"
fi
# patches do Pulsar (aplicados uma vez; a marca fica no proprio fonte)
for p in "$REPO"/user/firefox/patches/*.patch; do
    [ -f "$p" ] || continue
    m=$SRC/.pulsar-$(basename "$p")
    [ -f "$m" ] && continue
    patch -d "$SRC" -p1 -s < "$p" && touch "$m"
done

# ---------------------------------------------------------------- ambiente
OBJ=$FF/obj-$ARCH
mkdir -p "$OBJ"
# pkg-config do sysroot, para ligacao DINAMICA (sem --static)
printf '#!/bin/sh\nPKG_CONFIG_LIBDIR="%s/lib/pkgconfig:%s/share/pkgconfig" PKG_CONFIG_PATH= exec pkg-config "$@"\n' \
    "$SYSROOT" "$SYSROOT" > "$FF/pkg-config-$ARCH"
chmod +x "$FF/pkg-config-$ARCH"
export PULSAR_ARCH=$ARCH PULSAR_SYSROOT=$SYSROOT PULSAR_OBJDIR=$OBJ PULSAR_PKGCONFIG=$FF/pkg-config-$ARCH
# o Firefox 153 exige clang >= 19: um LLVM proprio (so para o Firefox; o resto
# do Pulsar segue com o clang do sistema de build)
LLVM=${FIREFOX_LLVM:-$HOME/llvm22}
[ -x "$LLVM/bin/clang" ] || { echo "falta o LLVM >= 19 em $LLVM (ex.: LLVM-22.1.8-Linux-X64 do GitHub)"; exit 1; }
export PULSAR_LIBCLANG=$LLVM/lib
export MOZCONFIG=$REPO/user/firefox/mozconfig
export RUSTUP_TOOLCHAIN=${RUSTUP_TOOLCHAIN:-stable}
export PATH=$LLVM/bin:$HOME/node-local/bin:$HOME/.cargo/bin:$PATH
export MOZBUILD_STATE_PATH=${MOZBUILD_STATE_PATH:-$FF/mozbuild}
export MACH_BUILD_PYTHON_NATIVE_PACKAGE_SOURCE=none
cd "$SRC"
if [ $# = 0 ]; then
    ./mach configure
    ./mach build
else
    ./mach "$@"
fi
