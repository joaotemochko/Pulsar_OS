#!/bin/sh
# build_linux_libs.sh — compila, sem meson, as bibliotecas graficas da
# camada de compatibilidade Linux, a partir dos fontes ORIGINAIS em
# user/third_party (nada e modificado):
#
#   libffi 3.4.6           (exigida pela libwayland)
#   libwayland-client 1.23 (+ wayland-scanner para o host)
#   xdg-shell              (gerado de wayland-protocols 1.38)
#   libxkbcommon 1.7.0     (+ compose; parser gerado com bison)
#
# Uso: build_linux_libs.sh <arch> <sysroot> <dir-de-build> <cc-alvo...>
# Instala em <sysroot>/lib e <sysroot>/include.
set -e
ARCH=$1; SYSROOT=$2; B=$3; shift 3
CC="$*"
TP=$(cd "$(dirname "$0")/../user/third_party" && pwd)
AR=${AR:-llvm-ar}
mkdir -p "$B" "$SYSROOT/lib" "$SYSROOT/include"
SYSROOT=$(cd "$SYSROOT" && pwd); B=$(cd "$B" && pwd)

# ---------------------------------------------------------------- wayland-scanner (host)
HOST=$B/host
mkdir -p "$HOST"
if [ ! -x "$HOST/wayland-scanner" ]; then
    printf '#define PACKAGE "wayland"\n#define PACKAGE_VERSION "1.23.1"\n' > "$HOST/config.h"
    python3 "$TP/wayland/src/embed.py" "$TP/wayland/protocol/wayland.dtd" wayland_dtd > "$HOST/wayland.dtd.h"
    cp "$TP/wayland/src/wayland-version.h.in" "$HOST/wayland-version.h.in"
    sed -e 's/@WAYLAND_VERSION@/1.23.1/' -e 's/@WAYLAND_VERSION_MAJOR@/1/' -e 's/@WAYLAND_VERSION_MINOR@/23/' \
        -e 's/@WAYLAND_VERSION_MICRO@/1/' "$TP/wayland/src/wayland-version.h.in" > "$HOST/wayland-version.h"
    cc -O2 -include "$HOST/config.h" -I"$HOST" -I"$TP/wayland/src" -I"$TP/wayland/protocol" \
        "$TP/wayland/src/scanner.c" "$TP/wayland/src/wayland-util.c" -lexpat -o "$HOST/wayland-scanner"
fi
SCANNER=$HOST/wayland-scanner

# ---------------------------------------------------------------- libffi
case $ARCH in
    aarch64) FT=AARCH64; FD=aarch64 ;;
    riscv64) FT=RISCV;   FD=riscv ;;
    *) echo "arquitetura desconhecida: $ARCH"; exit 1 ;;
esac
F=$B/ffi; mkdir -p "$F/obj"
sed -e 's/@VERSION@/3.4.6/' -e "s/@TARGET@/$FT/" -e 's/@HAVE_LONG_DOUBLE@/1/' \
    -e 's/@HAVE_LONG_DOUBLE_VARIANT@/0/' -e 's/@FFI_EXEC_TRAMPOLINE_TABLE@/0/' \
    "$TP/libffi/include/ffi.h.in" > "$F/ffi.h"
cp "$TP/libffi/src/$FD/ffitarget.h" "$F/ffitarget.h"
cat > "$F/fficonfig.h" <<'EOF'
/* fficonfig.h para <arch>-linux-musl no Pulsar (o que o configure da libffi
 * detectaria; trampolins estaticos desligados: closures usam mmap). */
#define HAVE_ALLOCA_H 1
#define HAVE_AS_CFI_PSEUDO_OP 1
#define HAVE_HIDDEN_VISIBILITY_ATTRIBUTE 1
#define HAVE_INTTYPES_H 1
#define HAVE_LONG_DOUBLE 1
#define HAVE_MEMCPY 1
#define HAVE_MKOSTEMP 1
#define HAVE_MMAP 1
#define HAVE_MMAP_ANON 1
#define HAVE_STDINT_H 1
#define HAVE_STDLIB_H 1
#define HAVE_STRING_H 1
#define HAVE_SYS_MMAN_H 1
#define HAVE_UNISTD_H 1
#define SIZEOF_DOUBLE 8
#define SIZEOF_LONG_DOUBLE 16
#define SIZEOF_SIZE_T 8
#define STDC_HEADERS 1
#define EH_FRAME_FLAGS "a"
#define PACKAGE "libffi"
#define VERSION "3.4.6"
#ifdef LIBFFI_ASM
#define FFI_HIDDEN(name) .hidden name
#else
#define FFI_HIDDEN __attribute__ ((visibility ("hidden")))
#endif
EOF
FCC="$CC -I$F -I$TP/libffi/include -I$TP/libffi/src/$FD -DFFI_STATIC_BUILD -w"
for s in prep_cif types raw_api java_raw_api closures tramp; do
    $FCC -c "$TP/libffi/src/$s.c" -o "$F/obj/$s.o"
