#!/bin/sh
# $1 = runlist, $2 = log, $3 = SMP, $4 = timeout s
cd ${PULSAR_DIR}
${BENCH_DIR}/mkdisk.sh $1 >/dev/null
timeout ${4:-280} qemu-system-aarch64 -machine virt,gic-version=2 -cpu max,pauth=off -accel tcg,thread=multi,tb-size=512 \
 -smp ${3:-1} -m 1536M -nographic -serial file:$2 -monitor none \
 -global virtio-mmio.force-legacy=false -device virtio-gpu-device -device virtio-keyboard-device -device virtio-tablet-device \
 -drive file=build/aarch64/bench.img,if=none,format=raw,cache=unsafe,aio=threads,id=hd0 -device virtio-blk-device,drive=hd0 \
 -netdev user,id=n0 -device virtio-net-device,netdev=n0 -device virtio-rng-device \
 -kernel ${KERNEL:-target/aarch64-unknown-none-softfloat/release/pulsar_kernel} &
QP=$!
while kill -0 $QP 2>/dev/null; do grep -q ALLDONE $2 2>/dev/null && { sleep 1; kill $QP; break; }; sleep 2; done
wait $QP 2>/dev/null; true
