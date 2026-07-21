#!/bin/bash
# Compila um app C do Pulsar usando a libc mínima (libpulsar.a).
set -e
CC="clang --target=aarch64-none-elf -ffreestanding -nostdlib -mgeneral-regs-only -fno-stack-protector -Os -Wall"
INC="-I../libc/include"
# garante a libc compilada
(cd ../libc && make -s)
mkdir -p obj
$CC $INC -c main.c -o obj/main.o
# crt0 primeiro (define _start), depois main, depois a lib
ld.lld -T link.ld -e _start ../libc/obj/crt0.o obj/main.o ../libc/libpulsar.a -o hello_c.elf
echo "hello_c.elf pronto"
