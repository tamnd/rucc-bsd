# Research, November 2026

These are the facts every other document relies on. They were gathered on 7 and 8 November 2026. Labels:
- **[tree]**: read in a shallow clone of the project's GitHub mirror. The newest commits were FreeBSD `main` of 7 November, NetBSD trunk of 4 November, OpenBSD master of 7 November and DragonFly master of 6 November 2026. The FreeBSD clone excludes `contrib/llvm-project`, `sys/contrib/dev` and `contrib/kyua`. The NetBSD clone excludes the GCC, binutils, GDB and LLVM `dist/` trees.
- **[web]**: from a fetched page, linked inline.
- **[inference]**: our reasoning, not a sourced fact.
- **[unverified]**: could not be confirmed for 2026. B0's first job is to settle these.

## 01.1 Where the systems are today

**FreeBSD** [web] ([releases](https://www.freebsd.org/releases/), [security](https://www.freebsd.org/security/), [releng](https://www.freebsd.org/releng/)):

| Line | Version | Released | End of life |
|---|---|---|---|
| `main` | 16.0-CURRENT | | |
| `stable/15` | 15.1-RELEASE | 16 June 2026 | 31 March 2027; the branch to 31 December 2029 |
| | 15.2 | BETA1 on 6 November 2026, RC1 due 27 November, release due 8 December ([schedule](https://lists.freebsd.org/archives/freebsd-stable/2026-July/004186.html)) | |
| | 15.0 | 2 December 2025 | 30 September 2026, already out of support |
| `stable/14` | 14.5-RELEASE | 8 September 2026 | 30 June 2027; the branch to 30 November 2028 |
| | 14.4-RELEASE | 10 March 2026 | 31 December 2026 |

13.x is no longer in the supported table. A stable branch gets four years from its .0 release, and a point release is supported until three months after the next ([Klara](https://klarasystems.com/articles/freebsd-release-cycle-and-upgrade-schedule/)). Scheduled: 14.6 in March 2027, 15.3 in June 2027, 16.0 in December 2027, then 15.4, 16.1, 15.5 and 16.2 through December 2028.

**NetBSD** [web] ([changes](https://www.netbsd.org/changes/), [releases](https://www.netbsd.org/releases/), [11.0 notes](https://www.netbsd.org/releases/formal-11/NetBSD-11.0.html)):

| Line | Version | Released |
|---|---|---|
| trunk | 11.99 | |
| `netbsd-11` | 11.1 | 2 November 2026 |
| | 11.0 | 30 July 2026, announced 1 August |
| `netbsd-10` | 10.2 | 15 September 2026 |
| `netbsd-9` | 9.5 | 31 August 2026, the final 9.x; the branch is end of life |

No 12.0 schedule is published [unverified].

**OpenBSD** [web]: 8.0 on 20 October 2026 ([80.html](https://www.openbsd.org/80.html)), 7.9 on 19 May 2026. Only the two newest releases are supported ([FAQ5](https://www.openbsd.org/faq/faq5.html)). 8.1 is expected around May 2027 from the six-month cadence [inference].

**DragonFly** [tree, web]: 6.4.2, tagged 9 May 2025, is the newest release. Master is tagged v6.5.0. No schedule or end of life is published.

## 01.2 How each system builds with GCC

**FreeBSD** [tree] (`Makefile.inc1`, `share/mk/bsd.compiler.mk`, [build(7)](https://github.com/freebsd/freebsd-src/blob/main/share/man/man7/build.7)):
- **The external toolchain.** `pkg install amd64-gcc15`, then `CROSS_TOOLCHAIN=amd64-gcc15`. `Makefile.inc1` includes `${LOCALBASE}/share/toolchains/${CROSS_TOOLCHAIN}.mk`. The port's file sets `XCC`, `XCXX`, `XCPP`, `CROSS_BINUTILS_PREFIX` and `X_COMPILER_TYPE=gcc` ([port files](https://github.com/freebsd/freebsd-ports/tree/main/devel/freebsd-gcc14/files)).
- **The ports.** `freebsd-gcc14` is GCC 14.2.0 and `freebsd-gcc15` is 15.2.0. There is no `freebsd-gcc16` port. The GCC ports link with BFD `ld` from `devel/binutils`, not `ld.lld`.
- **A FreeBSD-local GCC patch** (`files/patch-gcc_c-family_c-format.cc`) adds the `__freebsd_kprintf__` format archetype with `%b`, `%D`, `%r` and `%y`. The kernel uses `-D__printf__=__freebsd_kprintf__` with `-Werror` under GCC 12.1 or later.
- **Refusals.** `Makefile.inc1` fails GCC builds for 32-bit `arm` ("D36754"), `powerpc` ("Missing atomic builtins in libc++") and `powerpc64*` ("libsys.so.7.full has a LOAD segment with RWX permissions") unless `TRY_GCC_BROKEN` is set. `src.opts.mk` marks `LIB32` broken on aarch64 under GCC.
- **CI.** Three Jenkins jobs build with `CROSS_TOOLCHAIN=amd64-gcc14` and `SRCCONF=/dev/null`, in a 15.1-RELEASE jail: `main`, `stable/14` and `stable/15` ([freebsd-ci](https://github.com/freebsd/freebsd-ci/tree/main/jobs/FreeBSD-main-amd64-gcc14_build)). There is no `gcc15` job. `make universe` defaults to `gcc15`.
- **CI status [web].** All three jobs were "Still Failing" in October 2026 ([dev-ci](https://lists.freebsd.org/archives/dev-ci/2026-October/)). `main` build #5096 of 31 October stops in `buildkernel` ([message](https://lists.freebsd.org/archives/dev-ci/2026-October/025634.html)). `stable/14` #1335 fails in `buildworld` ([message](https://lists.freebsd.org/archives/dev-ci/2026-October/024979.html)). The real errors are truncated in both. Bug 297758 (22 August 2026): `usr.sbin/pmc/headers.hh` has a C++ flexible array member that `g++` 14 rejects and `g++` 15 and clang 21 accept ([report](http://www.mail-archive.com/freebsd-bugs@freebsd.org/msg98069.html)).
- **From Linux.** `tools/build/make.py` bootstraps `bmake` and takes `--cross-compiler-type gcc`. The only GitHub Actions build workflow cross-builds amd64 and aarch64 GENERIC with clang 15 and 18. The old Cirrus GCC task is gone.

**How FreeBSD identifies a compiler** [tree] (`bsd.compiler.mk`, `bsd.linker.mk`):
1. **Type.** A basename matching `*gcc*` or `*clang*`; else a `--version` word `gcc`, `(GCC)`, or ending in `GNU` for gcc, `clang` for clang; else `gcc version` in `-v`; else an error.
2. **Version.** The first `--version` word matching `[1-9]*.[0-9]*`, as `major*10000+minor*100+patch`.
3. **`COMPILER_FREEBSD_VERSION`** from `echo __FreeBSD_cc_version | ${CC} -E -`.
4. **The resource directory** from `-print-resource-dir`, failure tolerated.

The results are cached in the environment, keyed by a hash of `CC`, `MACHINE` and `PATH`. `COMPILER_FEATURES` follows the version:

| Feature | clang | GCC |
|---|---|---|
| `c++17` | yes | 7 or later |
| `c++20` | 10 or later | 10.1 or later |
| `init-all` | yes | 12 or later |
| `fileprefixmap` | 10 or later | 8.1 or later |
| `zeroregs` | 15 or later | 11 or later |
| `aarch64-sha512` | 13 or later | 9 or later |
| `blocks`, `retpoline`, `compressed-debug` | yes | no |

`ld -v` decides the linker type: "GNU ld" is `bfd`, "LLD" is `lld`. `lld` adds the features `retpoline` (6+), `ifunc-noplt` (9+) and `bti-report` (14+).

**NetBSD** [tree] (`build.sh`, `share/mk/bsd.own.mk`):
- **`build.sh`** builds host tools and a cross GCC into `TOOLDIR` from `external/gpl3/gcc`, from any POSIX host. Options include `-m`, `-a`, `-c gcc|clang` and `-V`.
- **No `--version` probe.** Identity is `HAVE_GCC=<major>`, `HAVE_LLVM` and `HAVE_PCC`. `AVAILABLE_COMPILER` picks `ACTIVE_CC` and sets `CC=${TOOL_CC.${ACTIVE_CC}}`. Warning flags are keyed on the number: the `CC_WNO_*` sets at `HAVE_GCC` 7, 8, 10, 12 and 14 (`bsd.own.mk:741-785`).
- **`EXTERNAL_TOOLCHAIN`** uses `${EXTERNAL_TOOLCHAIN}/bin/${MACHINE_GNU_PLATFORM}-gcc` and its siblings, and **forces `MKGCC=no`**, because GCC's support libraries are "tightly-coupled to the version of GCC being used". `libgcc` and `libgcc_eh` are then not built from the tree. This is why 05.4 overlays the `TOOLDIR` instead.
- **In-tree LLVM** is 13.0.0 on all three branches, used only with `build.sh -c clang`.

**DragonFly** [tree]: `CCVER=<name>` makes `/usr/bin/cc` run `/usr/libexec/<name>/cc`, or `/usr/libexec/custom/`, configured in `/etc/compilers.conf` (`compilers.conf(5)`). `bsd.sys.mk` enables the GCC warning sets on `${_WCCVER:Mgcc*}` and parses the version from the name. `-Werror` applies only for `gcc80` and `gcc120`.

**OpenBSD** [tree]: no probes. `COMPILER_VERSION` and `LINKER_VERSION` come from per-architecture tables in `bsd.own.mk`. Builds are native only ([FAQ5](https://www.openbsd.org/faq/faq5.html)).

## 01.3 Base compilers and the C dialect

| | FreeBSD | NetBSD | DragonFly | OpenBSD |
|---|---|---|---|---|
| Base compiler | clang/LLVM 21.1.8 on `main`, `stable/15` and `stable/14`; 19.1.7 in 15.0, 15.1 and 14.4 ([15.0 notes](https://www.freebsd.org/releases/15.0R/relnotes/)) | GCC 14.4.0 on trunk (12 kept as `gcc.old`); 12.5.0 on `netbsd-11`; 10.5.0 on `netbsd-10` | GCC 12.5.0 on master since 21 October 2026; 8.3 in 6.4.x; `gcc47` removed 11 April 2026 | clang 22.1.6 with local patches in 8.0; GCC 4.2.1 on alpha, hppa, m88k and sh |
| binutils | `devel/binutils` for GCC builds | 2.46 on trunk, 2.42 on `netbsd-11` | 2.34 | BFD 2.17 on the GCC architectures and mips64; `lld` elsewhere |
| GCC history | 4.2.1 removed from the tree on 29 February 2020 (UPDATING) | GCC is the default | GCC is the default | |
| Minimum GCC | `sys/sys/cdefs.h` says "All supported versions of gcc (10+)". UPDATING's "GCC 6.4" entry of 2020 is stale | the in-tree version | the in-tree version | |
| C standard | `CSTD?=gnu17`, older than C99 is an error | `-std=gnu11` | `CSTD?=gnu11` | the compiler's default |

**OpenBSD-local clang options** in kernel and libc ([clang-local(1)](https://github.com/openbsd/src/blob/master/share/man/man1/clang-local.1)): `-fret-protector` (retguard), `-fret-clean`, `-ffixup-gadgets`, `-msave-args`, and `-mretpoline` on by default on amd64. The amd64 GENERIC kernel adds `-mretpoline-external-thunk -fcf-protection=branch -fret-clean`.

## 01.4 C++ in base

- **FreeBSD** [tree]. `WITHOUT_CXX` was removed on 26 January 2023: "C++ components in the base system are now built unconditionally" (UPDATING). Outside `contrib/llvm-project`: `devd`, `ctld`, `zfsd`, `pmc`, `libpmc`, `libdevdctl`, `libutil++`, `dtc`, `config(8)`, `users`, `tcopy`, `rs`, kyua, ATF, lutok, GoogleTest, `libcxxrt`, and `libc++`. The knobs that remove large C++ blocks are `WITHOUT_CLANG`, `WITHOUT_CLANG_BOOTSTRAP`, `WITHOUT_LLD`, `WITHOUT_LLD_BOOTSTRAP`, `WITHOUT_LLDB`, `WITHOUT_LLVM_BINUTILS`, `WITHOUT_GOOGLETEST`, `WITHOUT_PMC` and `WITHOUT_ZFS` (`tools/build/options`). Document 10.2 counts the files.
- **NetBSD** [tree]. GCC itself, `libstdc++`, ATF's C++ half, kyua (off by default), GDB, groff, gold, and some `sys/external` code. `MKCXX=no` forces `MKATF`, `MKGCCCMDS`, `MKGDB`, `MKGROFF` and `MKKYUA` to `no`. It is the only one of the four systems where a C-only base is a supported configuration.
- **DragonFly.** GCC 12 and 8 themselves, `libstdc++`, gold. No general switch.
- **OpenBSD.** clang, `lld`, `lldb`, `libc++`, `libc++abi`. No switch on clang architectures.

No `.rs` file exists in any of the four base trees [tree].

## 01.5 Tests and how the projects run them

- **FreeBSD** [tree]. kyua with ATF, TAP, plain and GoogleTest programs in `/usr/tests`. The tree has about 5,041 `ATF_TC*(` C and C++ cases and about 4,059 `atf_test_case` shell cases, including the imported NetBSD tests. No published 2026 total was found [unverified]. CI's `*_testvm` jobs boot an image under bhyve with 2 vCPUs and 8 GB, or QEMU with `USE_QEMU=1` for other architectures, then run `kyua test` and `kyua report-junit` ([run-tests.sh](https://github.com/freebsd/freebsd-ci/tree/main/scripts/test)). Jobs include KASAN, KMSAN, DTrace, ZFS, LTP and a TCP test suite.
- **NetBSD** [tree, web]. ATF in `/usr/tests` via `atf-run` or kyua. `tests/` has 361 Makefiles, about 2,812 C `ATF_TC` cases and about 1,731 shell cases. The bare-metal amd64 run of `-current` on 4 October 2026 had **10,870 passed, 12 failed, 587 skipped and 140 expected failures** ([gson](https://www.gson.org/netbsd/bugs/build/amd64-baremetal/)). Official anita runs under QEMU or NVMM: amd64 daily on HEAD, `netbsd-11`, `netbsd-10` and `netbsd-9`; i386 about six times a day; aarch64, riscv64 and others daily ([test-results](https://releng.netbsd.org/test-results.html)).
- **OpenBSD.** `regress/` with 1,055 Makefiles, run natively with `make regress`.
- **DragonFly.** No ATF or kyua. `test/` holds ad hoc cases and benchmarks.

## 01.6 The size of the thing

Counted with `scc` on the clones of 01's header dates [tree]:

| | Kernel C lines (files) | Userland C lines (files) | Notes |
|---|---|---|---|
| FreeBSD `main` | 5.92M (6,227) | 7.10M (14,992) | headers 2.66M kernel and 2.25M userland; C++ outside LLVM about 330k lines; `contrib/llvm-project` adds 5,702 C++ files |
| NetBSD trunk | 5.13M (9,542) | 10.9M (23,525) | headers 1.99M and 4.38M; C++ 102k kernel and 215k userland, outside the excluded `dist/` trees |
| OpenBSD master | 2.70M (3,564) | 6.56M (10,002) | without `gnu/llvm` and the AMD DRM driver |
| DragonFly master | 2.79M (2,813) | 6.04M (9,601) | 1.88M lines of C++, nearly all GCC |

FreeBSD and NetBSD together are about 29 million lines of C, the "30 million" of 02.5. Assembly counts per tree are in 07.1.

## 01.7 Changes in 2025 and 2026 that matter

- **FreeBSD 15.0 retired i386, armv6 and 32-bit powerpc** ([15.0 notes](https://www.freebsd.org/releases/15.0R/relnotes/)). armv7 is the last 32-bit platform. The i386 kernel configurations still exist on `main`.
- **pkgbase.** In 15.1, `bsdinstall` uses pkgbase by default, and `installworld` and `installkernel` are blocked on pkgbase-installed systems. On `main`, `release/Makefile.vm`'s `vm-image` needs a pkgbase repository (12.3).
- **C23.** FreeBSD 15.1 adds `<stdbit.h>` and `memalignment()`, with "complete support anticipated for FreeBSD 16". NetBSD 11.0 claims "enhanced compliance with POSIX.1-2024 and C23" and ships a `c17(1)` wrapper. Both systems' default `-std` is unchanged.
- **LLVM 21** was merged to FreeBSD `main`, `stable/15` and `stable/14`.
- **NetBSD 11.0** added the riscv and virt68k ports and the MICROVM x86 kernel. Trunk moved to GCC 14 and binutils 2.46.
- **powerpc64le** on FreeBSD changed `long double` to IEEE binary128 on 20 June 2026. It is outside the claim (02.6), but it shows that ABIs still move.
- **DragonFly** switched its default compiler from `gcc80` to `gcc120` on 21 October 2026.
- **OpenBSD 8.0** moved to clang 22, switched sparc64 to the LLVM toolchain, and retired loongson as "not working with Clang 22".

## 01.8 Prior art

- **Changing a BSD's compiler has been done, by the projects themselves.** FreeBSD moved from GCC 4.2.1 to clang over several releases and kept GCC buildable as an external toolchain after removing GCC from the tree in 2020. NetBSD keeps a clang build (`build.sh -c clang`, LLVM 13) beside its GCC default, and still has `HAVE_PCC` in `bsd.own.mk`. Both show that each system's build already separates "which compiler" from "what the tree is", which is what this folder relies on.
- **Two-compiler builds.** FreeBSD's GCC CI jobs are the nearest thing to G that exists today. They build but do not boot or test, and they were failing in October 2026 (01.2).
- **`tamnd/rucc-kernel`.** The sibling folder for Linux. It established the harness shape (pins, shims, baselines, the 2x2, mixed-object bisection) that `rucc-bsd` copies, and most kernel compiler features a BSD kernel needs (03.8).

No project we found builds a whole BSD base system with a compiler that is neither GCC nor clang and grades it against the system's own tests [unverified: absence is hard to prove].

## 01.9 Where rucc's own spec stands

- **The target matrix** (`spec/cross-compile/04-target-matrix.md` in `tamnd/rucc`) lists the BSDs at plan tiers 2 and 3, for user programs linked against stub libraries, as zig does. 15.1 proposes moving `x86_64-freebsd` and `x86_64-netbsd` to tier 1.
- **`tamnd/rucc-cross`** already carries `x86_64-freebsd`, `aarch64-freebsd`, `x86_64-netbsd`, `aarch64-netbsd` and `x86_64-openbsd` facts, taken from zig 0.16.0 and not yet checked against a reference GCC (15.5).
- **rucc 0.30.15 refuses every BSD target** (03.1). Nothing in this folder has been built on a BSD yet.
- **The target ladder's rung 4** names only the Linux kernel. 15.1 proposes adding this folder's levels A, B and C.
