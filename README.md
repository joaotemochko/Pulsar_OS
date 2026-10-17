# Pulsar OS

A microkernel operating system written from scratch in **Rust**, **C** and assembly, running on **AArch64** and **RISC-V (RV64)**, with **SMP** (multi-core) scheduling and a fast, fully software-rendered desktop. Pulsar aims to combine the ease of use of Windows, the customizability of Linux and the look of macOS.

> **Status:** active development. Both architectures boot under QEMU on up to 8 cores, run isolated user processes with W^X memory protection, and start a complete graphical desktop with ten applications.

---

## Highlights

**Kernel**
- **Two architectures from one codebase** — AArch64 (EL1/EL0, 4-level page tables, GICv2, PSCI) and RISC-V RV64 (S-mode under OpenSBI, Sv39, SBI timer/IPI/HSM). Everything above `kernel/src/arch/` is architecture-independent.
- **SMP** — all cores are started at boot (PSCI `CPU_ON` / SBI `hart_start`); a per-CPU scheduler with an idle loop (`wfi`), wake-up IPIs, and per-core load accounting. User processes run truly in parallel, and the kernel uses **fine-grained per-subsystem locks** (scheduler, memory, disk, GPU, network, input), so cores only contend on what they actually share.
- **Microkernel IPC** — synchronous rendezvous messages (send / receive / reply, plus non-blocking and timed receive), a name server, and user-space servers (filesystem daemon, window manager).
- **Memory** — the kernel manages **4 GiB of RAM** (QEMU runs with `-m 4G`; ARM maps it with 2 MiB blocks, RISC-V with gigapages); each process has a heap that grows on demand (`SBRK`, from `0x10_0000_0000`, up to 4 GiB) and a 256 KiB stack; the C library provides `malloc`/`free`/`calloc`/`realloc`.
- **Isolation** — per-process address spaces with ASIDs, W^X segments, user-pointer validation on every syscall, and fault containment: a crashing or misbehaving program is killed, the rest of the system keeps running. Process memory is fully reclaimed on exit.
- **Drivers** — virtio-gpu (with **hardware cursor**), virtio-blk, virtio-input (keyboard + tablet), virtio-net with an IPv4 stack exposing **TCP and UDP sockets** (64 KiB receive window, flow control, retransmission), virtio-rng (host entropy for TLS), PL031 / goldfish RTC. Application protocols (DNS, HTTP, TLS) live in user space.

**Desktop (Pulsar Desktop)**
- **macOS look** — frosted-glass menu bar and dock, dock magnification, traffic-light window controls, soft shadows, rounded corners, Noto Sans typography with full Portuguese accents, light & dark themes, eight accent colors, six procedural wallpapers.
- **Windows ease** — Alt+Tab, window snapping (drag to the screen edges or Super+arrows), right-click context menus everywhere, a task manager (Ctrl+Shift+Esc) that can end processes, automatic "end task" for apps that stop responding, crash notifications, a power-off confirmation dialog.
- **Linux customization** — the whole environment is described by a plain-text dotfile, `pulsar.cfg`. Change it in *Configurações* or edit it in the Editor: the desktop reloads it live. ABNT2 (Brazilian) and US keyboard layouts, with dead keys for accents.
- **Universal search** (Ctrl+Space or a tap on Super): apps, files, commands and inline math (`12*7`).
- **Control Center**: dark mode, network, per-core CPU load, wallpaper and accent pickers.

**Fast by design**
- The compositor redraws only damaged rectangles, skips windows that are fully covered, and never redraws for mouse movement (hardware cursor).
- Frames are presented outside the kernel lock, so other cores keep serving syscalls while the host copies pixels; the back buffer is the GPU scanout backing (no extra full-frame copy).
- Apps are event-driven and **long-poll** the window manager: a poll is answered only when the app has something new (input, hover, theme/size change) or after 250 ms, so idle apps and the compositor stay near 0% CPU. Commits are acknowledged only after composition (no tearing), and the compositor wakes as soon as a frame arrives.
- Primitives are span-based with cached anti-aliasing masks; shadows are a single distance-field pass; the frosted glass is a pre-blurred copy of the wallpaper.

---

## Applications

