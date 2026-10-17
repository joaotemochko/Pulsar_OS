# Pulsar OS — build para AArch64 (padrao) ou RISC-V RV64.
#
#   make                      compila kernel + apps + disco (ARCH=aarch64)
#   make run                  compila e roda no QEMU
#   make run ARCH=riscv64     o mesmo, para RISC-V
#   make run SMP=8            numero de nucleos (padrao 4)
#
# Tudo que depende da arquitetura vai para build/$(ARCH)/.

.PHONY: all kernel user user_rust user_c disk run run-debug clean distclean

ARCH ?= aarch64
SMP  ?= 4
MEM  ?= 4G

# Rust baremetal precisa de build-std (nightly ou RUSTC_BOOTSTRAP=1).
export RUSTC_BOOTSTRAP := 1

ifeq ($(ARCH),aarch64)
  RUST_TARGET  := aarch64-unknown-none-softfloat
  CLANG_TARGET := --target=aarch64-none-elf -mgeneral-regs-only
  QEMU         := qemu-system-aarch64 -machine virt,gic-version=2 -cpu cortex-a53
else ifeq ($(ARCH),riscv64)
  RUST_TARGET  := riscv64imac-unknown-none-elf
  CLANG_TARGET := --target=riscv64-none-elf -march=rv64imac -mabi=lp64 -mcmodel=medany -mno-relax
  QEMU         := qemu-system-riscv64 -machine virt
else
  $(error ARCH deve ser aarch64 ou riscv64)
endif

BUILD  := build/$(ARCH)
KERNEL := target/$(RUST_TARGET)/release/pulsar_kernel
DISK   := $(BUILD)/disk.img

CC      := clang $(CLANG_TARGET) -ffreestanding -nostdlib -fno-stack-protector -ffp-contract=off -Os -Wall
AR      := $(shell command -v llvm-ar-18 || command -v llvm-ar || echo ar)
LD      := ld.lld

# ---- apps: nome -> topo da pilha (o loader mapeia 64 KiB abaixo) ----
RUST_APPS := shell fsd ball editor sysmon trespass
C_APPS    := terminal calc netmon vela hello_c

STACK_shell    = 0x02F00000
STACK_ball     = 0x04100000
STACK_trespass = 0x06100000
STACK_sysmon   = 0x08100000
STACK_editor   = 0x0B100000
STACK_fsd      = 0x0D100000
STACK_terminal = 0x11400000
STACK_calc     = 0x12400000
STACK_netmon   = 0x13400000
STACK_vela     = 0x16000000
STACK_hello_c  = 0x10400000

DISK_FILES := fsd shell editor ball sysmon terminal calc netmon vela hello_c trespass
DISK_PULSES := $(addprefix $(BUILD)/,$(addsuffix .pulse,$(DISK_FILES)))

all: kernel user disk

kernel:
	@echo "== Kernel Pulsar ($(ARCH)) =="
	@cargo build --release --target $(RUST_TARGET)

user: user_rust user_c

# ---- apps em Rust (plib) ----
user_rust: | $(BUILD)
	@echo "== Apps Rust ($(ARCH)) =="
	@for app in $(RUST_APPS); do \
	    (cd user/$$app && cargo build --release --target $(RUST_TARGET) -q) || exit 1; \
	done
	@$(foreach app,$(RUST_APPS),python3 tools/mkpulse.py \
	    user/$(app)/target/$(RUST_TARGET)/release/$(app) $(BUILD)/$(app).pulse $(STACK_$(app)) >/dev/null &&) true

# ---- apps em C (libc minima) ----
# Ponto flutuante por software (o kernel nao salva os registradores de FP)
# e a libm vem do compiler_builtins do Rust, ja instalado para o alvo.
BUILTINS  := $(firstword $(wildcard $(shell rustc --print sysroot)/lib/rustlib/$(RUST_TARGET)/lib/libcompiler_builtins-*.rlib))
LIBC_OBJ  := $(BUILD)/libc
LIBC_SRCS := string malloc thread stdio stdlib math time graphics text wm font_ui font_ui_bold font_small font_mono font_title font_huge
LIBC_OBJS := $(addprefix $(LIBC_OBJ)/,$(addsuffix .o,$(LIBC_SRCS)))
LIBC_A    := $(LIBC_OBJ)/libpulsar.a
CRT0      := $(LIBC_OBJ)/crt0.o

