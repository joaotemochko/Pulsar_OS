# Pulsar OS

A microkernel operating system written from scratch in **Rust**, **C** and assembly, running on **AArch64** and **RISC-V (RV64)**, with **SMP** (multi-core) scheduling and a fast, fully software-rendered desktop. Pulsar aims to combine the ease of use of Windows, the customizability of Linux and the look of macOS.

> **Status:** active development. Both architectures boot under QEMU on up to 8 cores, run isolated user processes with W^X memory protection, and start a complete graphical desktop with ten applications.

---

## Highlights

**Kernel**
- **Two architectures from one codebase** — AArch64 (EL1/EL0, 4-level page tables, GICv2, PSCI) and RISC-V RV64 (S-mode under OpenSBI, Sv39, SBI timer/IPI/HSM). Everything above `kernel/src/arch/` is architecture-independent.
- **SMP** — all cores are started at boot (PSCI `CPU_ON` / SBI `hart_start`); a per-CPU scheduler with an idle loop (`wfi`), wake-up IPIs, and per-core load accounting. User processes run truly in parallel, and the kernel uses **fine-grained per-subsystem locks** (scheduler, memory, disk, GPU, network, input), so cores only contend on what they actually share.
- **Microkernel IPC** — synchronous rendezvous messages (send / receive / reply, plus non-blocking and timed receive), a name server, and user-space servers (filesystem daemon, window manager).
- **Memory** — the kernel manages **4 GiB of RAM** (QEMU runs with `-m 4G`; ARM maps it with 2 MiB blocks, RISC-V with gigapages); each process has a heap that grows on demand (`SBRK`, from `0x10_0000_0000`, up to 4 GiB) and **`mmap`/`munmap`/`mprotect`** over a per-process VMA table: pages are created lazily on first touch, so a program can reserve gigabytes with `PROT_NONE` (as JavaScript engines do) at no cost.
- **Floating point** — apps are built hard-float (NEON / RV64GC); the kernel stays soft-float and switches each process's FP/SIMD registers only when a core changes owner (RISC-V skips the save when `sstatus.FS` is clean).
- **Filesystem** — **ext2** with directories, symbolic links, files up to terabytes (single/double/triple indirect blocks) and a write-back block cache with scatter-gather I/O; the image is built by the host's `mke2fs -d` and passes `e2fsck`. POSIX file descriptors (`open/read/write/lseek/pread/getdents/rename/unlink/symlink`…) and pipes sit on top; the old indexed API still works on the files of `/`.
- **Linux personality** — **ELF** binaries, static or **dynamically linked** (`ld-musl` + shared libraries, `dlopen`), run with the Linux system-call numbers and structures (the generic table is the same on aarch64 and riscv64): **unmodified musl libc** works out of the box, with `fork`, `vfork`, `execve`, `posix_spawn`, `waitpid`, threads (`clone`, futexes, TLS), **signals with handlers** (Linux `ucontext` layout, `sigaltstack`, `setitimer`), pipes, `poll`/`select`/**`epoll`**/`eventfd`, **TCP/UDP sockets** (DNS through musl's own resolver) and loopback TCP servers, **`AF_UNIX` sockets** (paths, abstract names, `SOCK_SEQPACKET`, descriptor passing with `SCM_RIGHTS`), **shared memory** (`memfd_create`, `shm_open`, `MAP_SHARED`), `/dev` and `/proc`. `tools/elf2pulse.py` turns a static Linux binary into a native `.pulse`. Type a program's name in the Terminal to run it from `/bin`. Graphical Linux programs speak **Wayland** to a Pulsar compatibility service and show up as ordinary Pulsar windows.
- **Isolation** — per-process address spaces with ASIDs, W^X segments, user-pointer validation on every syscall, and fault containment: a crashing or misbehaving program is killed, the rest of the system keeps running. Process memory is fully reclaimed on exit.
- **No blue screen** — a bug inside the kernel while serving a process (a Rust `panic`, or a CPU exception in kernel mode) is an *oops*, not a crash: the CPU releases the locks it held, only that process dies (as with `SIGSEGV`), and the CPU goes back to the scheduler on a clean stack. System services — filesystem daemon, window manager, Wayland bridge — are ordinary processes started and **restarted by `init`** from `/etc/services`, and apps reconnect by themselves when the window manager comes back.
- **Drivers** — virtio-gpu (with **hardware cursor**), virtio-blk, virtio-input (keyboard + tablet), virtio-net with an IPv4 stack exposing **TCP and UDP sockets** (64 KiB receive window, flow control, retransmission), virtio-rng (host entropy for TLS), PL031 / goldfish RTC. Application protocols (DNS, HTTP, TLS) live in user space.

**Desktop (Pulsar Desktop)**
- **macOS look** — frosted-glass menu bar and dock, dock magnification, traffic-light window controls, soft shadows, rounded corners, Noto Sans typography with full Portuguese accents, light & dark themes, eight accent colors, six procedural wallpapers.
- **Windows ease** — Alt+Tab, window snapping (drag to the screen edges or Super+arrows), right-click context menus everywhere, a task manager (Ctrl+Shift+Esc) that can end processes, automatic "end task" for apps that stop responding, crash notifications, a power-off confirmation dialog.
- **Linux customization** — the whole environment is described by a plain-text dotfile, `pulsar.cfg`. Change it in *Configurações* or edit it in the Editor: the desktop reloads it live. Four keyboard layouts — **ABNT**, **ABNT2** (Brazil), **US** and **US-International** — with AltGr and dead keys, identical for native apps and Linux programs.
- **Universal search** (Ctrl+Space or a tap on Super): apps, files, commands and inline math (`12*7`).
- **Control Center**: dark mode, network, per-core CPU load, wallpaper and accent pickers.

**Fast by design**
- The compositor redraws only damaged rectangles, skips windows that are fully covered, and never redraws for mouse movement (hardware cursor). Damage is split around the topmost opaque window, so dragging a window does not repaint what lies underneath it. Apps can declare which part of their surface changed (`COMMIT` with a damage rectangle: `app_commit_rect` in C, `App::commit_rect` in Rust, and `wl_surface.damage` for Wayland clients). The Terminal's blinking caret, for example, recomposes one line instead of the whole window.
- The drawing primitives avoid the slow paths of a `no_std` build. Pixel copies use aligned 64-bit words (NEON on AArch64) instead of `compiler_builtins`' memcpy, which falls back to byte loops for every odd x. Alpha blending mixes two pixels per 64-bit multiply, and soft shadows use a lookup table and constant-alpha spans. Dock icons and their shadows are cached as premultiplied sprites. Measured under QEMU, composing a typical interaction frame dropped from ~31 ms to ~8–12 ms. Set `PERF_LOG` in `user/shell/src/main.rs` to print per-phase timings and a primitive benchmark on the serial console.
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
| **Terminal** | C | Unix-style shell: `ls cat ps kill open date free cpus uname neofetch`…; any other name runs `/bin/<name>` with its output in the window (Ctrl+C stops it) |
| **Calculadora** | C | Decimal fixed-point, pt-BR formatting, full keyboard support |
| **Monitor do Sistema** | Rust | Per-core CPU graphs, memory, process list, end process |
| **Vela** | C | Web browser: **HTTPS** (TLS 1.2, BearSSL, Mozilla roots), **CSS** (selectors, cascade, custom properties, `@media`/`@supports`, flexbox, grid, tables, positioning), **JavaScript** (QuickJS: DOM, events, timers, `fetch`/XHR, ES modules, custom elements), TrueType text, PNG/JPEG/GIF/SVG images, forms, cookies |
| **Rede** | C | Network status and UDP ping |
| **Bolhas** | Rust | Physics demo (fixed 60 Hz timestep), runs on its own core |
| **hello_c** | C | Minimal windowed app — the template for new C apps |
| **wlkeys** | C (musl) | A Wayland program built on `libwayland-client` + `libxkbcommon`: types text using the system layout, with dead keys |
| **wlhello** | C (musl) | A Wayland program (raw protocol, `memfd` buffers): an animated window driven through the compatibility service |
| **posixtest**, **spawntest**, **sigtest**, **iotest**, **unixtest**, **dyntest**, **oopstest**, **svctest**, **wltest** | C (musl) | Linux programs in `/bin` (also as `.pulse`) that exercise the POSIX layer — run them from the Terminal |

---

## Building and running

### Prerequisites
- Rust **nightly** with `rust-src` (see `rust-toolchain.toml`; `build-std` builds `core` for the targets)
- `clang`, `ld.lld` and `llvm-ar` (LLVM 15+) for the C apps and musl
- `mke2fs` (e2fsprogs 1.43+) to build the ext2 disk image (`make DISKFS=nbfs` builds the old flat image without it)
- `qemu-system-aarch64` and/or `qemu-system-riscv64` (QEMU 8+; RISC-V uses the bundled OpenSBI)
- Python 3 (image tools)
- For the Linux compatibility libraries:
  - `cmake`, `meson` (1.4+) and `ninja`;
  - `pkg-config`, `bison`, `gperf`;
  - the host glib tools (`glib-compile-resources`, `glib-mkenums`, `glib-genmarshal`), used while building gdk-pixbuf and, later, GTK;
  - `gcc` + `libexpat` for the host `wayland-scanner`.

  Meson is pure Python; running it from its release tarball is enough.

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
keyboard=abnt2         # abnt | abnt2 | us | us-intl
```

The file lives at `/pulsar.cfg` on the disk. The window manager rewrites it when you change something in *Configurações*, and re-reads it every couple of seconds, so edits saved from the Editor apply immediately.

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
            syscall.rs  Pulsar ABI, user-pointer validation
            linux.rs    Linux ABI (ELF processes)  ->  file.rs vm.rs exec.rs
            ext2.rs     filesystem + block cache   file.rs  descriptors, pipes
            vm.rs       mmap/mprotect, lazy pages  exec.rs  ELF/.pulse, argv/auxv
            drivers     virtio-gpu/blk/input/net
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

### Linux personality and the POSIX layer
Every process has an ABI. Native `.pulse` apps use Pulsar's own syscalls; ELF binaries (and `.pulse` v3 files converted from them) use the **Linux** ones, dispatched by `kernel/src/linux.rs` to the same subsystems. Nothing in musl is patched: `make` compiles `user/third_party/musl` for `<arch>-linux-musl` (static `libc.a` and the shared `libc.so`, which is also the dynamic linker `/lib/ld-musl-<arch>.so.1`) into `build/<arch>/sysroot`; `user/linux/*.c` are linked statically, `user/linux/dynamic/*.c` dynamically against the libraries of `user/linux/lib/`, and everything lands in `/bin` and `/lib`.

- **Processes** — the ELF loader (`exec.rs`) builds the Linux initial stack (`argc`, `argv`, `envp`, auxv with `AT_PHDR`, `AT_RANDOM`, `AT_HWCAP`…); the 8 MiB stack is a lazy VMA. `fork` copies the address space, `vfork`/`posix_spawn` borrow the parent's until `execve`, and `wait4` returns Linux-style status words. Threads are `clone(CLONE_THREAD)` with their own TLS register (`TPIDR_EL0` is now part of the trap frame) and `CLONE_CHILD_CLEARTID` futex wake-up, which is how `pthread_join` works.
- **Dynamic linking** — for a binary with `PT_INTERP` the kernel also loads the interpreter (at `0x3000_0000`; PIE programs go to `0x1000_0000`) and hands over `AT_BASE`/`AT_ENTRY`/`AT_PHDR`; musl's `ld.so` maps the libraries itself with `mmap` of files. Loaded segments are registered as VMAs, so `mprotect` (RELRO) works on them.
- **Files** — descriptors live per process, are inherited by children, and honour `O_CLOEXEC`; relative paths resolve from the current directory or a directory descriptor (`openat` family), and `getcwd` walks `..` back to `/`. `/dev/{null,zero,urandom,tty}`, `/dev/shm`, `/proc/{cpuinfo,meminfo,version}` and `/proc/self/{exe,maps,cmdline,status,stat}` are synthesized by the kernel. Blocking calls (pipes, eventfd, sockets, `poll`/`epoll`) put the caller to sleep and **re-execute the syscall** when something changes, so no kernel stack is ever parked; the original timeout survives the re-execution.
- **Signals** — per-process actions, per-thread masks and pending sets, `rt_sigframe` built with the real Linux layout (registers + FP/SIMD in `uc_mcontext`), so handlers can inspect or rewrite the context — the trick JavaScript engines use for guard pages. On RISC-V, where Linux returns through the vDSO, Pulsar maps a one-page trampoline. CPU faults become `SIGSEGV`/`SIGBUS`/`SIGILL`/`SIGTRAP` with `si_addr`; `SA_RESTART`, `sigaltstack`, `sigsuspend`, `SIGCHLD`, `SIGPIPE`, `setitimer` are supported.
- **Network** — `AF_INET` stream and datagram sockets over the kernel TCP/UDP stack (blocking and non-blocking `connect`, `SO_ERROR`, `sendmsg`/`recvmsg`). `/etc/resolv.conf` points musl's resolver at QEMU's DNS. The NIC is polled, so socket waits re-check every 10 ms. Connections to `127.0.0.1` (with `listen`/`accept` on the other side) never touch the NIC: they use the same local channels as `AF_UNIX`.
- **Local IPC** (`kernel/src/unix.rs`) — `AF_UNIX` stream and `SOCK_SEQPACKET` sockets: `socketpair`, `bind` to a path (creates a socket inode, so `stat`/`unlink` work) or to an abstract name, `listen`/`accept4`/`connect`, `shutdown`, and descriptor passing with `SCM_RIGHTS` — the descriptors travel attached to the message and are installed in the receiving process.
- **Shared memory** (`kernel/src/shm.rs`) — memory objects with pages created on demand, referenced by descriptors and by VMAs: `memfd_create`, `shm_open`/`shm_unlink` (`/dev/shm`), `MAP_SHARED` of those descriptors and `MAP_SHARED | MAP_ANONYMOUS` (shared across `fork`). Their page-table entries carry a software bit, so tearing down an address space never frees them and `fork` shares them instead of copying; the pages return to the allocator when the last reference goes.
- **`.pulse` conversion** — `tools/elf2pulse.py` repackages a static Linux ELF as a `.pulse` v3 file (segments + program-header location + "Linux ABI" flag); the kernel loads it exactly like the ELF. `make` installs a converted copy of each program as `/bin/<name>.pulse`.
- **Automation** — if the disk has `/etc/autorun`, each line (program + arguments) is started at boot with its output on the serial console; the test images use it.
- **Not yet** — inbound connections from outside the VM (listening works on loopback only), `AF_UNIX` datagram sockets with addresses, `MAP_SHARED` writes reaching regular files (a shared map of an ext2 file is a private copy), `epoll` edge-triggered semantics (reported level-triggered), ptrace; see the Roadmap.

### Fault tolerance (microkernel)
Pulsar is the operating system; Linux support is a compatibility layer on top of it. Pieces are isolated so that one failing does not take the others down:

- **Kernel oops** (`main.rs` panic handler, `sync.rs`) — every spinlock records the CPU holding it. On a `panic` or a kernel-mode exception the CPU prints the diagnostics, releases its locks, kills the process it was serving (wait status `SIGSEGV`) and re-enters the scheduler. Only a fault before the scheduler exists, or a second fault during recovery, halts the machine. `SYS_DEBUG_OOPS` (99) provokes each kind of fault in the caller for testing (`oopstest`).
- **`init`** (`user/init`) — the first process. It starts the services listed in `/etc/services` in order, waits on them and restarts whichever dies; a service that dies more than 5 times in 30 s is given up on instead of looping. Without `init.pulse` on the disk the kernel starts `fsd` and the shell directly, as before.
- **Reconnecting apps** — `libpulsar` (C) and `plib` (Rust) notice when the window manager is gone, wait for the new one, register the window again and ask the app to redraw: a crash or restart of the WM does not close the apps (`svctest`, run from the Terminal, kills the WM and the filesystem daemon under the Terminal's feet).
- **Wayland compatibility service** — `wlsrv` only listens on `/run/wayland-0` and, for each connection, starts a dedicated `wlclient` translator. A client that breaks the protocol gets `wl_display.error` and loses only its own translator; killing `wlsrv` leaves existing connections alive while `init` restarts it (`wltest`).

### Wayland compatibility service
`wlclient` (native Pulsar C) speaks the Wayland wire protocol on one side and the window manager's own protocol (`HELLO`/`POLL`/`COMMIT`/`TITLE`/`BYE`) on the other, so the WM knows nothing about Wayland. It implements:

- `wl_compositor` 4 and `wl_subcompositor` 1;
- `wl_shm` 1 (ARGB/XRGB);
- `xdg_wm_base` 2: `xdg_surface` with window geometry, `xdg_toplevel`, `xdg_popup` and `xdg_positioner`;
- `wl_seat` 5 (pointer and keyboard);
- `wl_output` 2;
- frame callbacks.

Only the region a client declares with `wl_surface.damage` is copied and recomposed.

The WM still sees one surface per window, so `wlclient` composes the scene itself: the toplevel, then subsurfaces, then popups, blended with alpha. Popups are kept inside the window. The positioner flips and slides them as the client allows, the way menus near a screen edge behave. Mouse, wheel and keys come back as Wayland events for the surface under the pointer (Pulsar key codes already are evdev codes). A grabbing popup gets the keyboard, and clicking outside it closes it (`popup_done`). The keyboard is announced with a real **XKB keymap** for the layout chosen in `pulsar.cfg` (sent again if you switch layouts while the program runs), and modifiers follow the XKB convention (AltGr = Mod5), so toolkits using `libxkbcommon` translate keys and compose dead keys themselves. One window (the first `xdg_toplevel`) per program. Popups cannot extend beyond their window yet.

`make` also builds the libraries a toolkit needs, from the original sources in `user/third_party` and without meson (`tools/build_linux_libs.sh`): **libffi**, **libwayland-client** (with a host `wayland-scanner`), the **xdg-shell** protocol from wayland-protocols, and **libxkbcommon** with compose. `wlkeys` is a client written against them.

### Keyboard layouts
`tools/genkeymaps.py` is the single source for the four layouts. From one table per layout (base, Shift, AltGr, AltGr+Shift for each key) it generates the native tables used by the window manager (`user/plib/src/keymaps.rs`), the XKB keymaps handed to Wayland programs (`/usr/share/pulsar/xkb/<layout>.xkb`, validated with the host's `xkbcomp`) and the dead-key table in XCompose format (`/usr/share/pulsar/Compose`, found by programs through `XCOMPOSEFILE`). Native apps and Linux programs therefore type exactly the same text:

| Layout | AltGr | Dead keys | Notes |
|---|---|---|---|
| `abnt2` | ¹²³£¢¬§ª º, AltGr+Q/W/E = / ? ° | ´ \` ~ ^ ¨ | the extra `/?` key next to the right Shift |
| `abnt` | same as ABNT2 | same | without the extra key (use AltGr+Q/W) |
| `us` | — (right Alt is Alt) | — | |
| `us-intl` | á é í ó ú ñ ç ü ß ¿ ¡ € … | ' \` ~ ^ " | ' + c = ç, as Brazilians expect |

### Fonts for Linux programs
Linux programs find and draw fonts the same way they do on Linux, with **fontconfig 2.15** and **FreeType 2.13** (plus **expat** for the XML configuration).

**Build.** `tools/build_font_libs.sh` builds all three from unmodified sources by running their own `configure` scripts. `tools/musl-cc.sh` stands in for a cross compiler: it compiles against the musl sysroot and links static executables.

**What ends up on the disk:**

- The fonts live in `/usr/share/fonts/pulsar`. They are complete fonts with their OpenType layout tables, which harfbuzz needs for kerning, ligatures and complex scripts. They come from `user/third_party/fonts-linux`:
  - Noto Sans, Serif and Sans Mono;
  - Liberation Sans, Serif and Mono;
  - DejaVu Sans;
  - Noto for Arabic, Hebrew and Devanagari.

  The Vela browser keeps its own trimmed subsets.
- The configuration lives in `/etc/fonts` (the stock `fonts.conf` and `conf.d`).
- The cache lives in `/var/cache/fontconfig`.
- The `fc-list`, `fc-match`, `fc-cache` (and other `fc-*`) tools are in `/bin`.

**Pulsar rules.** `/etc/fonts/local.conf` (from `rootfs/`) sets the defaults:

| Requested name | Font used |
|---|---|
| `sans-serif`, `system-ui` | Noto Sans |
| `serif` | Noto Serif |
| `monospace` | Noto Sans Mono |
| Arial, Helvetica, Times New Roman, Courier New | Liberation Sans, Serif and Mono (same metrics, so layouts don't shift; these come from fontconfig's stock `30-metric-aliases.conf`) |

DejaVu Sans is the fallback for symbols.

`fonttest` checks the whole chain:

- every name above resolves to the expected font;
- a character that only DejaVu has is found by coverage;
- FreeType renders the text into a Wayland window.

Portions of this software are copyright © The FreeType Project (www.freetype.org). All rights reserved.

### C++, text shaping and 2D drawing for Linux programs
**C++.** Linux programs get a complete C++ standard library: **libc++, libc++abi and libunwind** from LLVM 18.1.3, the same version as the clang that builds them.

- `tools/build_cxx_runtime.sh` runs LLVM's own CMake build of the runtimes for `<arch>-linux-musl`.
- The result is a static `libc++.a` that already contains the ABI library and the unwinder.
- Exceptions (including inside threads), RTTI, iostreams, `std::thread`, mutexes and condition variables all work.
- The kernel's Linux UAPI headers (`linux/*.h`, `asm/*.h`) come from Alpine's `linux-headers` 6.6 package and are installed into the sysroot. libc++ needs them, and so will glib and Firefox.
- C and C++ programs are linked with `--eh-frame-hdr`; without it the unwinder finds no tables and every `throw` ends in `std::terminate`.

**Drawing and text.** `tools/build_draw_libs.sh` builds, from unmodified sources and with each project's own build system:

| Library | Version | Built with | Purpose |
|---|---|---|---|
| zlib | 1.3.1 | configure | compression |
| libpng | 1.6.44 | configure | PNG images |
| pixman | 0.44.2 | meson | pixel rasterization |
| **harfbuzz** | 10.1.0 | meson | text shaping |
| **cairo** | 1.18.2 | meson | 2D vector drawing, with FreeType, fontconfig, PNG, PDF, SVG and PS output |

Meson runs as a normal cross build. The script writes a cross file per architecture, and a `pkg-config --static` wrapper that only sees the sysroot.

The clang driver links Linux programs with `-lgcc`, `-lgcc_eh` and `crtbegin`/`crtend`. In the sysroot these are the Rust compiler-builtins and empty objects: musl runs constructors through `.init_array`, and the unwinder is inside `libc++.a`.

**Tests.**

- `cxxtest` checks the C++ runtime.
- `drawtest` checks the drawing stack:
  - harfbuzz composes `e` + a combining accent into `é`;
  - harfbuzz kerns `AVATAR` with the font's GPOS table;
  - harfbuzz shapes Arabic right-to-left with joining forms;
  - harfbuzz reorders the Devanagari vowel *i* before its consonant;
  - cairo draws gradients, curves, translucency, fontconfig text and the shaped glyphs;
  - cairo writes and re-reads a PNG and writes a PDF;
  - the result is shown in a Wayland window.

### The GTK foundation: glib, pango, gdk-pixbuf
`tools/build_glib_libs.sh` builds, with each project's own build system:

| Library | Version | Built with | Purpose |
|---|---|---|---|
| pcre2 | 10.44 | configure | regular expressions for `GRegex` |
| glib | 2.82.4 | meson | glib, gobject, gio, gmodule |
| fribidi | 1.0.16 | meson | Unicode bidirectional algorithm |
| pango | 1.54.0 | meson | text layout |
| gdk-pixbuf | 2.42.12 | meson | images |

Notes on the build:

- glib normally answers some configuration questions by running test programs, which a cross build can't do. Meson reads the answers for musl from the cross file instead (`tools/meson-cross.sh`, shared by all the meson builds).
- With everything static, gdk-pixbuf's image loaders (PNG, GIF, BMP, ICO, …) are built into the library.
- The libraries are LGPL and statically linked. The complete sources and the build that relinks them are in this repository.

`pangotest` checks the stack:

- **glib:** the main loop, GRegex, a GObject with a property and a signal, GVariant, a thread pool, gio files, and `g_spawn_sync` running `fc-match`.
- **pango:**
  - markup with word wrapping;
  - fallback per character within one string requested only as `sans-serif`: Noto Sans, DejaVu for ✓, Noto Naskh Arabic, Noto Sans Hebrew, Noto Sans Devanagari;
  - bidirectional runs.
- **gdk-pixbuf:** loads, scales and saves a PNG.

`/etc/fonts/local.conf` puts each script's Noto font before DejaVu Sans, which also covers Arabic and Hebrew but is only the last resort.

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

**Version 3** (`tools/elf2pulse.py`) holds a Linux program: `stack_top` is 0 (the kernel builds the Linux stack) and a 16-byte extension follows the header — `phdr_va` (u64), `phnum` (u16), `abi` (u16, 1 = Linux), reserved (u32) — so the C library can find its TLS template through `AT_PHDR`. Dynamically linked binaries are not converted; they run as ELF.

### Graphics stack
- **Kernel**: virtio-gpu 2D resource backed by the back buffer; `FB_PRESENT` transfers and flushes a rectangle; a 64×64 ARGB resource on the cursor queue provides the hardware cursor.
- **Shared region** (64 MiB): back buffer, wallpaper, blurred wallpaper, and eight 4 MiB app surfaces (stride 1280).
- **Window manager** (`user/shell`): owns the screen, composes windows, menu bar, dock and overlays with damage tracking and occlusion culling, and talks to apps over IPC: `HELLO` (register, get a surface), `POLL` (events: mouse, keys, menu actions, theme, size), `COMMIT` (frame ready, optionally with the changed rectangle; answered after composition), `TITLE`.
- **Toolkits**: `user/plib` (Rust) and `user/libc` (C) provide the same primitives, fonts, theme and `App` client.

### Selected syscalls
`IPC_SEND/RECV/REPLY/TRY_RECV/RECV_TIMEOUT`, `SPAWN`, `SPAWN_EX` (path + argv/envp, ELF or `.pulse`), `WAIT`, `EXIT`, `KILL`, `SLEEP`, `TIME_MS`, `RTC`, `SYSINFO`, `PROC_INFO`, `CPU_LOAD`, `FB_MAP/FB_PRESENT`, `SURF_MAP`, `CURSOR_SET/CURSOR_MOVE`, `INPUT_POLL`, `FS_COUNT/STAT/READ/WRITE/CREATE`, `NET_STATUS`, `TCP_CONNECT/SEND/RECV/STATUS/CLOSE`, `UDP_SEND/UDP_RECV`, `SBRK` (per-process heap), `GETRANDOM` (virtio-rng), `THREAD_CREATE`/`EXIT_GROUP` (threads sharing the address space, scheduled on any core), `FUTEX_WAIT/FUTEX_WAKE`, `MMAP/MUNMAP/MPROTECT`, `SOCKET/BIND/LISTEN/ACCEPT/CONNECT/SENDMSG/RECVMSG/PPOLL/MMAP_FD/MEMFD/SOCKETPAIR/SHUTDOWN` (kernel services shared with the Linux layer — same code, Pulsar numbers 78–89), `DEBUG_OOPS`, `OPEN/CLOSE/READ/WRITE/LSEEK/FSTAT/STAT/GETDENTS/MKDIR/UNLINK/RENAME/FTRUNCATE/SYMLINK/READLINK/DUP/PIPE/FCNTL`, `POWEROFF`. The full numbered list is documented at the top of `kernel/src/syscall.rs`.

---

## Project layout

```
.
├── Makefile                     # build/run for ARCH=aarch64|riscv64
├── pulsar.cfg, welcome.txt      # files placed on the disk image
├── rootfs/                      # copied into the image as-is (/etc/services, resolv.conf, hosts, passwd)
├── kernel/src/
│   ├── main.rs                  # boot sequence (boot CPU + secondaries)
│   ├── arch/{mod.rs, aarch64/, riscv64/}
│   ├── process.rs               # SMP scheduler, IPC, threads, fork/exec, waits
│   ├── syscall.rs, linux.rs     # Pulsar ABI / Linux ABI
│   ├── trap.rs, sync.rs, time.rs
│   ├── exec.rs, loader.rs, pulse.rs   # ELF + .pulse loading
│   ├── vm.rs, frame_allocator.rs      # mmap/VMAs, physical memory
│   ├── file.rs, ext2.rs         # descriptors + pipes, ext2 + block cache
│   ├── gpu.rs, fb.rs, input.rs, blk.rs, net.rs, rng.rs, virtio.rs
│   ├── signal.rs                # signal actions, frames, sigreturn
│   └── fs.rs, nbfs.rs, pfs.rs   # format detection, legacy flat filesystems
├── user/
│   ├── plib/                    # Rust userspace library (gfx, text, icons, theme, App)
│   ├── libc/                    # C userspace library (same feature set)
│   ├── init/                    # first process: starts and restarts the services
│   ├── shell/                   # Pulsar Desktop (compositor + window manager)
│   ├── wlsrv/ wlclient/         # Wayland compatibility service (listener + per-client translator)
│   ├── fsd/                     # filesystem daemon (IPC server)
│   ├── editor/ ball/ sysmon/ trespass/      # Rust apps
│   ├── terminal/ calc/ netmon/ vela/ hello_c/   # C apps
│   ├── linux/                   # Linux programs (musl): /bin, dynamic/, lib/ (-> /lib), rt/
│   └── third_party/             # musl, BearSSL, QuickJS, stb, NanoSVG, fonts (see its README)
└── tools/
    ├── mkpulse.py               # ELF -> .pulse (native apps)
    ├── elf2pulse.py             # static Linux ELF -> .pulse v3 (Linux ABI)
    ├── genkeymaps.py            # keyboard layouts -> native tables, XKB keymaps, Compose
    ├── build_linux_libs.sh      # libffi, libwayland-client, xdg-shell, libxkbcommon (no meson)
    ├── build_font_libs.sh       # expat, freetype, fontconfig (their own configure scripts)
    ├── build_cxx_runtime.sh     # libc++, libc++abi, libunwind (LLVM's CMake build)
    ├── build_draw_libs.sh       # zlib, libpng, pixman, harfbuzz, cairo (configure / meson)
    ├── build_glib_libs.sh       # pcre2, glib, fribidi, pango, gdk-pixbuf (configure / meson)
    ├── meson-cross.sh           # meson cross file + pkg-config wrapper for <arch>-linux-musl
    ├── musl-cc.sh               # "cross compiler" for configure scripts: musl sysroot, static links
    ├── mknblr.py / mkpfs.py     # disk image builders
    ├── genfonts.py              # font atlases (Rust + C)
    └── bin2c.py                 # embeds binary files (fonts, prelude.js) in C sources
```

---

## Roadmap

- Linux personality, next steps towards Firefox: GTK 3 on the Wayland backend (with its remaining dependencies: atk, epoxy, the cursor and icon themes), and finally Firefox itself. Popups larger than their window would need WM support for override-redirect surfaces.
- Fault tolerance: move device drivers (virtio-blk, virtio-net, virtio-gpu) out of the kernel into restartable user-space servers, so a driver bug becomes a restart instead of an oops.
- Interrupt-driven virtio (block, input, network) instead of polling; move more drivers into user-space servers.
- Vela: TLS 1.3, HTTP keep-alive, WebP images; shared clipboard between apps.
- Dynamic linking, ASLR, and running on real hardware (Raspberry Pi, VisionFive 2).

---

## License

Pulsar OS is released under the **MIT License**. See [`LICENSE`](LICENSE).

Copyright (c) 2026 João Antônio Temochko Andre.

The bitmap font atlases in `user/plib/src/fonts.rs` and `user/libc/src/font_*.c` are generated from **Noto Sans** and **Noto Sans Mono** (Google), licensed under the SIL Open Font License 1.1.

The Linux programs are linked against **musl** (MIT), built from the unmodified upstream source in `user/third_party/musl`.

Vela bundles third-party code under its own permissive licenses — BearSSL (MIT), QuickJS (MIT), stb_truetype / stb_image / stb_sprintf (public domain / MIT), NanoSVG (zlib), Noto (OFL 1.1) and DejaVu (Bitstream Vera) font subsets, and the Mozilla CA list. Details and local patches are in [`user/third_party/README.md`](user/third_party/README.md).

## Acknowledgements

ARM Architecture Reference Manual (ARMv8-A), the RISC-V Privileged Specification and SBI specification, the virtio 1.1 specification, the `aarch64-cpu` crate, OpenSBI, and the canonical MMU bring-up sequence of the Linux `arch/arm64` boot path.
