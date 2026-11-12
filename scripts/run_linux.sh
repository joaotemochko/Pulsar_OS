#!/bin/sh
# mesmo disco ext2, mesmo QEMU/CPU, kernel Linux 6.12
cd ${PULSAR_DIR}
${BENCH_DIR}/mkdisk.sh $1 >/dev/null || exit 1
timeout ${4:-280} qemu-system-aarch64 -machine virt,gic-version=2 -cpu max,pauth=off -accel tcg,thread=multi,tb-size=512 \
 -smp ${3:-1} -m 1536M -nographic -serial file:$2 -monitor none \
 -global virtio-mmio.force-legacy=false \
 -drive file=build/aarch64/bench.img,if=none,format=raw,cache=unsafe,aio=threads,id=hd0 -device virtio-blk-device,drive=hd0 \
 -kernel ${LINUX_IMAGE} -append "console=ttyAMA0 root=/dev/vda rw init=/bin/runall quiet" &
QP=$!
while kill -0 $QP 2>/dev/null; do grep -q ALLDONE $2 2>/dev/null && { sleep 1; kill $QP; break; }; sleep 2; done
wait $QP 2>/dev/null; true
