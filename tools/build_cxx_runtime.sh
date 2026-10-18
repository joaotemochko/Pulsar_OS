#!/bin/sh
# build_cxx_runtime.sh — biblioteca C++ dos programas Linux do Pulsar: libc++,
# libc++abi e libunwind do LLVM 18.1.3 (mesma versao do clang do build), dos
# fontes originais em user/third_party/llvm-runtimes, compiladas com o CMake
# do proprio LLVM para <arch>-linux-musl. Saem estaticas, com a libc++abi e a
# libunwind dentro da libc++.a (excecoes, RTTI, threads e locale ligados).
#
# Uso: build_cxx_runtime.sh <arch> <sysroot> <dir-de-build>
# Instala em <sysroot>/include/c++/v1 e <sysroot>/lib/libc++.a.
set -e
ARCH=$1; SYSROOT=$(cd "$2" && pwd); B=$3
SRC=$(cd "$(dirname "$0")/../user/third_party/llvm-runtimes" && pwd)
mkdir -p "$B"; B=$(cd "$B" && pwd)
# o clang procura os cabecalhos e as bibliotecas do sistema em <sysroot>/usr:
# com --sysroot a musl vira o diretorio de SISTEMA, depois dos cabecalhos da
# libc++ (que usam #include_next <stdlib.h> etc.)
mkdir -p "$SYSROOT/usr"
[ -e "$SYSROOT/usr/include" ] || ln -s ../include "$SYSROOT/usr/include"
[ -e "$SYSROOT/usr/lib" ] || ln -s ../lib "$SYSROOT/usr/lib"
T=$ARCH-linux-musl
FLAGS="--target=$T --sysroot=$SYSROOT -O2 -fPIC"
rm -rf "$B/cxx" && mkdir -p "$B/cxx"
cmake -G Ninja -S "$SRC/runtimes" -B "$B/cxx" \
    -DLLVM_ENABLE_RUNTIMES="libcxx;libcxxabi;libunwind" \
    -DCMAKE_BUILD_TYPE=Release -DCMAKE_INSTALL_PREFIX="$SYSROOT" \
    -DCMAKE_SYSTEM_NAME=Linux -DCMAKE_SYSTEM_PROCESSOR=$ARCH \
    -DCMAKE_C_COMPILER=clang -DCMAKE_CXX_COMPILER=clang++ -DCMAKE_ASM_COMPILER=clang \
    -DCMAKE_C_COMPILER_TARGET=$T -DCMAKE_CXX_COMPILER_TARGET=$T -DCMAKE_ASM_COMPILER_TARGET=$T \
    -DCMAKE_C_FLAGS="$FLAGS" -DCMAKE_CXX_FLAGS="$FLAGS" -DCMAKE_ASM_FLAGS="$FLAGS" \
    -DCMAKE_AR="$(command -v llvm-ar)" -DCMAKE_RANLIB="$(command -v llvm-ranlib || command -v llvm-ar)" \
    -DCMAKE_TRY_COMPILE_TARGET_TYPE=STATIC_LIBRARY \
    -DLIBCXX_HAS_MUSL_LIBC=ON \
    -DLIBCXX_ENABLE_SHARED=OFF -DLIBCXXABI_ENABLE_SHARED=OFF -DLIBUNWIND_ENABLE_SHARED=OFF \
    -DLIBCXX_USE_COMPILER_RT=ON -DLIBCXXABI_USE_COMPILER_RT=ON -DLIBUNWIND_USE_COMPILER_RT=ON \
    -DLIBCXXABI_USE_LLVM_UNWINDER=ON -DLIBCXXABI_ENABLE_STATIC_UNWINDER=ON \
    -DLIBCXXABI_STATICALLY_LINK_UNWINDER_IN_STATIC_LIBRARY=ON \
    -DLIBCXX_ENABLE_STATIC_ABI_LIBRARY=ON -DLIBCXX_STATICALLY_LINK_ABI_IN_STATIC_LIBRARY=ON \
    -DLIBCXX_INCLUDE_TESTS=OFF -DLIBCXX_INCLUDE_BENCHMARKS=OFF -DLIBCXXABI_INCLUDE_TESTS=OFF \
    -DLIBUNWIND_INCLUDE_TESTS=OFF -DLIBCXX_INCLUDE_DOCS=OFF -DLIBUNWIND_INCLUDE_DOCS=OFF \
    -DLLVM_INCLUDE_TESTS=OFF -DLLVM_INCLUDE_DOCS=OFF >/dev/null
ninja -C "$B/cxx" >/dev/null
ninja -C "$B/cxx" install >/dev/null
echo "[cxx-runtime] $ARCH: libc++ (+abi, unwind) em $SYSROOT/lib"
