#!/bin/sh
M=${BENCH_DIR}
$M/runltp_pulsar.sh $M/ltp_tests.txt $M/ltp_pulsar.log 1 3000 >/dev/null 2>&1; echo pulsar >> $M/ltp_done.txt
$M/runltp_linux.sh $M/ltp_tests.txt $M/ltp_linux.log 1 3000 >/dev/null 2>&1; echo linux >> $M/ltp_done.txt
