#!/bin/sh
# Compila a selecao do LTP (lists/ltp_dirs.txt) e copia os binarios para $LTP_BIN.
set -e
cd ${LTP_DIR}
make autotools
./configure --host=aarch64-linux-musl CC=aarch64-linux-musl-cc AR=llvm-ar RANLIB=llvm-ranlib STRIP=llvm-strip \
  CFLAGS="-O2 -w" LDFLAGS="-static" --without-numa --without-tirpc --without-expect --without-perl --without-python
make -C lib -j4
for d in $(cat ${BENCH_DIR}/lists/ltp_dirs.txt); do make -C testcases/kernel/syscalls/$d -k -j4 || true; done
mkdir -p ${LTP_BIN}
for d in $(cat ${BENCH_DIR}/lists/ltp_dirs.txt); do
  find testcases/kernel/syscalls/$d -maxdepth 1 -type f -perm -u+x ! -name '*.sh' ! -name '*.c' ! -name 'Makefile*' -exec cp {} ${LTP_BIN}/ \;
done
llvm-strip ${LTP_BIN}/* 2>/dev/null || true
