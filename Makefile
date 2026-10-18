# Pulsar OS — build para AArch64 (padrao) ou RISC-V RV64.
#
#   make                      compila kernel + apps + disco (ARCH=aarch64)
#   make run                  compila e roda no QEMU
#   make run ARCH=riscv64     o mesmo, para RISC-V
#   make run SMP=8            numero de nucleos (padrao 4)
#   make linux_apps           so a musl e os programas Linux de user/linux (/bin)
#
# Tudo que depende da arquitetura vai para build/$(ARCH)/.

.PHONY: all kernel user user_rust user_c disk run run-debug clean distclean musl linux_apps

ARCH ?= aarch64
SMP  ?= 4
MEM  ?= 4G

# Rust baremetal precisa de build-std (nightly ou RUSTC_BOOTSTRAP=1).
export RUSTC_BOOTSTRAP := 1

# RUST_TARGET: o kernel (soft-float, nunca toca a FPU). USER_TARGET: os apps
# (hard-float; o kernel salva os registradores de FP de cada processo).
ifeq ($(ARCH),aarch64)
  RUST_TARGET  := aarch64-unknown-none-softfloat
  USER_TARGET  := aarch64-unknown-none
  CLANG_TARGET := --target=aarch64-none-elf
  QEMU         := qemu-system-aarch64 -machine virt,gic-version=2 -cpu cortex-a53
else ifeq ($(ARCH),riscv64)
  RUST_TARGET  := riscv64imac-unknown-none-elf
  USER_TARGET  := riscv64gc-unknown-none-elf
  CLANG_TARGET := --target=riscv64-none-elf -march=rv64imafdc -mabi=lp64d -mcmodel=medany -mno-relax
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
C_APPS    := terminal calc netmon vela hello_c init wlsrv wlclient

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
STACK_init     = 0x10400000
STACK_wlsrv    = 0x10400000
STACK_wlclient = 0x10400000

DISK_FILES := init wlsrv wlclient fsd shell editor ball sysmon terminal calc netmon vela hello_c trespass
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
	    (cd user/$$app && cargo build --release --target $(USER_TARGET) -q) || exit 1; \
	done
	@$(foreach app,$(RUST_APPS),python3 tools/mkpulse.py \
	    user/$(app)/target/$(USER_TARGET)/release/$(app) $(BUILD)/$(app).pulse $(STACK_$(app)) >/dev/null &&) true

# ---- apps em C (libc minima) ----
# O kernel e softfloat, mas salva/restaura os registradores de FP de cada
# processo: os apps usam a FPU (alvo hard-float). A libm vem do
# compiler_builtins do Rust para o alvo de usuario (rustup target add).
BUILTINS  := $(firstword $(wildcard $(shell rustc --print sysroot)/lib/rustlib/$(USER_TARGET)/lib/libcompiler_builtins-*.rlib))
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
# carimbo das flags do compilador: se o alvo mudar (ex.: ponto flutuante),
# as bibliotecas sao recompiladas em vez de misturar ABIs no link
CC_STAMP := $(BUILD)/.cc-flags
$(CC_STAMP): FORCE
	@mkdir -p $(BUILD)
	@echo '$(CC)' | cmp -s - $@ || echo '$(CC)' > $@
.PHONY: FORCE
FORCE:

TP := user/third_party
BEARSSL_SRCS := $(shell find $(TP)/bearssl/src -name '*.c')
BEARSSL_OBJS := $(patsubst $(TP)/bearssl/src/%.c,$(BUILD)/bearssl/%.o,$(BEARSSL_SRCS))
BEARSSL_A    := $(BUILD)/libbearssl.a
$(BUILD)/bearssl/%.o: $(TP)/bearssl/src/%.c $(CC_STAMP)
	@mkdir -p $(dir $@)
	@$(CC) -Iuser/libc/include -I$(TP)/bearssl/inc -I$(TP)/bearssl/src -c $< -o $@
$(BEARSSL_A): $(BEARSSL_OBJS)
	@echo "== BearSSL ($(ARCH)) =="
	@$(AR) rcs $@ $^

