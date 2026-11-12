# The `tamnd/rucc-bsd` repository

`rucc-bsd` pins BSD source trees, reference toolchains, `src.conf` and `mk.conf` settings, kernel configurations and builder machines. It builds each tree with rucc and with the reference, writes disk images, boots them, runs the systems' own test suites against both, compares and reports. It has the shape and the restrictions of `rucc-kernel` and `rucc-postgres`, and whoever knows `rk` should find their way around `rb`.

It is the third harness in the family. `rucc-kernel` document 14.6 decided that code shared between `rpg` and `rk` would be copied, and factored out only "when a third harness needs it". This is that harness, so B0 does the factoring (15.6).

## 14.1 What it may and may not contain

The same four rules as `rucc-kernel`, stated for the BSDs:

- **No compiler code.** Nothing here compiles C. Every compile goes to rucc, the reference GCC, or the reference C++ compiler through the `rb-cc` shim, which records the call and changes nothing. A fix to rucc goes to `tamnd/rucc`.
- **No source patches, and no copied source files.** Trees are fetched by commit hash from the projects' own git servers or by SHA-256 from their release mirrors. The harness never writes into a tree. FreeBSD writes its objects under `MAKEOBJDIRPREFIX`, NetBSD under `-O` and `-T`, OpenBSD under `/usr/obj` and DragonFly under `/usr/obj`, and that is the only place output goes.
- **No reductions.** A reduced reproducer goes to `rucc-corpus`, or to `rucc-compat` if it compares two compilers. `rucc-bsd` links to it by issue number.
- **No hidden configuration.** Everything that influences a graded build is a committed file: pins, toolchain files, `src.conf`, `mk.conf`, kernel configuration names, the builder images, the exclusions and the list of explained differences.

Two rules are new, because a BSD build has a knob surface the kernel does not:

- **No knob turned to dodge rucc.** `src.conf`, `mk.conf` and `make -D` can turn off whole parts of a base system. A graded configuration may turn off only what document 02.1 lists, and every `WITHOUT_*` or `MK*=no` line carries the reason in a comment and applies identically to the reference build. Turning off a program because rucc miscompiles it is a patch by another name.
- **No toolchain file written for one compiler only.** The rucc toolchain file for FreeBSD and the rucc toolchain directory for NetBSD differ from the reference's in the C compiler line and nothing else. `rb toolchain-diff` checks this before every graded build (05.4).

## 14.2 Layout

```
rucc-bsd/
  Cargo.toml                    workspace, depends on rucc-harness (15.6)
  crates/
    rb/                         the CLI
    rb-shim/                    rb-cc and rb-c++: record every compiler call (14.4)
    rb-pins/                    pins.toml, toolchains.toml, builders.toml, fetch and verify
    rb-mk/                      bsd make introspection: make -V, src.conf effective knobs, mk-diff (05.5)
    rb-kconf/                   FreeBSD and NetBSD kernel config files, the options a build used
    rb-image/                   disk images: FreeBSD mkimg and makefs, NetBSD makefs and installboot
    rb-atf/                     kyua report parsing, atf-run and atf-report parsing, OpenBSD regress logs
    rb-ctf/                     CTF reading for ctf-diff (09.6)
  guest/
    rb-rc/                      the rc script that runs a suite and powers off (12.4)
    rb-bench/                   the benchmark program, C, built by rucc and by gcc (13.2)
  pins.toml                     trees per system and set (04.5)
  sets.toml                     set rules; rb sets expands them into pins.toml
  personas.toml                 the GCC version rucc claims per tree, and why (04.3)
  toolchains.toml               reference toolchains: pkg names and hashes, build.sh tool hashes
  builders.toml                 builder VMs and hosts: image hashes, cores, disk (14.6)
  rows.toml                     system, architecture, QEMU machine, firmware, console, timeouts
  conf/
    freebsd/src.conf.graded     the graded src.conf (02.1)
    freebsd/src.conf.c-only     the C-only row (10.5)
    freebsd/rucc.mk             the CROSS_TOOLCHAIN file for rucc (05.4)
    freebsd/kernel/RB-TEST      a config that includes GENERIC and adds test options (12.2)
    netbsd/mk.conf.graded
    netbsd/kernel/RB-TEST
    netbsd/toolchain/           how rb makes the TOOLDIR overlay (05.4)
  mk-divergences.toml           explained make -V and compiler-feature differences (05.5)
  sections-divergences.toml     explained section differences (09.5)
  exclusions.toml               tests excluded, each with the reference's failure log
  demands.toml                  the feature census (14.5)
  baselines/
    <system>/<row>/<version>/<conf>/   reference results, three runs
  provision/
    builders/freebsd-<v>/       how the FreeBSD builder image is made (14.6)
    builders/netbsd-host/       the Linux container that runs build.sh
    machines/                   gpc, server1 to server3
  runs/<id>/                    one directory per run (pruned, 14.7)
  reports/
    nightly.md
    weekly.md
  .github/workflows/
    nightly.yml
    weekly.yml
    pr.yml                      harness tests only, no system builds
```

