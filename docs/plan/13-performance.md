# Performance

02.5 sets two gates:
- the code rucc makes runs as fast as the reference's;
- rucc compiles at least twice as fast.

This document defines both measurements so that they can be repeated, and says what is reported but not gated.

## 13.1 What "as fast" means for a base system

A base system's speed is in three places:
- **the kernel's paths:** system calls, page faults, scheduling, the network stack and file systems;
- **libc and the dynamic linker,** which every program shares;
- **the tools people run in scripts and builds:** `sh`, `awk`, `sed`, `grep`, `sort`, the compressors, `ssh`.

Benchmarks of applications built from ports or pkgsrc would measure the package's compiler, not rucc. The only part of a package's speed rucc owns is the libc and kernel below it, and that is measured directly.

**The reference is G, not the official release.** For FreeBSD, the official release is built with clang. R against the official FreeBSD release is reported as an informational column, because a reader will ask, but the gate is R against G, built with GCC 15.2 from the same commit.

## 13.2 `rb bench`

The benchmark set is a C program, `rb-bench`, in the `rucc-bsd` repository, plus scripts that drive the base system's own tools. `rb-bench` is built twice for each system, once by rucc and once by the reference. Each image runs the copy built by its own compiler, so the benchmark program's own code is graded with the system.

| Group | Benchmarks | Measures |
|---|---|---|
| system calls | `getpid`, `read` and `write` of 1 byte on a pipe, `open` and `close`, `stat` of a cached path, `gettimeofday` (which goes through the vDSO or shared page on both) | the system call path and `libc` stubs |
| processes | `fork` with `wait`; `vfork` with `exec` of `/usr/bin/true`; `posix_spawn`; context switch by pipe ping-pong between two processes and between two threads | `fork`, the VM system, `rtld` start-up, the scheduler |
| memory | page faults over a 1 GB anonymous `mmap`; `mmap` and `munmap` of 64 KB in a loop; `malloc` and `free` mixes from 16 bytes to 1 MB in one and four threads (jemalloc on both systems) | `pmap`, `uvm` or `vm_map`, the allocator |
| events | `kqueue` with 1,000 descriptors; `poll` and `select` with 100 | the event code |
| locking | uncontended and 4-thread contended `pthread_mutex`; `pthread_cond` ping-pong; `rwlock` readers | `libthr` and `libpthread`, the kernel's `umtx` or `futex`-like path |
| network | TCP over loopback with one stream and with eight, both directions, using `rb-bench`'s own sender and receiver; UDP 64-byte packets per second over loopback | the network stack, `mbuf` handling, checksum code |
| file systems | extract a fixed 300 MB tar archive of small files onto UFS or FFS on a memory disk; `find` and `stat` over it; `rm -r`; a 4 GB sequential write and read on a virtio disk | the buffer cache, UFS or FFS, `md` or `vnd`, virtio |
| tools | `sh` running a fixed script of loops and expansions; `awk` and `sed` over a 200 MB log; `grep` with a fixed and a regular expression; `sort` of 10 million lines; `gzip`, `bzip2`, `xz` and `zstd` compress and decompress of a fixed corpus | userland C |
| crypto | `openssl speed` for `aes-256-gcm`, `sha256` and `rsa2048`; `ssh` over loopback copying 1 GB to `/dev/null` with the default cipher | OpenSSL's C and `perlasm`, OpenSSH |
| libc | `qsort`, `strtod` and `printf("%g")` over a fixed data set; `regcomp` and `regexec`; `memcpy` and `strlen` at 16 sizes; `libm` over a fixed argument set | `libc` and `libm` |

These are about 60 numbers. The list is fixed per major release of `rb-bench`. Adding a benchmark is a release, so geometric means stay comparable over time.

**Text size.** Also reported: `.text` size of the kernel, of `libc.so`, of `rtld`, and the total `.text` of `/bin`, `/sbin`, `/usr/bin` and `/usr/sbin`. R must be within 5% of G for the kernel and `libc.so` (02.5).

## 13.3 Method

**The machine.** server3:
- SMT off;
- the CPU frequency fixed;
- no other guest running during a benchmark run.

The guest is pinned to four host cores with 8 GB. R and G images are identical apart from the build that made them. Same image layout, same packages from the local repository (12.3), same `sysctl` settings.

**The runs.**
1. Interleave R and G: boot R, run the set once, shut down; boot G, run the set once, shut down. Repeat 10 times, so 20 boots.
2. For each benchmark, take the median over its 10 runs per side. The R/G ratio is the gated number.
3. A benchmark whose coefficient of variation exceeds 1% on G is reported but not gated, until it is fixed or removed.
4. The geometric mean of the gated ratios is the headline.

**Cadence.** Weekly on the current releases of both systems, and on any rucc commit that changes code generation by more than 0.5% in `rucc-real-corpus`'s size row. A full run is about 6 hours.

**When a benchmark is slow.**
1. `rb mixed --perf` uses the same object bisection as 12.7, timing a benchmark instead of passing a test.
2. Then `pmcstat` on FreeBSD, or `tprof` on NetBSD, on both images, compares the hottest functions.

The result is a `rucc-corpus` performance case or a `rucc-real-corpus` row, as for `rucc-kernel` K8.

## 13.4 `rb speed`: compile time

**Sources.** Every compile is recorded by `rb-cc` in `compile.jsonl` (14.4) with:
- the unit;
- user and system CPU time of the compiler process and its children, from `wait4` `rusage`;
- the peak resident memory of the largest process;
- the exit status.

The reference's numbers come from the same shim wrapping the reference compiler in G. So both include the driver, `cc1` or rucc's front end, the assembler (GCC's `as`, rucc's integrated one) and any `collect2`.

**What is counted.**
- Only C units and `.S` files given to the C compiler slot.
- Configure probes, which are recorded with a `probe` tag (05.8), and link steps are excluded.
- Units are matched by output path between R and G. A unit present in only one side makes the comparison invalid until it is explained.

**Outputs.**

| Number | Gate |
|---|---|
| sum over C units of rucc CPU time divided by the reference's, per build (world, kernel) | at most 0.5 for the graded rows, B8 onward (02.5) |
| the same ratio at the 50th, 90th and 99th unit percentiles | reported |
| the 50 units with the worst ratio, with their size in lines after preprocessing | reported, each above 1.0 is a performance issue in rucc |
| largest peak memory of any rucc process | at most 1.5 GB |
| wall time of the whole build, R and G | reported, not gated, because `g++` and `make` dominate it |
| probes slower than 20 ms | each one is a bug (05.8) |

**The same host for both sides.** FreeBSD R and G are built on the same builder VM image and the same host. NetBSD R and G are built in the same container image on the same host. Builds are not run concurrently with benchmarks on server3.

**What to expect.** rucc's 1.75x on Linux 7.3 `defconfig` (rucc #4024) is for kernel code with heavy headers. BSD userland has many small units, where process start-up and header parsing dominate. rucc's start-up time and its preprocessor speed matter more here than on Linux. B8 starts from a profile of the 50 worst units, not from a guess.

## 13.5 What is not measured

- **Runtime speed of `g++`-compiled code.** It is the same binary in R and G.
- **Link time.** It is the same linker. If rucc's objects make links slower, for example through more sections or more relocations, that shows in wall time and is investigated, but it is not gated.
- **Benchmarks of packages.** They are not built by rucc (13.1).
- **Performance on aarch64 and riscv64 under TCG.** Emulated timing says nothing about hardware. These rows get performance gates only if open question 7 provides hardware.
