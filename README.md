# Pulsar OS

A microkernel operating system written from scratch in **Rust** and **AArch64 assembly**, targeting the ARM64 architecture. Pulsar OS aims to combine the ease of use of a Windows-like experience with the control and customization of a Linux-like system, built on a clean microkernel foundation.

> **Status:** Early-stage / active development. The kernel boots on AArch64, brings up virtual memory, enforces user/kernel isolation, and can load and execute programs in its own native executable format (`.pulse`).

---

## Overview

Pulsar OS is an experimental, education-driven kernel built around a few core ideas:

- **Microkernel architecture** — keep the privileged core minimal; push drivers and services toward user space (EL0), communicating through message-based IPC.
- **Native `.pulse` executable format** — instead of emulating foreign binary formats, Pulsar defines its own AArch64-native executable format, loaded and executed directly by the kernel.
- **Hybrid IPC** — a message-passing model inspired by the Windows NT design.
- **Hardware-enforced isolation** — user programs run in EL0 with their own translation mappings and W^X (Write XOR Execute) memory protection.

The project is developed and tested under **QEMU** emulating the `virt` machine with a Cortex-A53 CPU.

---

## Current Features

The following subsystems are implemented and verified:

- **AArch64 boot sequence** — secondary cores parked, BSS cleared, stack set up, jump into Rust.
- **UART driver (PL011)** — serial output for logging and program I/O.
- **Exception vector table** — full 16-entry table installed at `VBAR_EL1`, with synchronous, IRQ, FIQ, and SError handlers.
- **Memory Management Unit (MMU)** — 4-level translation tables (L0→L1→L2→L3) with 4 KiB pages, `MAIR_EL1` / `TCR_EL1` / `TTBR0_EL1` configuration, and identity mapping for the kernel.
- **Dynamic page mapping** — `map_page(va, pa, flags)` builds intermediate translation tables on demand, backed by a physical frame allocator.
- **Physical frame allocator** — bitmap-based allocator handing out 4 KiB frames on demand.
- **EL1 → EL0 transition** — controlled drop into user mode via `ERET`, with a separate user stack.
- **System call ABI** — `svc`-based syscalls with arguments in `x0`–`x2`, syscall number in `x8`, and return value in `x0`.
- **EL0 fault handling** — synchronous EL0 exceptions are classified (syscall vs. fault) and reported.
- **`.pulse` loader** — parses the `.pulse` header, allocates frames, copies segments, maps them with per-segment permissions, and jumps to the entry point in EL0.
- **W^X enforcement** — code pages are mapped read-only + executable; data pages are writable + non-executable. Verified by an adversarial write test.

---

## The `.pulse` Executable Format

Pulsar defines its own native binary format rather than supporting foreign formats such as PE (`.exe`) or ELF directly. A `.pulse` file is a compact container:

```
[ PulseHeader ][ PulseSegment[] ][ segment data... ]
```

**Header**

| Field            | Size | Description                                  |
|------------------|------|----------------------------------------------|
| `magic`          | u32  | `"PULS"` (0x534C5550)                         |
| `version`        | u16  | Format version                               |
| `seg_count`      | u16  | Number of segments                           |
| `entry`          | u64  | Virtual address of the entry point           |
| `seg_table_off`  | u32  | Offset to the segment table                  |
| `_reserved`      | u32  | Reserved                                     |

**Segment**

| Field       | Size | Description                                        |
|-------------|------|----------------------------------------------------|
| `file_off`  | u32  | Offset of the segment bytes within the file        |
| `file_size` | u32  | Number of bytes to copy from the file              |
| `vaddr`     | u64  | Destination virtual address                        |
| `mem_size`  | u32  | Size in memory (`>= file_size`; remainder zeroed)  |
| `flags`     | u32  | Permission bits: R (1), W (2), X (4)               |

The loader honors the segment permission flags, mapping executable segments as read-only + executable and writable segments as non-executable, enforcing **W^X** at the hardware level.

---

## Architecture Notes

