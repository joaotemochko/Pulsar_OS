.PHONY: all kernel user user_c disk run clean

# Rust baremetal precisa de build-std (nightly). Usamos o toolchain 1.91
# com RUSTC_BOOTSTRAP=1 para habilitar features instaveis no stable.
export RUSTC_BOOTSTRAP := 1
export CARGO_TARGET_AARCH64_UNKNOWN_NONE_SOFTFLOAT_LINKER := ld.lld

# stacks dos apps Rust
SHELL_STACK    = 0x02100000
BALL_STACK     = 0x04100000
FSD_STACK      = 0x0D100000
EDITOR_STACK   = 0x0B100000
TRESPASS_STACK = 0x06100000
# stacks dos apps C
TERMINAL_STACK = 0x11400000
CALC_STACK     = 0x12400000
NETMON_STACK   = 0x13400000
VELA_STACK     = 0x14400000
HELLO_C_STACK  = 0x10100000

all: kernel user user_c disk

kernel:
	@echo "== Compilando o Pulsar Kernel (Rust) =="
	@cargo build

# ---- apps de usuario em Rust ----
user:
	@echo "== Compilando apps Rust (WM, fsd, editor, ball) =="
	@cd user/shell && cargo build --release
	@cd user/fsd   && cargo build --release
	@cd user/ball  && cargo build --release
	@cd user/editor && cargo build --release
	@cd user/trespass && cargo build --release
	@python3 tools/mkpulse.py user/shell/target/aarch64-unknown-none-softfloat/release/shell shell.pulse $(SHELL_STACK)
	@python3 tools/mkpulse.py user/fsd/target/aarch64-unknown-none-softfloat/release/fsd   fsd.pulse   $(FSD_STACK)
	@python3 tools/mkpulse.py user/ball/target/aarch64-unknown-none-softfloat/release/ball  ball.pulse  $(BALL_STACK)
	@python3 tools/mkpulse.py user/editor/target/aarch64-unknown-none-softfloat/release/editor editor.pulse $(EDITOR_STACK)
	@python3 tools/mkpulse.py user/trespass/target/aarch64-unknown-none-softfloat/release/trespass trespass.pulse $(TRESPASS_STACK)

# ---- apps de usuario em C (usam a libc minima) ----
user_c:
	@echo "== Compilando apps C (libc + terminal) =="
	@cd user/libc && $(MAKE) -s
	@cd user/terminal && ./build.sh
	@cd user/calc && ./build.sh
	@cd user/netmon && ./build.sh
	@cd user/vela && ./build.sh
	@python3 tools/mkpulse.py user/terminal/terminal.elf terminal.pulse $(TERMINAL_STACK)
	@python3 tools/mkpulse.py user/calc/calc.elf calc.pulse $(CALC_STACK)
	@python3 tools/mkpulse.py user/netmon/netmon.elf netmon.pulse $(NETMON_STACK)
	@python3 tools/mkpulse.py user/vela/vela.elf vela.pulse $(VELA_STACK)

disk: user user_c
	@echo "== Gerando disk.img (Nebular FileSystem) =="
	@python3 tools/mknblr.py disk.img fsd.pulse shell.pulse editor.pulse ball.pulse terminal.pulse calc.pulse netmon.pulse vela.pulse welcome.txt

run: all
	@echo "== Iniciando o Pulsar OS no QEMU =="
	@cargo run

clean:
	@cargo clean
	@cd user/shell && cargo clean
	@cd user/fsd   && cargo clean
	@cd user/ball  && cargo clean
	@cd user/editor && cargo clean
	@cd user/trespass && cargo clean
	@cd user/libc && $(MAKE) -s clean
	@rm -f user/terminal/terminal.elf user/calc/calc.elf user/netmon/netmon.elf user/vela/vela.elf user/hello_c/hello_c.elf
	@rm -f fsd.pulse shell.pulse editor.pulse ball.pulse trespass.pulse terminal.pulse calc.pulse netmon.pulse vela.pulse hello_c.pulse disk.img
