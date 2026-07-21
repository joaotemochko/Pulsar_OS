#!/bin/bash
set -e
CC="clang --target=aarch64-none-elf -ffreestanding -nostdlib -mgeneral-regs-only -fno-stack-protector -Os -Wall"
INC="-I../libc/include"
(cd ../libc && make -s)
mkdir -p obj
$CC $INC -c main.c -o obj/main.o
ld.lld -T link.ld -e _start ../libc/obj/crt0.o obj/main.o ../libc/libpulsar.a -o terminal.elf
echo "terminal.elf pronto"
