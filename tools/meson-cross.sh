#!/bin/sh
# meson-cross.sh <arch> <sysroot> <rtlib> <dir>
#
# Ambiente de compilacao cruzada para os programas Linux do Pulsar
# (<arch>-linux-musl), usado pelos scripts build_*_libs.sh:
#
#   <dir>/cross.ini   arquivo de cross do meson (clang --sysroot, estatico)
#   <dir>/pkg-config  pkg-config que so ve o sysroot, com --static
#   <dir>/musl-cc     "compilador" para scripts configure (tools/musl-cc.sh)
#
# Tambem completa o sysroot com o que o driver do clang procura ao ligar um
# programa Linux: -lgcc (= as rotinas do compilador, compiler_builtins),
# -lgcc_eh (vazia: o unwinder esta na libc++) e crtbegin/crtend (vazios: a
# musl roda construtores por .init_array).
set -e
ARCH=$1; SYSROOT=$(cd "$2" && pwd); RTLIB=$(cd "$(dirname "$3")" && pwd)/$(basename "$3"); D=$4
TOOLS=$(cd "$(dirname "$0")" && pwd)
mkdir -p "$D"; D=$(cd "$D" && pwd)
HOST=$ARCH-linux-musl
LLVM_AR=$(command -v llvm-ar)

cp "$RTLIB" "$SYSROOT/lib/libgcc.a"
[ -f "$SYSROOT/lib/libgcc_eh.a" ] || "$LLVM_AR" rcs "$SYSROOT/lib/libgcc_eh.a"
for o in crtbegin crtbeginS crtbeginT crtend crtendS; do
    [ -f "$SYSROOT/lib/$o.o" ] || echo "" | clang --target=$HOST -x c -c - -o "$SYSROOT/lib/$o.o"
done

printf '#!/bin/sh\nexec "%s" %s "%s" "%s" "$@"\n' "$TOOLS/musl-cc.sh" "$ARCH" "$SYSROOT" "$RTLIB" > "$D/musl-cc"
printf '#!/bin/sh\nPKG_CONFIG_LIBDIR="%s/lib/pkgconfig" PKG_CONFIG_PATH= exec pkg-config --static "$@"\n' "$SYSROOT" > "$D/pkg-config"
chmod +x "$D/musl-cc" "$D/pkg-config"

cat > "$D/cross.ini" <<EOF
[binaries]
c = ['clang', '--target=$HOST', '--sysroot=$SYSROOT']
cpp = ['clang++', '--target=$HOST', '--sysroot=$SYSROOT', '-stdlib=libc++']
ar = '$LLVM_AR'
strip = 'llvm-strip'
pkg-config = '$D/pkg-config'

[built-in options]
c_args = ['-O2', '-fPIC']
cpp_args = ['-O2', '-fPIC']
c_link_args = ['-fuse-ld=lld', '-static', '-Wl,--eh-frame-hdr']
cpp_link_args = ['-fuse-ld=lld', '-static', '-Wl,--eh-frame-hdr']
default_library = 'static'
prefix = '$SYSROOT'
libdir = 'lib'
buildtype = 'release'
wrap_mode = 'nodownload'

[properties]
# respostas que o glib obteria rodando programas de teste (musl):
have_c99_vsnprintf = true
have_c99_snprintf = true
have_unix98_printf = true
va_val_copy = true
growing_stack = false
have_strlcpy = true
have_proc_self_cmdline = true

[host_machine]
system = 'linux'
cpu_family = '$ARCH'
cpu = '$ARCH'
endian = 'little'
EOF
