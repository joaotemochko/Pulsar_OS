# libc mínima do Pulsar OS — gera libpulsar.a
CC = clang --target=aarch64-none-elf
CFLAGS = -ffreestanding -nostdlib -mgeneral-regs-only -fno-stack-protector -Os -Wall -Iinclude
AR = llvm-ar-18

OBJS = obj/string.o obj/graphics.o obj/text.o obj/wm.o obj/font_ui.o obj/crt0.o

all: libpulsar.a
obj/%.o: src/%.c | obj
	$(CC) $(CFLAGS) -c $< -o $@
obj/crt0.o: src/crt0.s | obj
	$(CC) $(CFLAGS) -c $< -o $@
libpulsar.a: $(OBJS)
	$(AR) rcs $@ $(OBJS)
obj:
	mkdir -p obj
clean:
	rm -rf obj libpulsar.a
