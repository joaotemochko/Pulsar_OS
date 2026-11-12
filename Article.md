# Reproducing the experiments of the article

## Contents
- `pulsar-kernel-fixes.patch`: every kernel change evaluated in the article (security stubs, process groups and sessions, demand paging in `execve`, copy-on-write for the `brk` heap, ASID allocator, orphan-inode reclamation, UART log switch). Apply it with `git apply` at the root of the Pulsar tree.
- `src/`: test and benchmark programs (static, musl).
- `scripts/`: disk image creation, QEMU runs, measurement campaigns and classifiers.
- `lists/`: run lists and the LTP selection (80 directories, 490 test cases).
- `linux-6.12-baseline.config`: configuration of the Linux baseline.

## Environment variables
```
export PULSAR_DIR=/path/to/Pulsar_OS      # tree with the patch applied
export BENCH_DIR=/path/to/this/package
export LINUX_IMAGE=/path/to/linux/arch/arm64/boot/Image
export LTP_DIR=/path/to/ltp               # LTP commit e74f6d6
export LTP_BIN=$BENCH_DIR/ltp_bin
```

## Kernel build options (environment variables for `cargo build`)
- `PULSAR_QUIET=1`: disables the UART log on every `execve` and process exit (used in all measurements).
- `PULSAR_EAGER_EXEC=1`: restores whole-file executable loading (the "eager" row of the exec table).
- `PULSAR_ASID_TEST=1`: limits the allocator to 8 ASIDs to exercise exhaustion (`asidtest`).

Without these options the kernel behaves normally.

One adjustment was needed only because of the measurement host: `RAM_SIZE` was reduced to 1.5 GiB in `kernel/src/arch/aarch64/mod.rs`, and QEMU runs with `-m 1536M`. This change is **not** in the patch. Apply it by hand if your machine has little memory, or replace `-m 1536M` with `-m 4G` in the `run_*` scripts.

## Steps
1. Build the kernel (`make kernel`), musl (`make build/aarch64/sysroot/lib/libc.a`), `init.pulse` and `fsd.pulse` as in the Pulsar Makefile.
2. Build the programs: `scripts/build_linux.sh src/*.c` and `scripts/mkbig.sh`.
3. Build Linux 6.12 with `linux-6.12-baseline.config` (`make ARCH=arm64 LLVM=1 Image`).
4. Latency (Table VI): `scripts/camp_final.sh` (10 boots, kernels interleaved), then `scripts/aggregate.py`.
5. Executable size (Table VII): build `k_lazy` (default) and `k_eager` (`PULSAR_EAGER_EXEC=1`) into `$BENCH_DIR` and run `scripts/exec_campaign.sh`.
6. Stubs: `run_pulsar.sh lists/stub.list stub_pulsar.log` and `run_linux.sh lists/stub.list stub_linux.log`.
7. LTP: `scripts/mkcc.sh`, `scripts/build_ltp.sh`, then `scripts/ltp_campaign.sh` and `scripts/ltp_classify.py <pulsar_log> <linux_log> out.json`. Always classify from the test output, not from the exit status.
8. Defects: `lists/orph.list` (orphan inodes), `lists/asid.list` (with a `PULSAR_ASID_TEST=1` kernel), `src/forkbug2.c` (heap copy-on-write), `src/forkloop.c 600` (ASID reuse), `lists/dyn.list` (dynamic linker).
9. Static coverage: `scripts/coverage.py`.

The scripts use `$KERNEL` to select the Pulsar binary (default: the one under `target/`).
