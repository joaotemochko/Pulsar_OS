#!/bin/sh
M=${BENCH_DIR}; mkdir -p $M/campx
for b in 1 2 3 4 5 6 7 8 9 10; do
  KERNEL=$M/k_lazy  $M/run_pulsar.sh $M/exc.list $M/campx/lazy_b$b.log 1 280 >/dev/null 2>&1
  KERNEL=$M/k_eager $M/run_pulsar.sh $M/exc.list $M/campx/eager_b$b.log 1 280 >/dev/null 2>&1
  $M/run_linux.sh $M/exc.list $M/campx/linux_b$b.log 1 280 >/dev/null 2>&1
  echo $b >> $M/campx/progress
done
echo FIM >> $M/campx/progress