| App | Language | Description |
|-----|----------|-------------|
| **Arquivos** (Finder) | Rust (WM) | List and icon views, filters, open/edit, new text document, context menu |
| **Configurações** | Rust (WM) | Appearance, wallpaper, dock & menu bar, keyboard, about |
| **Editor** | Rust | UTF-8 text editor: cursor navigation, line numbers, undo, Ctrl+S |
| **Terminal** | C | Unix-style shell: `ls cat ps kill open date free cpus uname neofetch`… |
| **Calculadora** | C | Decimal fixed-point, pt-BR formatting, full keyboard support |
| **Monitor do Sistema** | Rust | Per-core CPU graphs, memory, process list, end process |
| **Vela** | C | Web browser: **HTTPS** (TLS 1.2, BearSSL, Mozilla roots), **CSS** (selectors, cascade, custom properties, `@media`/`@supports`, flexbox, grid, tables, positioning), **JavaScript** (QuickJS: DOM, events, timers, `fetch`/XHR, ES modules, custom elements), TrueType text, PNG/JPEG/GIF/SVG images, forms, cookies |
| **Rede** | C | Network status and UDP ping |
| **Bolhas** | Rust | Physics demo (fixed 60 Hz timestep), runs on its own core |
| **hello_c** | C | Minimal windowed app — the template for new C apps |

---

## Building and running

### Prerequisites
- Rust **nightly** with `rust-src` (see `rust-toolchain.toml`; `build-std` builds `core` for the targets)
- `clang`, `ld.lld` and `llvm-ar` (LLVM 15+) for the C apps
- `qemu-system-aarch64` and/or `qemu-system-riscv64` (QEMU 8+; RISC-V uses the bundled OpenSBI)
- Python 3 (image tools)

### Run

```sh
make run                    # AArch64, 4 cores
make run ARCH=riscv64       # RISC-V RV64, 4 harts
make run SMP=8              # number of cores (1-8)
make run MEM=8G             # more RAM for QEMU (the kernel uses the first 4 GiB; minimum 4G)
```

`make` (or `make all ARCH=…`) builds the kernel, every app, and the disk image into `build/<arch>/`. `make run-debug` starts QEMU paused with a GDB stub on `:1234`. Press `Ctrl-A X` in the terminal to quit QEMU, or use *Desligar…* in the Pulsar menu.

Vela opens sites by name over HTTPS (e.g. `wikipedia.org`, `news.ycombinator.com`; names resolve through QEMU's DNS at `10.0.2.3`), searches the web when you type words instead of an address, and can open a local server over plain HTTP: run `python3 -m http.server 8080` on the host and open `10.0.2.2:8080/` (`10.0.2.2` is the host in QEMU's user-mode network). See [`user/vela/README.md`](user/vela/README.md).

---

## Keyboard shortcuts

| Shortcut | Action |
|----------|--------|
| Ctrl+Space / tap Super | Search apps, files and commands |
| Alt+Tab (Shift reverses) | Switch windows |
| Ctrl+W / Alt+F4 / Ctrl+Q | Close the active window |
| Super+Up / F11, Super+Down | Maximize / restore, minimize |
| Super+Left / Super+Right | Snap to half of the screen |
| Super+D | Show desktop |
| Super+E / Super+I | Files / Settings |
| Ctrl+Alt+T | Terminal |
| Ctrl+Shift+Esc | System Monitor (task manager) |
| F1 | Full shortcut list |

Double-click a title bar to maximize; drag a window to the top or side edges to snap it; drag the right/bottom/left edges to resize; right-click the desktop, a title bar, a file or a dock icon for context menus.

---

## Configuration (`pulsar.cfg`)

```ini
theme=light            # light | dark
accent=azul            # azul roxo rosa vermelho laranja amarelo verde grafite
wallpaper=aurora       # aurora ondas oceano por-do-sol grafite nebulosa
dock.size=medium       # small | medium | large
dock.magnify=on
dock.position=bottom   # bottom | left
dock.click_minimizes=off  # on: clicking the focused app's dock icon minimizes it (Windows-style)
clock.24h=on
clock.seconds=off
menubar.cpu=on         # per-core load meter in the menu bar
glass=on               # translucent menu bar and dock
keyboard=abnt2         # abnt2 | us
```

The file lives on the Nebular disk. The window manager rewrites it when you change something in *Configurações*, and re-reads it every couple of seconds, so edits saved from the Editor apply immediately.

---

## Architecture

