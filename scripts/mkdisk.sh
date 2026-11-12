#!/bin/sh
# disco ext2 minimo: init + fsd + programas Linux de teste
set -e
cd ${PULSAR_DIR}; B=build/aarch64; R=$B/benchroot
rm -rf $R; mkdir -p $R/bin $R/dev $R/etc $R/home/user $R/proc $R/run $R/tmp $R/usr/bin $R/usr/lib
cp $B/init.pulse $B/fsd.pulse welcome.txt pulsar.cfg $R/
cp rootfs/etc/hosts rootfs/etc/resolv.conf rootfs/etc/passwd rootfs/etc/group $R/etc/
printf '/fsd.pulse\n' > $R/etc/services
for p in dirleak asidtest orphtest forkbug forkbug2 forkloop stubtest big4 big16 big64 execsz posixtest spawntest sigtest iotest unixtest oopstest lbench runall; do cp $B/linux/$p $R/bin/; done
mkdir -p $R/lib; cp $B/sysroot/lib/libc.so $R/lib/ld-musl-aarch64.so.1; cp $B/linux/lib/libdemo.so $R/lib/; cp $B/linux/dyn/dyntest $R/bin/
cp "$1" $R/etc/runlist
echo "${AUTORUN:-/bin/runall /etc/runlist}" > $R/etc/autorun
chmod 1777 $R/tmp
python3 -c "import uuid;print(uuid.uuid4().hex)" > $R/etc/machine-id
rm -f $B/bench.img
mke2fs -q -F -t ext2 -b 4096 -O none,filetype,sparse_super,large_file -E root_owner=0:0 -L Pulsar -d $R $B/bench.img 512M
echo disco ok