- **Exception Levels:** Pulsar uses AArch64 Exception Levels rather than x86-style rings. The kernel runs at **EL1**; user programs run at **EL0**. (EL2/EL3 are not used.)
- **Microkernel vs. compile-time layout:** Cargo workspaces are used for code organization, but the isolation boundary is enforced at runtime by the MMU (separate address mappings per privilege level), not by how crates are split.
- **Target machine:** QEMU `virt` is used during development for its stable, documented memory map and its clean EL1 entry, avoiding hardware-specific quirks during bring-up.

---

## Building and Running

### Prerequisites

- Rust **nightly** toolchain (see `rust-toolchain.toml`)
- The `aarch64-unknown-none` target
- `qemu-system-aarch64`
- (Optional, for debugging) `gdb-multiarch` or an AArch64-capable GDB/LLDB

The toolchain components and target are declared in `rust-toolchain.toml` and installed automatically by `rustup`.

### Build

```sh
cargo build
```

### Run in QEMU

```sh
cargo run
```

or, using the provided Makefile:

```sh
make run
```

This launches the kernel under QEMU with:

```
qemu-system-aarch64 -machine virt -cpu cortex-a53 -nographic -kernel <kernel binary>
```

To exit QEMU: press `Ctrl-A` then `X`.

### Debugging

Start QEMU paused with a GDB stub:

```sh
qemu-system-aarch64 -machine virt -cpu cortex-a53 -nographic -kernel target/aarch64-unknown-none/debug/pulsar_kernel -S -s
```

Then attach a debugger in another terminal:

```sh
gdb-multiarch target/aarch64-unknown-none/debug/pulsar_kernel
(gdb) target remote :1234
```

The QEMU monitor (`Ctrl-A` then `C`) also exposes `info registers`, useful for inspecting system registers such as `SCTLR_EL1`, `ESR_EL1`, and `TCR_EL1` during low-level debugging.

---

## Project Layout

```
.
├── Cargo.toml                       # Workspace manifest
├── rust-toolchain.toml              # Pinned nightly toolchain + target
├── .cargo/config.toml               # Build target and QEMU runner
├── Makefile                         # build / run / clean helpers
└── kernel/
    ├── Cargo.toml
    └── src/
        ├── main.rs                  # Kernel entry, boot orchestration
        ├── uart.rs                  # PL011 UART driver
        ├── cpu.rs                   # System register access, EL helpers, EL0 entry
        ├── mmu.rs                   # MMU setup, dynamic page mapping, page flags
        ├── frame_allocator.rs       # Bitmap physical frame allocator
        ├── exceptions.rs            # Generic exception reporting
        ├── syscall.rs               # Syscall dispatch + EL0 fault classification
        ├── pulse.rs                 # .pulse format definitions
        ├── loader.rs                # .pulse loader
        ├── user.rs                  # Embedded user program / .pulse image
        └── arch/aarch64/
            ├── boot.S               # Early boot (core parking, BSS, stack)
            ├── vectors.S            # Exception vector table
            └── linker.ld            # Memory layout
```

---

## Roadmap

Planned next steps, roughly in dependency order:

1. **Process model** — a `Process` abstraction (entry, stack, saved register state, address space), the ability to load more than one program, and a simple scheduler.
2. **Process-aware fault handling** — terminate a faulting process rather than halting the kernel.
3. **IPC** — a message/port mechanism for inter-process communication, inspired by NT's LPC/ALPC.
4. **Storage driver** — block-level access to a virtual disk under QEMU.
5. **Filesystem server + VFS** — a user-space filesystem service reachable through IPC.
6. **User space** — a shell and basic utilities, all as `.pulse` programs.

Longer-term goals include multi-segment `.pulse` support, a `.pulse` build tool, dynamic linking, and ASLR.

---

## License

Pulsar OS is released under the **MIT License**. See [`LICENSE`](LICENSE) for details.

Copyright (c) 2026 João Antônio Temochko Andre.

---

## Acknowledgements

Low-level bring-up references the ARM Architecture Reference Manual (ARMv8-A), the AArch64 memory management documentation, and the `aarch64-cpu` crate for typed system-register access. The MMU activation sequence draws on the canonical ordering used by the Linux `arch/arm64` boot path.