```
            ┌───────────┐ ┌──────────┐ ┌──────────┐ ┌─────────┐
  EL0 / U   │ apps (C / │ │ shell    │ │ fsd      │ │ ...     │   one address space
            │ Rust)     │ │ (WM)     │ │ (FS srv) │ │         │   (ASID) per process
            └─────┬─────┘ └────┬─────┘ └────┬─────┘ └────┬────┘
                  │  IPC (send/recv/reply) + syscalls      │
  ════════════════╪═══════════════════════════════════════╪═══════════
  EL1 / S   process.rs  scheduler (per-CPU), IPC, sleep, kill
            syscall.rs  ABI, user-pointer validation
            loader.rs   .pulse loader (W^X, per-arch check)
            drivers     virtio-gpu/blk/input/net, NBFS/PFS
            arch/       aarch64/ (GIC, PSCI, MMU 4-level)  riscv64/ (SBI, Sv39)
```

### SMP model
There is no big kernel lock. Each subsystem has its own spinlock — `SCHED` (process table, scheduler, IPC), `MMU`, `FRAME` (physical memory), `FS` (filesystem + disk), `INPUT`, `GPU`, `NET`, plus small locks for the service registry, the file staging buffer and the console — with a fixed nesting order documented in `kernel/src/sync.rs`, which rules out deadlocks. The kernel runs with interrupts masked, so a core is never interrupted while holding a lock. One core can read the disk while another presents a frame and a third exchanges IPC messages. Each core runs the round-robin scheduler on its own; a core with nothing to run sleeps in `wfi` and is woken by its timer or by an IPI when another core makes a process runnable. Timers are programmed for the end of the 10 ms quantum or the next sleeping process's deadline, whichever comes first.

### Architecture layer
`kernel/src/arch/mod.rs` defines the contract every backend implements: the trap frame (`Context`), MMU operations, timer, IPIs, CPU start-up, console, RTC and power-off.

| | AArch64 | RISC-V RV64 |
|---|---|---|
| Privilege | EL1 kernel, EL0 user | S-mode kernel (OpenSBI in M-mode), U-mode user |
| Paging | 4 KiB, 4 levels, TTBR0 + 8-bit ASID | Sv39, 3 levels, `satp` ASID |
| Timer / IPI | generic timer (PPI 30) / GICv2 SGI 1 | SBI TIME / SBI IPI |
| CPU start-up | PSCI `CPU_ON` (HVC) | SBI HSM `hart_start` |
| Syscall | `svc #0`, number in `x8` | `ecall`, number in `a7` |
| MMIO | identity-mapped | high-half window (`0xFFFF_FFC0_…`) |
| RAM layout | 4 GiB from `0x4000_0000`; kernel at the start, graphics at `0x4600_0000` | 4 GiB from `0x8000_0000`; kernel `0x8020_0000`, graphics `0x8600_0000` |

User programs are linked at the same virtual addresses on both architectures and the shared graphics region is mapped at the same user address (`0x4600_0000`), so only the syscall instruction differs between the two builds. The `.pulse` header records the target architecture and the loader refuses foreign binaries.

### The `.pulse` executable format

```
[ PulseHeader ][ PulseSegment[] ][ segment data... ]
```

| Header field | Size | Description |
|---|---|---|
| `magic` | u32 | `"PULS"` (0x534C5550) |
| `version` | u16 | 2 |
| `seg_count` | u16 | number of segments |
| `entry` | u64 | entry point |
| `seg_table_off` | u32 | offset of the segment table |
| `arch` | u32 | 1 = AArch64, 2 = RISC-V RV64 (0 = legacy AArch64) |
| `stack_top` | u64 | top of the user stack (64 KiB mapped below it) |

Each segment carries `file_off`, `file_size`, `vaddr`, `mem_size` and R/W/X `flags`; executable segments are mapped read-only + executable and writable ones non-executable (**W^X**). `tools/mkpulse.py` converts an ELF into a `.pulse`.

### Graphics stack
- **Kernel**: virtio-gpu 2D resource backed by the back buffer; `FB_PRESENT` transfers and flushes a rectangle; a 64×64 ARGB resource on the cursor queue provides the hardware cursor.
- **Shared region** (64 MiB): back buffer, wallpaper, blurred wallpaper, and eight 4 MiB app surfaces (stride 1280).
- **Window manager** (`user/shell`): owns the screen, composes windows, menu bar, dock and overlays with damage tracking and occlusion culling, and talks to apps over IPC: `HELLO` (register, get a surface), `POLL` (events: mouse, keys, menu actions, theme, size), `COMMIT` (frame ready, answered after composition), `TITLE`.
- **Toolkits**: `user/plib` (Rust) and `user/libc` (C) provide the same primitives, fonts, theme and `App` client.

