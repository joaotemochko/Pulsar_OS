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
# programa Linux: -lgcc (= as rotinas do compilador, do compiler-rt),
# -lgcc_eh (vazia: o unwinder esta na libc++) e crtbegin/crtend (os do
# compiler-rt; a musl roda os construtores por .init_array).
set -e
ARCH=$1; SYSROOT=$(cd "$2" && pwd); RTLIB=$(cd "$(dirname "$3")" && pwd)/$(basename "$3"); D=$4
TOOLS=$(cd "$(dirname "$0")" && pwd)
mkdir -p "$D"; D=$(cd "$D" && pwd)
HOST=$ARCH-linux-musl
LLVM_AR=$(command -v llvm-ar)

# rotinas do compilador: as do compiler-rt (PIC; tools/build_cxx_runtime.sh)
# quando existem, senao as do compiler_builtins do Rust (so para estaticos)
CRT=$SYSROOT/lib/linux
if [ -f "$CRT/libclang_rt.builtins-$ARCH.a" ]; then
    cp "$CRT/libclang_rt.builtins-$ARCH.a" "$SYSROOT/lib/libgcc.a"
    cp "$CRT/clang_rt.crtbegin-$ARCH.o" "$SYSROOT/lib/crtbegin.o"
    cp "$CRT/clang_rt.crtbegin-$ARCH.o" "$SYSROOT/lib/crtbeginS.o"
    cp "$CRT/clang_rt.crtbegin-$ARCH.o" "$SYSROOT/lib/crtbeginT.o"
    cp "$CRT/clang_rt.crtend-$ARCH.o" "$SYSROOT/lib/crtend.o"
    cp "$CRT/clang_rt.crtend-$ARCH.o" "$SYSROOT/lib/crtendS.o"
else
    cp "$RTLIB" "$SYSROOT/lib/libgcc.a"
    for o in crtbegin crtbeginS crtbeginT crtend crtendS; do
        [ -f "$SYSROOT/lib/$o.o" ] || echo "" | clang --target=$HOST -x c -c - -o "$SYSROOT/lib/$o.o"
    done
fi
for l in gcc_eh gcc_s atomic; do [ -f "$SYSROOT/lib/lib$l.a" ] || "$LLVM_AR" rcs "$SYSROOT/lib/lib$l.a"; done

printf '#!/bin/sh\nexec "%s" %s "%s" "%s" "$@"\n' "$TOOLS/musl-cc.sh" "$ARCH" "$SYSROOT" "$RTLIB" > "$D/musl-cc"
printf '#!/bin/sh\nPKG_CONFIG_LIBDIR="%s/lib/pkgconfig:%s/share/pkgconfig" PKG_CONFIG_PATH= exec pkg-config --static "$@"\n' "$SYSROOT" "$SYSROOT" > "$D/pkg-config"
chmod +x "$D/musl-cc" "$D/pkg-config"

cat > "$D/cross.ini" <<EOF
[binaries]
c = ['clang', '--target=$HOST', '--sysroot=$SYSROOT']
# libc++ do sysroot: cabecalhos por -isystem (a do clang e a do host) e
# -stdlib=libc++ so na ligacao (na compilacao seria argumento sem uso)
cpp = ['clang++', '--target=$HOST', '--sysroot=$SYSROOT', '-nostdinc++', '-isystem', '$SYSROOT/include/c++/v1']
ar = '$LLVM_AR'
strip = 'llvm-strip'
pkg-config = '$D/pkg-config'

[built-in options]
c_args = ['-O2', '-fPIC']
cpp_args = ['-O2', '-fPIC']
c_link_args = ['-fuse-ld=lld', '-Wl,--eh-frame-hdr']
cpp_link_args = ['-fuse-ld=lld', '-Wl,--eh-frame-hdr', '-stdlib=libc++']
# estaticas (programas de teste ligados pelo Makefile) e compartilhadas
# (/usr/lib: ligacao dinamica, dlopen e o que o Firefox espera)
default_library = 'both'
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