## 14.3 Commands

| Command | Does |
|---|---|
| `rb fetch V` | fetch and verify a tree into the cache |
| `rb sets [--write]` | compare `sets.toml` with what the projects publish today, and rewrite `pins.toml` |
| `rb personas` | print the persona and the reference toolchain for every pin |
| `rb toolchain-diff` | check that the rucc toolchain file differs from the reference's only in the C compiler (05.4) |
| `rb probes V --row R` | run every compiler probe the build system makes, against both compilers, without building (05.3) |
| `rb baseline V --row R --conf C` | build, image, boot and test with the reference three times, and write `baselines/` |
| `rb build V --row R --conf C --cc rucc\|ref [--targets kernel,world,release]` | build through the shim |
| `rb image --build DIR` | turn a build into a bootable disk image (12.3) |
| `rb mk-diff` | compare `make -V` of every compiler-dependent variable and the effective knob set (05.5) |
| `rb flags-diff` | compare the command line of every unit (05.6) |
| `rb syntax --build DIR --cc rucc` | replay every C unit of a reference build through rucc's front end with `-fsyntax-only` (05.7) |
| `rb warnings --build DIR` | list every warning rucc gave that the reference did not, per unit (06.9) |
| `rb sections-diff`, `rb symbols-diff` | compare sections, linker sets, symbol versions and exported names (09.5) |
| `rb abi-diff` | compare the shared libraries' dynamic symbol tables and version definitions (09.4) |
| `rb ctf-diff` | compare the CTF in two kernels and in their modules (09.6) |
| `rb vec-audit` | decode no-FPU kernel objects and list vector and x87 instructions (08.3) |
| `rb frames` | compare stack frames from `-fstack-usage` (08.7) |
| `rb census` | which objects of a build came from rucc, from the C++ compiler, and from the assembler (10.4) |
| `rb boot --image IMG --suite S` | one boot (12.3) |
| `rb test --reference DIR --other DIR --kinds boot,smoke,kyua\|atf,regress` | the graded run, with attribution (12.6) |
| `rb mixed --unit U` | mixed-object bisection inside one library, program or kernel (12.7) |
| `rb cross-modules` | load each build's kernel modules into the other's kernel (09.3) |
| `rb foreign` | run the project's own release binaries and packages on the rucc system (12.8) |
| `rb bench` | the runtime benchmark set (13.2) |
| `rb speed` | compile time and memory, from the shim's records (13.4) |
| `rb demands` | the feature census (14.5) |
| `rb repro <run> <unit>` | print the commands that reproduce one failure without `rb` |
| `rb report`, `rb nightly` | reports, and the whole nightly |

Every graded command takes `--rucc PATH` and records the binary's hash and `rucc --version`. It refuses a binary built from a dirty tree unless `--ungraded` is passed. This is the same rule as `rk`.

## 14.4 The shims

A BSD base system calls three kinds of compiler: the C compiler, the C++ compiler, and the host compiler for the build tools. `rb` wraps all three.

- **`rb-cc`** is the C compiler of the target build. In a rucc build it runs rucc. In a reference build it runs the reference GCC. It is put in place by the toolchain file (05.4), never by editing the tree.
- **`rb-c++`** is the C++ compiler of the target build. It runs the reference `g++` in both builds (document 10). It records every call, so that `rb census` can say exactly which objects rucc did not build.
- **`rb-cpp` and `rb-as`** are recording-only wrappers for the standalone preprocessor slot (FreeBSD `XCPP`, NetBSD `cpp`) and for any direct assembler call. They exist so that the census (10.4) can say where preprocessor output goes and whether anything bypasses `${CC}` to assemble.
- **The host compiler** builds the bootstrap tools, such as FreeBSD's `bmake` and `mkimg` and NetBSD's `nbmake` and `nbmakefs`. It is the builder's own compiler in both builds, and it is not graded until B12 (16.13).

Each call becomes a line in `compile.jsonl`: working directory, argument vector, inputs, output, exit status, user and system CPU time, peak resident memory, a hash of standard error, and whether the call was a probe or a real unit. A call whose output goes to `/dev/null` or a temporary file is a probe. FreeBSD's `bsd.compiler.mk` runs `${CC} --version` and `${CC} -E` probes on every make invocation, so the probe lines are many and are kept separately.

`RB_TWICE=1` compiles every real unit twice and compares the bytes, as `RK_TWICE` does. `RB_BRINGUP=as,cxx-c` is the only door for the two bring-up delegations of 00, and it marks the run ungraded. `RB_BRINGUP_UNITS` names the units of `cxx-c` as `PATH=ISSUE`, such as `stand/i386/boot2/boot2.c=tamnd/rucc#4097`. A `PATH` that ends in `/` names a directory, and each C unit whose source is under it, or that make compiles in a directory under it, goes to the reference, such as `stand/=tamnd/rucc#4097`. A unit without an issue is refused, and the record of each delegated call says `cxx-c`, the issue and, for a directory, the directory.

