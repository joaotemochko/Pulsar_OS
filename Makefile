.PHONY: all build run clean

all: build

build:
	@echo "Compilando o Pulsar Kernel para AArch64..."
	@cargo build

run:
	@echo "Iniciando o Pulsar OS no QEMU..."
	@cargo run

clean:
	@echo "Limpando os binários gerados..."
	@cargo clean