BROTLI_SRCS := $(wildcard $(TP)/brotli/common/*.c $(TP)/brotli/dec/*.c)
BROTLI_OBJS := $(patsubst $(TP)/brotli/%.c,$(BUILD)/brotli/%.o,$(BROTLI_SRCS))
BROTLI_A    := $(BUILD)/libbrotlidec.a
$(BUILD)/brotli/%.o: $(TP)/brotli/%.c $(CC_STAMP)
	@mkdir -p $(dir $@)
	@$(subst -Os,-O2,$(CC)) -w -Iuser/libc/include -I$(TP)/brotli/include -c $< -o $@
$(BROTLI_A): $(BROTLI_OBJS)
	@echo "== Brotli ($(ARCH)) =="
	@$(AR) rcs $@ $^

QJS_SRCS := quickjs libregexp libunicode cutils dtoa
QJS_OBJS := $(addprefix $(BUILD)/quickjs/,$(addsuffix .o,$(QJS_SRCS)))
QJS_A    := $(BUILD)/libquickjs.a
QJS_CC   := $(subst -Os,-O2,$(CC)) -w -D__pulsar__ -DNDEBUG -DCONFIG_VERSION=\"$(shell cat $(TP)/quickjs/VERSION)\"
$(BUILD)/quickjs/%.o: $(TP)/quickjs/%.c $(wildcard $(TP)/quickjs/*.h) $(CC_STAMP)
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
$(VELA_OBJ)/%.o: user/vela/%.c user/vela/vela.h user/libc/include/pulsar.h $(CC_STAMP)
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

# ---- musl e programas Linux ----
# A musl e compilada sem modificacoes para <arch>-linux-musl: o kernel atende
# as syscalls do Linux para binarios ELF (kernel/src/linux.rs). Os programas
# de user/linux/*.c sao ligados estaticamente e vao para /bin no disco.
MUSL_TRIPLE := $(ARCH)-linux-musl
MUSL_SRC    := user/third_party/musl
MUSL_BUILD  := $(BUILD)/musl
SYSROOT     := $(BUILD)/sysroot
MUSL_LIBC   := $(SYSROOT)/lib/libc.a
# cabecalhos da musl primeiro; os do proprio clang (stdatomic.h, intrinsics) depois
LCC         := clang --target=$(MUSL_TRIPLE) -nostdinc -isystem $(SYSROOT)/include \
               -isystem $(shell clang -print-resource-dir)/include -O2 -Wall
LINUX_APPS  := $(basename $(notdir $(wildcard user/linux/*.c user/linux/*.cpp)))
LINUX_BINS  := $(addprefix $(BUILD)/linux/,$(LINUX_APPS))
# programas ligados dinamicamente (user/linux/dynamic) e bibliotecas
# compartilhadas (user/linux/lib/<nome>.c -> /lib/lib<nome>.so)
DYN_APPS    := $(basename $(notdir $(wildcard user/linux/dynamic/*.c)))
DYN_BINS    := $(addprefix $(BUILD)/linux/dyn/,$(DYN_APPS))
SO_LIBS     := $(addprefix $(BUILD)/linux/lib/lib,$(addsuffix .so,$(basename $(notdir $(wildcard user/linux/lib/*.c)))))
LDSO_NAME   := ld-musl-$(ARCH).so.1
# rotinas do compilador (long double etc.) para o libc.so: os objetos do
# compiler_builtins do Rust num .a comum, mais o que falta nele
# (user/linux/rt). --syslibdir mantem o ld-musl dentro do sysroot.
RTLIB       := $(BUILD)/libcompiler_rt.a

RT_SRCS     := $(wildcard user/linux/rt/*.c)
# as mesmas rotinas, para os programas estaticos (o rlib do Rust nao as tem)
PULSAR_RT   := $(BUILD)/libpulsar-rt.a
$(PULSAR_RT): $(RT_SRCS)
	@mkdir -p $(BUILD)/prt
	@for f in $(RT_SRCS); do clang --target=$(MUSL_TRIPLE) -ffreestanding -O2 -fPIC -c $$f -o $(BUILD)/prt/$$(basename $$f .c).o || exit 1; done
	@rm -f $@ && $(AR) rcs $@ $(BUILD)/prt/*.o

$(RTLIB): $(RT_SRCS)
	@rm -rf $(BUILD)/rtlib && mkdir -p $(BUILD)/rtlib
	@for f in $(RT_SRCS); do clang --target=$(MUSL_TRIPLE) -ffreestanding -O2 -fPIC -c $$f -o $(BUILD)/rtlib/zz_$$(basename $$f .c).o || exit 1; done
	@cd $(BUILD)/rtlib && $(AR) x $(BUILTINS) && rm -f lib.rmeta && $(AR) rcs $(CURDIR)/$@ *.o
	@rm -rf $(BUILD)/rtlib

$(MUSL_LIBC): $(RTLIB)
	@echo "== musl ($(ARCH)) =="
	@rm -rf $(MUSL_BUILD) && mkdir -p $(MUSL_BUILD)
	@cd $(MUSL_BUILD) && $(CURDIR)/$(MUSL_SRC)/configure --target=$(MUSL_TRIPLE) \
	    --prefix=$(CURDIR)/$(SYSROOT) --syslibdir=$(CURDIR)/$(SYSROOT)/lib CC="clang --target=$(MUSL_TRIPLE)" \
	    AR=$(AR) RANLIB="$(AR) s" CFLAGS=-O2 LDFLAGS=-fuse-ld=lld LIBCC=$(CURDIR)/$(RTLIB) >/dev/null
	@$(MAKE) -s -C $(MUSL_BUILD) -j8 install >/dev/null

# bibliotecas graficas da compatibilidade (fontes originais em user/third_party):
# libffi, libwayland-client, xdg-shell e libxkbcommon, compiladas sem meson
LINUX_GFX := $(SYSROOT)/lib/libxkbcommon.a
$(LINUX_GFX): $(MUSL_LIBC) tools/build_linux_libs.sh
	@echo "== libffi, libwayland-client, libxkbcommon ($(ARCH)) =="
	@AR=$(AR) sh tools/build_linux_libs.sh $(ARCH) $(SYSROOT) $(BUILD)/linuxlibs $(LCC) >/dev/null
# cabecalhos UAPI do Linux (linux/*.h, asm/*.h), os do pacote linux-headers
# do Alpine, por arquitetura
LINUX_HDRS := $(SYSROOT)/include/linux/futex.h
$(LINUX_HDRS): $(MUSL_LIBC)
	@cp -r user/third_party/linux-headers/$(ARCH)/. $(SYSROOT)/include/
# C++: libc++, libc++abi e libunwind do LLVM (tools/build_cxx_runtime.sh)
CXX_RT := $(SYSROOT)/lib/libc++.a
$(CXX_RT): $(LINUX_HDRS) tools/build_cxx_runtime.sh
	@echo "== libc++, libc++abi, libunwind ($(ARCH)) =="
	@sh tools/build_cxx_runtime.sh $(ARCH) $(SYSROOT) $(BUILD)/cxxrt >/dev/null 2>&1 \
	    || { echo "falhou: veja $(BUILD)/cxxrt"; exit 1; }
LCXX := clang++ --target=$(MUSL_TRIPLE) --sysroot=$(CURDIR)/$(SYSROOT) -stdlib=libc++ -O2 -Wall

# fontes: expat + freetype + fontconfig (os configure originais, cruzando via
# tools/musl-cc.sh); os programas fc-* e /etc/fonts vao para $(FONT_ROOT)
FONT_LIBS := $(SYSROOT)/lib/libfontconfig.a
FONT_ROOT := $(BUILD)/fontroot
$(FONT_LIBS): $(MUSL_LIBC) $(RTLIB) tools/build_font_libs.sh tools/musl-cc.sh
	@echo "== expat, freetype, fontconfig ($(ARCH)) =="
	@AR=$(AR) sh tools/build_font_libs.sh $(ARCH) $(SYSROOT) $(RTLIB) $(BUILD)/fontlibs $(FONT_ROOT) >/dev/null 2>&1 \
	    || { echo "falhou: veja $(BUILD)/fontlibs"; exit 1; }
# texto e desenho 2D: zlib, libpng (configure), pixman, harfbuzz, cairo (meson)
DRAW_LIBS := $(SYSROOT)/lib/libcairo.a
$(DRAW_LIBS): $(CXX_RT) $(FONT_LIBS) tools/build_draw_libs.sh
	@echo "== zlib, libpng, pixman, harfbuzz, cairo ($(ARCH)) =="
	@sh tools/build_draw_libs.sh $(ARCH) $(SYSROOT) $(RTLIB) $(BUILD)/drawlibs >/dev/null 2>&1 \
	    || { echo "falhou: veja $(BUILD)/drawlibs/*.log"; exit 1; }
# base do GTK: pcre2 (configure), glib, fribidi, pango, gdk-pixbuf (meson)
GLIB_LIBS := $(SYSROOT)/lib/libgdk_pixbuf-2.0.a
$(GLIB_LIBS): $(DRAW_LIBS) tools/build_glib_libs.sh tools/meson-cross.sh
	@echo "== pcre2, glib, fribidi, pango, gdk-pixbuf ($(ARCH)) =="
	@sh tools/build_glib_libs.sh $(ARCH) $(SYSROOT) $(RTLIB) $(BUILD)/glibs >/dev/null 2>&1 \
	    || { echo "falhou: veja $(BUILD)/glibs/*.log"; exit 1; }
GFX_LIBS := -L$(SYSROOT)/lib -lpangocairo-1.0 -lpangoft2-1.0 -lpango-1.0 -lfribidi -lgdk_pixbuf-2.0 \
            -lgio-2.0 -lgobject-2.0 -lgmodule-2.0 -lglib-2.0 -lpcre2-8 \
            -lcairo -lharfbuzz -lpixman-1 -lpng16 -lz -lxdg-shell -lwayland-client -lffi \
            -lxkbcommon -lfontconfig -lfreetype -lexpat -lc++
# cabecalhos das bibliotecas acima (sem pkg-config: na primeira passada elas
# ainda nao existem)
LINUX_INC := $(addprefix -I$(SYSROOT)/include/,freetype2 harfbuzz cairo pango-1.0 glib-2.0 gdk-pixbuf-2.0 \
             fribidi gio-unix-2.0) -I$(SYSROOT)/lib/glib-2.0/include

$(BUILD)/linux/%: user/linux/%.c $(MUSL_LIBC) $(LINUX_GFX) $(GLIB_LIBS) $(PULSAR_RT)
	@mkdir -p $(BUILD)/linux
	@$(LCC) $(LINUX_INC) -c $< -o $@.o
	@$(LD) -static --eh-frame-hdr $(SYSROOT)/lib/crt1.o $(SYSROOT)/lib/crti.o $@.o $(GFX_LIBS) $(MUSL_LIBC) $(PULSAR_RT) $(BUILTINS) \
	    $(SYSROOT)/lib/crtn.o -o $@

$(BUILD)/linux/%: user/linux/%.cpp $(MUSL_LIBC) $(LINUX_GFX) $(GLIB_LIBS) $(PULSAR_RT)
	@mkdir -p $(BUILD)/linux
	@$(LCXX) $(LINUX_INC) -c $< -o $@.o
	@$(LD) -static --eh-frame-hdr $(SYSROOT)/lib/crt1.o $(SYSROOT)/lib/crti.o $@.o $(GFX_LIBS) $(MUSL_LIBC) \
	    $(PULSAR_RT) $(BUILTINS) $(SYSROOT)/lib/crtn.o -o $@

$(BUILD)/linux/lib/lib%.so: user/linux/lib/%.c $(MUSL_LIBC)
	@mkdir -p $(BUILD)/linux/lib
	@$(LCC) -fPIC -c $< -o $@.o
	@$(LD) -shared -soname lib$*.so $@.o -L$(SYSROOT)/lib -lc -o $@

$(BUILD)/linux/dyn/%: user/linux/dynamic/%.c $(MUSL_LIBC) $(SO_LIBS)
	@mkdir -p $(BUILD)/linux/dyn
	@$(LCC) -fPIE -I user/linux/lib -c $< -o $@.o
	@$(LD) -pie -dynamic-linker /lib/$(LDSO_NAME) $(SYSROOT)/lib/Scrt1.o $(SYSROOT)/lib/crti.o $@.o \
	    -L$(BUILD)/linux/lib $(patsubst $(BUILD)/linux/lib/lib%.so,-l%,$(SO_LIBS)) \
	    -L$(SYSROOT)/lib -lc $(SYSROOT)/lib/crtn.o -o $@

# O mesmo programa convertido para .pulse (tools/elf2pulse.py): executavel
# nativo do Pulsar que roda na personalidade Linux.
$(BUILD)/linux/%.pulse: $(BUILD)/linux/% tools/elf2pulse.py
	@python3 tools/elf2pulse.py $< $@ >/dev/null

musl: $(MUSL_LIBC) $(LINUX_GFX) $(FONT_LIBS) $(CXX_RT) $(DRAW_LIBS) $(GLIB_LIBS)
linux_apps: $(LINUX_BINS) $(addsuffix .pulse,$(LINUX_BINS)) $(DYN_BINS) $(SO_LIBS)

# ---- disco ----
# ext2 (padrao): gerado pelo mke2fs do host a partir de $(BUILD)/root, sem
# root/sudo. Os apps e arquivos de configuracao ficam na raiz (os apps antigos
# os acham por nome); a arvore Unix fica pronta para o que vier.
# rootfs/ (repositorio) tem os arquivos de /etc (resolv.conf, hosts...).
# DISKFS=nbfs gera o Nebular FS plano antigo.
DISKFS    ?= ext2
DISK_SIZE ?= 512M
MKE2FS    := $(shell command -v mke2fs || echo /sbin/mke2fs)
ROOTFS    := $(BUILD)/root
ROOT_DIRS := bin dev etc home/user proc run tmp usr/bin usr/lib usr/share

disk: user $(if $(filter ext2,$(DISKFS)),linux_apps)
	@echo "== Disco $(DISK) ($(DISKFS)) =="
ifeq ($(DISKFS),ext2)
	@rm -rf $(ROOTFS) && mkdir -p $(addprefix $(ROOTFS)/,$(ROOT_DIRS))
	@cp $(DISK_PULSES) welcome.txt pulsar.cfg $(ROOTFS)/
	@cp -r rootfs/. $(ROOTFS)/
	@$(if $(LINUX_BINS),cp $(LINUX_BINS) $(addsuffix .pulse,$(LINUX_BINS)) $(ROOTFS)/bin/)
	@mkdir -p $(ROOTFS)/lib && cp $(SYSROOT)/lib/libc.so $(SO_LIBS) $(ROOTFS)/lib/ && ln -sf libc.so $(ROOTFS)/lib/$(LDSO_NAME)
	@$(if $(DYN_BINS),cp $(DYN_BINS) $(ROOTFS)/bin/)
	@cp -r $(FONT_ROOT)/. $(ROOTFS)/ && mkdir -p $(ROOTFS)/usr/share/fonts/pulsar \
	    && cp user/third_party/fonts-linux/* $(ROOTFS)/usr/share/fonts/pulsar/
	@rm -f $(DISK)
	@$(MKE2FS) -q -F -t ext2 -b 4096 -O none,filetype,sparse_super,large_file \
	    -E root_owner=0:0 -L Pulsar -d $(ROOTFS) $(DISK) $(DISK_SIZE)
else
	@python3 tools/mknblr.py $(DISK) $(DISK_PULSES) welcome.txt pulsar.cfg >/dev/null
endif

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