done
$FCC -c "$TP/libffi/src/$FD/ffi.c" -o "$F/obj/arch_ffi.o"
$FCC -c "$TP/libffi/src/$FD/sysv.S" -o "$F/obj/arch_sysv.o"
rm -f "$SYSROOT/lib/libffi.a"; $AR rcs "$SYSROOT/lib/libffi.a" "$F"/obj/*.o
cp "$F/ffi.h" "$F/ffitarget.h" "$SYSROOT/include/"

# ---------------------------------------------------------------- libwayland-client
W=$B/wayland; mkdir -p "$W/obj"
cat > "$W/config.h" <<'EOF'
#define PACKAGE "wayland"
#define PACKAGE_VERSION "1.23.1"
#define HAVE_SYS_PRCTL_H 1
#define HAVE_ACCEPT4 1
#define HAVE_MKOSTEMP 1
#define HAVE_POSIX_FALLOCATE 1
#define HAVE_PRCTL 1
#define HAVE_MEMFD_CREATE 1
#define HAVE_MREMAP 1
#define HAVE_STRNDUP 1
#define HAVE_XUCRED_CR_PID 0
#define HAVE_BROKEN_MSG_CMSG_CLOEXEC 0
EOF
cp "$HOST/wayland-version.h" "$W/"
$SCANNER client-header "$TP/wayland/protocol/wayland.xml" "$W/wayland-client-protocol.h"
$SCANNER public-code "$TP/wayland/protocol/wayland.xml" "$W/wayland-protocol.c"
# os fontes fazem #include "../config.h": um subdiretorio cujo ".." e $W
mkdir -p "$W/sub"
WCC="$CC -D_POSIX_C_SOURCE=200809L -I$W/sub -I$W -I$TP/wayland/src -I$SYSROOT/include -fvisibility=hidden -w"
for s in wayland-client connection wayland-os wayland-util; do
    $WCC -c "$TP/wayland/src/$s.c" -o "$W/obj/$s.o"
done
$WCC -c "$W/wayland-protocol.c" -o "$W/obj/wayland-protocol.o"
rm -f "$SYSROOT/lib/libwayland-client.a"; $AR rcs "$SYSROOT/lib/libwayland-client.a" "$W"/obj/*.o
cp "$TP/wayland/src/wayland-client.h" "$TP/wayland/src/wayland-client-core.h" "$TP/wayland/src/wayland-util.h" \
   "$W/wayland-version.h" "$W/wayland-client-protocol.h" "$SYSROOT/include/"

# ---------------------------------------------------------------- xdg-shell (wayland-protocols)
X=$B/xdg; mkdir -p "$X"
XML=$TP/wayland-protocols/stable/xdg-shell/xdg-shell.xml
$SCANNER client-header "$XML" "$X/xdg-shell-client-protocol.h"
$SCANNER private-code "$XML" "$X/xdg-shell-protocol.c"
$CC -I"$SYSROOT/include" -w -c "$X/xdg-shell-protocol.c" -o "$X/xdg-shell-protocol.o"
rm -f "$SYSROOT/lib/libxdg-shell.a"; $AR rcs "$SYSROOT/lib/libxdg-shell.a" "$X/xdg-shell-protocol.o"
cp "$X/xdg-shell-client-protocol.h" "$SYSROOT/include/"

# ---------------------------------------------------------------- libxkbcommon
K=$B/xkbcommon; mkdir -p "$K/obj" "$K/xkbcomp"
cat > "$K/config.h" <<'EOF'
#define EXIT_INVALID_USAGE 2
#define LIBXKBCOMMON_VERSION "1.7.0"
#define LIBXKBCOMMON_TOOL_PATH "/usr/libexec/xkbcommon"
#define _GNU_SOURCE 1
#define DFLT_XKB_CONFIG_ROOT "/usr/share/X11/xkb"
#define DFLT_XKB_CONFIG_EXTRA_PATH "/etc/xkb"
#define XLOCALEDIR "/usr/share/X11/locale"
#define DEFAULT_XKB_RULES "evdev"
#define DEFAULT_XKB_MODEL "pc105"
#define DEFAULT_XKB_LAYOUT "us"
#define DEFAULT_XKB_VARIANT NULL
#define DEFAULT_XKB_OPTIONS NULL
#define HAVE_UNISTD_H 1
#define HAVE___BUILTIN_EXPECT 1
#define HAVE_EACCESS 1
#define HAVE_EUIDACCESS 1
#define HAVE_MMAP 1
#define HAVE_MKOSTEMP 1
#define HAVE_POSIX_FALLOCATE 1
#define HAVE_STRNDUP 1
#define HAVE_ASPRINTF 1
#define HAVE_VASPRINTF 1
#define HAVE_SECURE_GETENV 1
EOF
bison --defines="$K/xkbcomp/parser.h" -o "$K/xkbcomp/parser.c" -p _xkbcommon_ "$TP/libxkbcommon/src/xkbcomp/parser.y"
SRCS=$(python3 - "$TP/libxkbcommon/meson.build" <<'PY'
import re, sys
t = open(sys.argv[1]).read()
blk = t[t.index('libxkbcommon_sources = ['):]
blk = blk[:blk.index(']')]
print(' '.join(s for s in re.findall(r"'(src/[^']+\.c)'", blk)))
PY
)
KCC="$CC -include $K/config.h -I$K -I$K/xkbcomp -I$TP/libxkbcommon -I$TP/libxkbcommon/src -I$TP/libxkbcommon/include -fvisibility=hidden -w"
for s in $SRCS; do
    o=$(echo "$s" | tr '/' '_' | sed 's/\.c$/.o/')
    $KCC -c "$TP/libxkbcommon/$s" -o "$K/obj/$o"
done
$KCC -I"$TP/libxkbcommon/src/xkbcomp" -c "$K/xkbcomp/parser.c" -o "$K/obj/xkbcomp_parser_gen.o"
rm -f "$SYSROOT/lib/libxkbcommon.a"; $AR rcs "$SYSROOT/lib/libxkbcommon.a" "$K"/obj/*.o
mkdir -p "$SYSROOT/include/xkbcommon"
cp "$TP/libxkbcommon/include/xkbcommon/"*.h "$SYSROOT/include/xkbcommon/"
echo "[linux-libs] $ARCH: libffi, libwayland-client, xdg-shell, libxkbcommon em $SYSROOT/lib"