### Selected syscalls
`IPC_SEND/RECV/REPLY/TRY_RECV/RECV_TIMEOUT`, `SPAWN`, `EXIT`, `KILL`, `SLEEP`, `TIME_MS`, `RTC`, `SYSINFO`, `PROC_INFO`, `CPU_LOAD`, `FB_MAP/FB_PRESENT`, `SURF_MAP`, `CURSOR_SET/CURSOR_MOVE`, `INPUT_POLL`, `FS_COUNT/STAT/READ/WRITE/CREATE`, `NET_STATUS`, `TCP_CONNECT/SEND/RECV/STATUS/CLOSE`, `UDP_SEND/UDP_RECV`, `SBRK` (per-process heap), `GETRANDOM` (virtio-rng), `THREAD_CREATE`/`EXIT_GROUP` (threads sharing the address space, scheduled on any core), `FUTEX_WAIT/FUTEX_WAKE`, `POWEROFF`. The full numbered list is documented at the top of `kernel/src/syscall.rs`.

---

## Project layout

```
.
├── Makefile                     # build/run for ARCH=aarch64|riscv64
├── pulsar.cfg, welcome.txt      # files placed on the disk image
├── kernel/src/
│   ├── main.rs                  # boot sequence (boot CPU + secondaries)
│   ├── arch/{mod.rs, aarch64/, riscv64/}
│   ├── process.rs               # SMP scheduler, IPC, sleep, kill
│   ├── syscall.rs, trap.rs, sync.rs, time.rs
│   ├── loader.rs, pulse.rs, frame_allocator.rs
│   ├── gpu.rs, fb.rs, input.rs, blk.rs, net.rs, rng.rs, virtio.rs
│   └── fs.rs, nbfs.rs, pfs.rs   # Nebular FS / legacy PulsarFS
├── user/
│   ├── plib/                    # Rust userspace library (gfx, text, icons, theme, App)
│   ├── libc/                    # C userspace library (same feature set)
│   ├── shell/                   # Pulsar Desktop (compositor + window manager)
│   ├── fsd/                     # filesystem daemon (IPC server)
│   ├── editor/ ball/ sysmon/ trespass/      # Rust apps
│   ├── terminal/ calc/ netmon/ vela/ hello_c/   # C apps
│   └── third_party/             # BearSSL, QuickJS, stb, NanoSVG, fonts (see its README)
└── tools/
    ├── mkpulse.py               # ELF -> .pulse
    ├── mknblr.py / mkpfs.py     # disk image builders
    ├── genfonts.py              # font atlases (Rust + C)
    └── bin2c.py                 # embeds binary files (fonts, prelude.js) in C sources
```

---

## Roadmap

- Interrupt-driven virtio (block, input, network) instead of polling.
- A real VFS with directories; move more drivers into user-space servers.
- Vela: TLS 1.3, HTTP keep-alive, WebP images; shared clipboard between apps.
- Dynamic linking, ASLR, and running on real hardware (Raspberry Pi, VisionFive 2).

---

## License

Pulsar OS is released under the **MIT License**. See [`LICENSE`](LICENSE).

Copyright (c) 2026 João Antônio Temochko Andre.

The bitmap font atlases in `user/plib/src/fonts.rs` and `user/libc/src/font_*.c` are generated from **Noto Sans** and **Noto Sans Mono** (Google), licensed under the SIL Open Font License 1.1.

Vela bundles third-party code under its own permissive licenses — BearSSL (MIT), QuickJS (MIT), stb_truetype / stb_image / stb_sprintf (public domain / MIT), NanoSVG (zlib), Noto (OFL 1.1) and DejaVu (Bitstream Vera) font subsets, and the Mozilla CA list. Details and local patches are in [`user/third_party/README.md`](user/third_party/README.md).

## Acknowledgements

ARM Architecture Reference Manual (ARMv8-A), the RISC-V Privileged Specification and SBI specification, the virtio 1.1 specification, the `aarch64-cpu` crate, OpenSBI, and the canonical MMU bring-up sequence of the Linux `arch/arm64` boot path.
