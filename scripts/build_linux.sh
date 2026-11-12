#!/bin/sh
# compila programas Linux estaticos (musl do Pulsar) para aarch64
set -e
export PATH=/opt/tc:$PATH
cd ${PULSAR_DIR}
B=build/aarch64; SR=$B/sysroot
BUILTINS=$(ls $(rustc --print sysroot)/lib/rustlib/aarch64-unknown-none/lib/libcompiler_builtins-*.rlib)
LCC="clang --target=aarch64-linux-musl -nostdinc -isystem $SR/include -isystem $(clang -print-resource-dir)/include -O2 -w"
mkdir -p $B/linux
for src in "$@"; do
  n=$(basename $src .c)
  $LCC -c $src -o $B/linux/$n.o
  ld.lld -static --eh-frame-hdr $SR/lib/crt1.o $SR/lib/crti.o $B/linux/$n.o $SR/lib/libc.a $B/libpulsar-rt.a $BUILTINS $SR/lib/crtn.o -o $B/linux/$n
  echo "ok $n"
done
