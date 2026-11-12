#!/bin/sh
# disco com o LTP: $1 = lista de testes
set -e
cd ${PULSAR_DIR}; B=build/aarch64; R=$B/ltproot
rm -rf $R; mkdir -p $R/bin $R/dev $R/etc $R/home/user $R/proc $R/sys $R/run $R/tmp $R/usr/bin $R/opt
cp $B/init.pulse $B/fsd.pulse welcome.txt pulsar.cfg $R/
cp rootfs/etc/hosts rootfs/etc/resolv.conf rootfs/etc/passwd rootfs/etc/group $R/etc/
printf '/fsd.pulse\n' > $R/etc/services
cp -r ${LTP_BIN} $R/opt/ltp
cp $B/linux/ltprun $R/bin/
cp "$1" $R/etc/ltplist
echo "/bin/ltprun /etc/ltplist ${LTP_LIMIT:-60}" > $R/etc/autorun
chmod 1777 $R/tmp; chown -R 1000:1000 $R/home/user
rm -f $B/ltp.img
mke2fs -q -F -t ext2 -b 4096 -O none,filetype,sparse_super,large_file -E root_owner=0:0 -L Pulsar -d $R $B/ltp.img 512M
