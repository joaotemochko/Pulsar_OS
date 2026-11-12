#!/bin/sh
# gera os executaveis de 4, 16 e 64 MiB do experimento de tamanho do execve
for sz in 4 16 64; do
  printf '#include <unistd.h>\nstatic const volatile char pad[%d<<20] = {1};\nint main(int c,char**v){ return pad[0]-1; }\n' $sz > ${BENCH_DIR}/src/big$sz.c
  ${BENCH_DIR}/scripts/build_linux.sh ${BENCH_DIR}/src/big$sz.c
done
