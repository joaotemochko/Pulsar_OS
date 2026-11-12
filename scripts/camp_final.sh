#!/bin/sh
M=${BENCH_DIR}; mkdir -p $M/campf
for b in 1 2 3 4 5 6 7 8 9 10; do for g in g1 g2 g3; do
  KERNEL=$M/k_final $M/run_pulsar.sh $M/$g.list $M/campf/${g}_pulsar_b$b.log 1 280 >/dev/null 2>&1
  $M/run_linux.sh $M/$g.list $M/campf/${g}_linux_b$b.log 1 280 >/dev/null 2>&1
done; echo $b >> $M/campf/progress; done; echo FIM >> $M/campf/progress
