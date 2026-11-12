#!/bin/sh
# Compilador cruzado musl estatico (AArch64) usado para o LTP.
# Usa o sysroot da musl gerado pelo Makefile do Pulsar.
SR=${PULSAR_DIR}/build/aarch64/sysroot
mkdir -p $SR/usr && ln -sfn ../include $SR/usr/include && ln -sfn ../lib $SR/usr/lib
B=$(ls $(rustc --print sysroot)/lib/rustlib/aarch64-unknown-none/lib/libcompiler_builtins-*.rlib)
T=$(mktemp -d); (cd $T && llvm-ar x $B && rm -f lib.rmeta && llvm-ar x ${PULSAR_DIR}/build/aarch64/libpulsar-rt.a && llvm-ar rcs $SR/lib/libgcc.a *.o); rm -rf $T
llvm-ar rcs $SR/lib/libgcc_eh.a
echo "" | clang --target=aarch64-linux-musl -x c -c - -o $SR/lib/crtbeginT.o
cp $SR/lib/crtbeginT.o $SR/lib/crtbegin.o; cp $SR/lib/crtbeginT.o $SR/lib/crtend.o
mkdir -p ${BENCH_DIR}/bin
cat > ${BENCH_DIR}/bin/aarch64-linux-musl-cc <<EOT
#!/bin/sh
exec clang --target=aarch64-linux-musl --sysroot=$SR -static -fuse-ld=lld -Wno-unused-command-line-argument "\$@"
EOT
chmod +x ${BENCH_DIR}/bin/aarch64-linux-musl-cc
echo "adicione ${BENCH_DIR}/bin ao PATH"