## 14.5 The demand census (`rb demands`)

`demands.toml` records, per tree and row, which compiler features the reference build uses, from the reference build's preprocessed output, its `-S` output and the shim's command lines:

- attributes and how often, with `__section__` names grouped by prefix (`set_`, `link_set_`, `.note.`, `.init_array`);
- builtins;
- inline assembly constraints and modifiers, and `asm goto`;
- file-scope `asm` blocks, by the directives in them (07.3), which is where the three bugs of 03.2 live;
- assembler directives and mnemonics from the `.S` files;
- every `-f`, `-m`, `-W` and `--param` flag passed;
- the `#pragma` lines, since both systems' contributed code carries `#pragma GCC diagnostic` and `#pragma weak`.

Each feature gets rucc's state from `rucc --print-features`, which `rucc-kernel` K0 added. The census then reads like "11 features missing, blocking 4,212 of 21,880 units", which ranks the work. It is regenerated when a new tree is pinned, so a new release's new demands show up the day it is tagged.

## 14.6 Machines, builders and hosts

The four systems are built in two different ways, because the systems allow different things (04.4).

| System | Where the build runs | Why |
|---|---|---|
| NetBSD | a Linux container on gpc or server3, running `build.sh` | `build.sh` cross-builds from any POSIX host by design, and builds its own reference GCC there |
| FreeBSD | a FreeBSD builder VM under KVM on gpc, running `make buildworld buildkernel` | the reference cross GCC is a FreeBSD package that runs on FreeBSD hosts. `tools/build/make.py` from Linux is recorded, not graded (04.4) |
| OpenBSD | an OpenBSD builder VM, native `make build` | OpenBSD supports only native builds |
| DragonFly | a DragonFly builder VM, native `make buildworld` | the same |

The FreeBSD builder runs the latest FreeBSD release, with the pinned reference GCC packages, binutils, Rust and a rucc built on it from the commit under test. That makes FreeBSD a rucc host. It is a cheap step, because Rust supports FreeBSD as a host, but it is new, and B1 owns it.

| Machine | Role |
|---|---|
| gpc (32 cores x86-64) | the sweep worker. FreeBSD builder VMs at 24 vCPUs, NetBSD `build.sh` containers, KVM boots of every amd64 and i386 image |
| server3 (8 cores x86-64) | the benchmark box (13.2). Nightly amd64 tests when it is not benchmarking |
| server2, server1 | TCG boots for aarch64 and riscv64 images, and the nightly kyua and ATF runs on them |
| GitHub `ubuntu-24.04` runners | `pr.yml`, and a NetBSD `build.sh` kernel build as a canary, since it needs no VM |
| GitHub `ubuntu-24.04-arm` runners | aarch64 boots under KVM if the runners expose it, the same question as `rucc-kernel` 16.8 |

Rough capacity, to be replaced by measurement at B0:
- a FreeBSD `buildworld buildkernel` of 15.x on 24 vCPUs takes about 40 minutes with clang and is expected to take about as long with GCC. With rucc on the C part and g++ on the C++ part it should be shorter, since most of the C++ time is LLVM and the graded configuration does not build LLVM (10.3);
- a NetBSD `build.sh release` for amd64 on 32 cores takes about 45 minutes, of which the tools are about 10;
- a FreeBSD kyua run of `/usr/tests` takes about 1 to 2 hours on 4 vCPUs, and NetBSD's ATF run through anita about 2 to 3 hours;
- so one system, one version and one row, built and tested on both sides three times for a baseline, is a day of gpc. The nightly does one run per side, not three, on the current set only.

## 14.7 Storage

Per run we keep the manifest, the summary of `compile.jsonl`, the test results, the console logs and the diffs. Disk images are kept for failures only, for 14 days, since a FreeBSD image is about 5 GB and a NetBSD one about 2 GB. Baselines are kept forever and are small. The tree cache holds two to three trees per system (about 4 GB for FreeBSD, 2 GB for NetBSD), and the builder images about 40 GB. gpc needs about 600 GB of scratch on top of what `rucc-kernel` uses, which goes into open question 9.

## 14.8 Reports

`reports/nightly.md` follows `rucc-kernel`'s layout, with one section per system:

1. **Headline.** Per system and row: set members green, red and not run.
2. **Census.** The share of objects built by rucc, by the C++ compiler and by bring-up (which must be zero in a graded run).
3. **Regressions** since the last night, each with its bisected rucc commit.
4. **Differentials.** `mk-diff`, `flags-diff`, `sections-diff`, `abi-diff`, `ctf-diff`, `warnings`, `vec-audit`: counts and links.
5. **Tests.** Per suite, pass counts against the reference's G.
6. **Performance.** Static measures nightly, runtime benchmarks when server3 ran them.
7. **Compile time and memory.** The ratio against the reference GCC, and the slowest 10 units.
8. **Demands.** Missing features by blocked units.
9. **Open findings**, with issue links.