$(LIBC_OBJ)/%.o: user/libc/src/%.c user/libc/include/*.h | $(LIBC_OBJ)
	@$(CC) -Iuser/libc/include -Iuser/third_party/stb -c $< -o $@

$(CRT0): user/libc/src/crt0_$(ARCH).s | $(LIBC_OBJ)
	@clang $(CLANG_TARGET) -c $< -o $@ 2>/dev/null

$(LIBC_A): $(LIBC_OBJS)
	@$(AR) rcs $@ $^

# apps C podem ter varios .c (ex.: o Vela); cada um vira um .o
.SECONDEXPANSION:
$(BUILD)/%.elf: $$(wildcard user/%/*.c) $$(wildcard user/%/*.h) user/%/link.ld $(LIBC_A) $(CRT0)
	@mkdir -p $(BUILD)/obj-$*
	@for f in $(wildcard user/$*/*.c); do \
	    $(CC) -Iuser/libc/include -Iuser/$* -c $$f -o $(BUILD)/obj-$*/$$(basename $$f .c).o || exit 1; \
	done
	@$(LD) -T user/$*/link.ld -e _start $(CRT0) $(BUILD)/obj-$*/*.o $(LIBC_A) $(BUILTINS) -o $@

# ---- bibliotecas de terceiros (user/third_party, ver README la) ----
TP := user/third_party
BEARSSL_SRCS := $(shell find $(TP)/bearssl/src -name '*.c')
BEARSSL_OBJS := $(patsubst $(TP)/bearssl/src/%.c,$(BUILD)/bearssl/%.o,$(BEARSSL_SRCS))
BEARSSL_A    := $(BUILD)/libbearssl.a
$(BUILD)/bearssl/%.o: $(TP)/bearssl/src/%.c
	@mkdir -p $(dir $@)
	@$(CC) -Iuser/libc/include -I$(TP)/bearssl/inc -I$(TP)/bearssl/src -c $< -o $@
$(BEARSSL_A): $(BEARSSL_OBJS)
	@echo "== BearSSL ($(ARCH)) =="
	@$(AR) rcs $@ $^

BROTLI_SRCS := $(wildcard $(TP)/brotli/common/*.c $(TP)/brotli/dec/*.c)
BROTLI_OBJS := $(patsubst $(TP)/brotli/%.c,$(BUILD)/brotli/%.o,$(BROTLI_SRCS))
BROTLI_A    := $(BUILD)/libbrotlidec.a
$(BUILD)/brotli/%.o: $(TP)/brotli/%.c
	@mkdir -p $(dir $@)
	@$(subst -Os,-O2,$(CC)) -w -Iuser/libc/include -I$(TP)/brotli/include -c $< -o $@
$(BROTLI_A): $(BROTLI_OBJS)
	@echo "== Brotli ($(ARCH)) =="
	@$(AR) rcs $@ $^

QJS_SRCS := quickjs libregexp libunicode cutils dtoa
QJS_OBJS := $(addprefix $(BUILD)/quickjs/,$(addsuffix .o,$(QJS_SRCS)))
QJS_A    := $(BUILD)/libquickjs.a
QJS_CC   := $(subst -Os,-O2,$(CC)) -w -D__pulsar__ -DNDEBUG -DCONFIG_VERSION=\"$(shell cat $(TP)/quickjs/VERSION)\"
$(BUILD)/quickjs/%.o: $(TP)/quickjs/%.c $(wildcard $(TP)/quickjs/*.h)
	@mkdir -p $(dir $@)
	@$(QJS_CC) -Iuser/libc/include -c $< -o $@
$(QJS_A): $(QJS_OBJS)
	@echo "== QuickJS ($(ARCH)) =="
	@$(AR) rcs $@ $^

# ---- Vela: varios .c + fontes TrueType e o prelude.js embutidos ----
VELA_OBJ  := $(BUILD)/obj-vela
VELA_SRCS := $(wildcard user/vela/*.c)
VELA_OBJS := $(patsubst user/vela/%.c,$(VELA_OBJ)/%.o,$(VELA_SRCS)) $(VELA_OBJ)/gen_fonts.o $(VELA_OBJ)/gen_prelude.o
VELA_CC   := $(CC) -Iuser/libc/include -Iuser/vela -I$(TP)/bearssl/inc -I$(TP)/quickjs -I$(TP)/stb -I$(TP)/nanosvg -I$(TP)/brotli/include
FONTS     := font_sans_regular=$(TP)/fonts/NotoSans-Regular.ttf font_sans_bold=$(TP)/fonts/NotoSans-Bold.ttf \
             font_sans_italic=$(TP)/fonts/NotoSans-Italic.ttf font_sans_bolditalic=$(TP)/fonts/NotoSans-BoldItalic.ttf \
             font_mono_regular=$(TP)/fonts/NotoSansMono-Regular.ttf font_mono_bold=$(TP)/fonts/NotoSansMono-Bold.ttf \
             font_symbols=$(TP)/fonts/DejaVuSans-Symbols.ttf \
             font_serif_regular=$(TP)/fonts/NotoSerif-Regular.ttf font_serif_bold=$(TP)/fonts/NotoSerif-Bold.ttf \
             font_serif_italic=$(TP)/fonts/NotoSerif-Italic.ttf font_serif_bolditalic=$(TP)/fonts/NotoSerif-BoldItalic.ttf \
             font_msans_regular=$(TP)/fonts/PulsarMetricSans-Regular.ttf font_msans_bold=$(TP)/fonts/PulsarMetricSans-Bold.ttf \
             font_msans_italic=$(TP)/fonts/PulsarMetricSans-Italic.ttf font_msans_bolditalic=$(TP)/fonts/PulsarMetricSans-BoldItalic.ttf \
             font_mserif_regular=$(TP)/fonts/PulsarMetricSerif-Regular.ttf font_mserif_bold=$(TP)/fonts/PulsarMetricSerif-Bold.ttf \
             font_mserif_italic=$(TP)/fonts/PulsarMetricSerif-Italic.ttf font_mserif_bolditalic=$(TP)/fonts/PulsarMetricSerif-BoldItalic.ttf
$(VELA_OBJ)/%.o: user/vela/%.c user/vela/vela.h user/libc/include/pulsar.h
	@mkdir -p $(VELA_OBJ)
	@$(VELA_CC) -c $< -o $@
$(VELA_OBJ)/gen_fonts.c: tools/bin2c.py $(wildcard $(TP)/fonts/*.ttf)
	@mkdir -p $(VELA_OBJ)
	@python3 tools/bin2c.py $@ $(FONTS)
$(VELA_OBJ)/gen_prelude.c: tools/bin2c.py user/vela/prelude.js
	@mkdir -p $(VELA_OBJ)
	@python3 tools/bin2c.py $@ vela_prelude=user/vela/prelude.js
$(VELA_OBJ)/gen_%.o: $(VELA_OBJ)/gen_%.c
	@$(CC) -c $< -o $@
$(BUILD)/vela.elf: $(VELA_OBJS) user/vela/link.ld $(LIBC_A) $(CRT0) $(BEARSSL_A) $(QJS_A) $(BROTLI_A)
	@echo "== Vela ($(ARCH)) =="
	@$(LD) -T user/vela/link.ld -e _start $(CRT0) $(VELA_OBJS) $(BEARSSL_A) $(QJS_A) $(BROTLI_A) $(LIBC_A) $(BUILTINS) -o $@

user_c: $(addprefix $(BUILD)/,$(addsuffix .elf,$(C_APPS)))
	@echo "== Apps C ($(ARCH)) =="
	@$(foreach app,$(C_APPS),python3 tools/mkpulse.py \
	    $(BUILD)/$(app).elf $(BUILD)/$(app).pulse $(STACK_$(app)) >/dev/null &&) true

$(BUILD) $(LIBC_OBJ):
	@mkdir -p $@

# ---- disco (Nebular FileSystem) ----
disk: user
	@echo "== Disco $(DISK) =="
	@python3 tools/mknblr.py $(DISK) $(DISK_PULSES) welcome.txt pulsar.cfg >/dev/null

QEMU_ARGS := -smp $(SMP) -m $(MEM) -serial mon:stdio \
	-global virtio-mmio.force-legacy=false \
	-device virtio-gpu-device -device virtio-keyboard-device -device virtio-tablet-device \
	-drive file=$(DISK),if=none,format=raw,id=hd0 -device virtio-blk-device,drive=hd0 \
	-netdev user,id=n0 -device virtio-net-device,netdev=n0 \
	-device virtio-rng-device

run: all
	@echo "== Pulsar OS no QEMU ($(ARCH), $(SMP) CPUs) =="
	@$(QEMU) $(QEMU_ARGS) -kernel $(KERNEL)

# Pausado com stub GDB em :1234
run-debug: all
	@$(QEMU) $(QEMU_ARGS) -kernel $(KERNEL) -S -s

clean:
	@rm -rf build
	@cargo clean
	@for app in $(RUST_APPS) plib; do (cd user/$$app && cargo clean); done

distclean: clean
