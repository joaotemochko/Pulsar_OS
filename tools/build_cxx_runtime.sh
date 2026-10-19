#!/bin/sh
# build_cxx_runtime.sh — runtime de C/C++ dos programas Linux do Pulsar: libc++,
# libc++abi, libunwind e compiler-rt (builtins + crtbegin/crtend) do LLVM 18.1.3 (mesma versao do clang do build), dos
# fontes originais em user/third_party/llvm-runtimes, compiladas com o CMake
# do proprio LLVM para <arch>-linux-musl. Saem estaticas (a libc++.a ja traz a
# libc++abi e a libunwind) e compartilhadas (libc++.so, libc++abi.so,
# libunwind.so), com excecoes, RTTI, threads e locale.
#
# Uso: build_cxx_runtime.sh <arch> <sysroot> <dir-de-build>
# Instala em <sysroot>/include/c++/v1, <sysroot>/lib/libc++.a e
# <sysroot>/lib/linux/libclang_rt.builtins-<arch>.a (+ clang_rt.crt*.o): as
# rotinas do compilador em PIC, que servem tambem para bibliotecas .so (as do
# compiler_builtins do Rust sao para bare metal, sem PIC).
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
COMMON="-DCMAKE_BUILD_TYPE=Release -DCMAKE_INSTALL_PREFIX=$SYSROOT \
    -DCMAKE_SYSTEM_NAME=Linux -DCMAKE_SYSTEM_PROCESSOR=$ARCH \
    -DCMAKE_C_COMPILER=clang -DCMAKE_CXX_COMPILER=clang++ -DCMAKE_ASM_COMPILER=clang \
    -DCMAKE_C_COMPILER_TARGET=$T -DCMAKE_CXX_COMPILER_TARGET=$T -DCMAKE_ASM_COMPILER_TARGET=$T \
    -DCMAKE_AR=$(command -v llvm-ar) -DCMAKE_RANLIB=$(command -v llvm-ranlib || command -v llvm-ar) \
    -DCMAKE_TRY_COMPILE_TARGET_TYPE=STATIC_LIBRARY \
    -DLLVM_INCLUDE_TESTS=OFF -DLLVM_INCLUDE_DOCS=OFF"
LD="-fuse-ld=lld -Wl,--eh-frame-hdr"

# 1) compiler-rt: builtins (PIC) e crtbegin/crtend. Viram a libgcc.a e os
#    crt*.o do sysroot, que o driver do clang usa ao ligar programas e .so
rm -rf "${B:?}/crt" && mkdir -p "$B/crt"
cmake -G Ninja -S "$SRC/runtimes" -B "$B/crt" $COMMON \
    -DCMAKE_C_FLAGS="$FLAGS" -DCMAKE_CXX_FLAGS="$FLAGS" -DCMAKE_ASM_FLAGS="$FLAGS" \
    -DLLVM_ENABLE_RUNTIMES="compiler-rt" \
    -DCOMPILER_RT_BUILD_BUILTINS=ON -DCOMPILER_RT_BUILD_CRT=ON -DCOMPILER_RT_BUILD_SANITIZERS=OFF \
    -DCOMPILER_RT_BUILD_XRAY=OFF -DCOMPILER_RT_BUILD_LIBFUZZER=OFF -DCOMPILER_RT_BUILD_PROFILE=OFF \
    -DCOMPILER_RT_BUILD_MEMPROF=OFF -DCOMPILER_RT_BUILD_ORC=OFF -DCOMPILER_RT_BUILD_GWP_ASAN=OFF \
    -DCOMPILER_RT_DEFAULT_TARGET_ONLY=ON -DLLVM_ENABLE_PER_TARGET_RUNTIME_DIR=OFF \
    -DCOMPILER_RT_INCLUDE_TESTS=OFF >/dev/null
ninja -C "$B/crt" >/dev/null && ninja -C "$B/crt" install >/dev/null
CRT=$SYSROOT/lib/linux
cp "$CRT/libclang_rt.builtins-$ARCH.a" "$SYSROOT/lib/libgcc.a"
for o in crtbegin crtbeginS crtbeginT; do cp "$CRT/clang_rt.crtbegin-$ARCH.o" "$SYSROOT/lib/$o.o"; done
for o in crtend crtendS; do cp "$CRT/clang_rt.crtend-$ARCH.o" "$SYSROOT/lib/$o.o"; done
# vazias: o unwinder esta na libc++/libunwind; atomicos sao inline nas duas
# arquiteturas (a libc++ pede -latomic mesmo assim)
for l in gcc_eh gcc_s atomic; do [ -f "$SYSROOT/lib/lib$l.a" ] || llvm-ar rcs "$SYSROOT/lib/lib$l.a"; done

# 2) libc++, libc++abi e libunwind, estaticas e compartilhadas
rm -rf "${B:?}/cxx" && mkdir -p "$B/cxx"
cmake -G Ninja -S "$SRC/runtimes" -B "$B/cxx" $COMMON \
    -DCMAKE_C_FLAGS="$FLAGS" -DCMAKE_CXX_FLAGS="$FLAGS" -DCMAKE_ASM_FLAGS="$FLAGS" \
    -DCMAKE_SHARED_LINKER_FLAGS="$LD" -DCMAKE_EXE_LINKER_FLAGS="$LD" \
    -DLLVM_ENABLE_RUNTIMES="libcxx;libcxxabi;libunwind" \
    -DLIBCXX_HAS_MUSL_LIBC=ON -DLIBCXXABI_HAS_CXA_THREAD_ATEXIT_IMPL=OFF \
    -DLIBCXX_ENABLE_SHARED=ON -DLIBCXXABI_ENABLE_SHARED=ON -DLIBUNWIND_ENABLE_SHARED=ON \
    -DLIBCXX_USE_COMPILER_RT=OFF -DLIBCXXABI_USE_COMPILER_RT=OFF -DLIBUNWIND_USE_COMPILER_RT=OFF \
    -DLIBCXXABI_USE_LLVM_UNWINDER=ON -DLIBCXXABI_ENABLE_STATIC_UNWINDER=ON \
    -DLIBCXXABI_STATICALLY_LINK_UNWINDER_IN_STATIC_LIBRARY=ON \
    -DLIBCXX_ENABLE_STATIC_ABI_LIBRARY=ON -DLIBCXX_STATICALLY_LINK_ABI_IN_STATIC_LIBRARY=ON \
    -DLIBCXX_INCLUDE_TESTS=OFF -DLIBCXX_INCLUDE_BENCHMARKS=OFF -DLIBCXXABI_INCLUDE_TESTS=OFF \
    -DLIBUNWIND_INCLUDE_TESTS=OFF -DLIBCXX_INCLUDE_DOCS=OFF -DLIBUNWIND_INCLUDE_DOCS=OFF >/dev/null
ninja -C "$B/cxx" >/dev/null
ninja -C "$B/cxx" install >/dev/null
echo "[cxx-runtime] $ARCH: libc++ (+abi, unwind) e compiler-rt em $SYSROOT/lib"
