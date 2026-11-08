# The C++ boundary

rucc is a C compiler, and both base systems contain C++. This document draws the line between them, says which objects fall on which side, and describes the rows that make the C++ side as small as each system allows. Nothing here asks rucc to compile C++.

## 10.1 The line

**The rule.**
- In R, every C translation unit and every assembly file the build gives to the C compiler slot is rucc's.
- Every C++ translation unit is compiled by the reference's C++ compiler, the same binary G uses, with the same flags.
- The C++ standard library, its runtime and its headers are whatever G builds.

| Slot | FreeBSD | NetBSD | In R |
|---|---|---|---|
| C compiler | `XCC` | `TOOL_CC.gcc`, `bin/x86_64--netbsd-gcc` in `TOOLDIR` | rucc, through the `rb-cc` shim |
| C preprocessor | `XCPP` | `bin/x86_64--netbsd-cpp` | the reference at first; rucc where the census shows it feeds C (05.4) |
| C++ compiler | `XCXX` | `bin/x86_64--netbsd-g++` | the reference, through the recording-only `rb-c++` shim |
| Assembler for `.S` | `${CC}` | `${CC}` | rucc |
| Linker | `CROSS_BINUTILS_PREFIX` or `-B` | `TOOLDIR` | the reference BFD `ld` |

**C++ input on the C slot.** If the C compiler slot is given a C++ input (`-x c++`, or a `.cc`, `.cpp` or `.cxx` file), the shim:
1. hands it to the reference `g++` with the same arguments;
2. records it in the census as `cxx-on-cc`;
3. flags it in the report.

This keeps the line exact without a patch. B0's census says whether it ever happens. Linking C++ objects through `${CC}` with an explicit `-lc++` or `-lstdc++` is not this case: rucc's driver links them like any objects.

**Why the line is here.** The user's goal is a C compiler that builds and runs the BSDs. A C++ front end is a different project, larger than this one. Grading it would need its own oracle. Meanwhile the parts of the BSDs that matter most to a C compiler are C: the kernels, libc, `rtld`, `libthr`, the start files and nearly all of userland. 10.4 measures how much.

## 10.2 What C++ there is

Counted from the trees of 7 November 2026. These are source files, not objects: they overcount what is built, because they include tests, unused contributed code and other architectures. The census (10.4) replaces them with built objects.

**FreeBSD** (`main`, 421 C++ files outside `contrib/llvm-project` and `contrib/kyua`, which were not counted):

| Part | Where | Knob |
|---|---|---|
| LLVM: `clang`, `lld`, `lldb` | `contrib/llvm-project`, `lib/clang`, `usr.bin/clang` | `WITHOUT_CLANG`, `WITHOUT_LLD`, `WITHOUT_LLDB` (10.3) |
| C++ runtime: `libc++`, `libcxxrt`, `libc++experimental` | `lib/` | none, since `WITHOUT_CXX` was removed in January 2023 |
| `libgcc_eh` and `libgcc_s`: `libunwind.cpp`, `Unwind-EHABI.cpp` | `lib/libgcc_eh/Makefile.inc` | none |
| the test runner: `kyua`, `lutok`, ATF's C++ library | `usr.bin/kyua`, `contrib/lutok`, `contrib/atf` | `WITHOUT_TESTS_SUPPORT`, never used |
| `googletest` and the tests built on it: `tests/sys/fs/fusefs`, `tests/sys/capsicum`, `zfsd` tests | `contrib/googletest` (106), `tests/sys` (69) | `WITHOUT_GOOGLETEST` |
| `devd`, `libdevdctl` | `sbin/devd`, `lib/libdevdctl` | none |
| `ctld` | `usr.sbin/ctld` (10) | none |
| `dtc` | `usr.bin/dtc` (6) | none |
| `config(8)` | `usr.sbin/config` (3) | `WITHOUT_TOOLCHAIN` |
| `zfsd` | `cddl/usr.sbin/zfsd` | `WITHOUT_ZFS`, which removes all of OpenZFS (10.5) |
| `pmc`, `libopencsd` | `usr.sbin/pmc` (8), `contrib/opencsd` (45) | `WITHOUT_PMC` |
| `libutil++` | `lib/libutil++` (4) | none |

**NetBSD** (1,036 C++ files outside the GCC, binutils, GDB and LLVM `dist` directories, which were not counted):

| Part | Where | Knob |
|---|---|---|
| GCC and its C++ runtime `libstdc++`, `libsupc++` | `external/gpl3/gcc` | `MKCXX=no` turns off the installed compiler (`MKGCCCMDS`) and `libstdc++` (`lib/Makefile:184`, `external/gpl3/gcc/lib/Makefile:11`) |
| GDB | `external/gpl3/gdb` | `MKGDB`, forced off by `MKCXX=no` |
| `groff` | `external/gpl2/groff` (106) | `MKGROFF`, forced off |
| ATF's C++ half and its tools | `external/bsd/atf` (77) | `MKATF`, forced off with all of ATF |
| `kyua` | `external/bsd/kyua-cli` (180), `lutok` (16) | `MKKYUA`, forced off |
| LLVM and `libc++` | `external/apache2/llvm`, `external/bsd/libc++` | built only with `MKLLVM` or `HAVE_LLVM`, not in a GCC build |

The forcing is in `share/mk/bsd.own.mk:1674-1680`.

**The sizes that matter.** On both systems the C++ that is always built is small in source lines next to the C. The exceptions are LLVM on FreeBSD and GCC on NetBSD, which are each about as large as all the rest of the C++ together. The two systems differ in where their large C++ is:
- **FreeBSD's large C++ (LLVM) can be removed** with supported knobs and still leave a system that boots and passes its tests, apart from tests that need a compiler (10.3).
- **NetBSD's large C++ (GCC) is the system compiler.** It is built by `g++` in both R and G, and removing it removes ATF too.

## 10.3 The graded FreeBSD configuration does not build LLVM

The `graded` row sets:
- `WITHOUT_CLANG`;
- `WITHOUT_CLANG_BOOTSTRAP`;
- `WITHOUT_LLD`;
- `WITHOUT_LLD_BOOTSTRAP`;
- `WITHOUT_LLDB`.

**Why it is allowed.** These knobs remove only C++, LLVM's own. LLVM is compiled by `g++` in both R and G, so it says nothing about rucc. It is also the largest single component of a world build. B0 measures its share of compile time from G's `rb-c++` records. Building it nightly in both R and G would cost that share twice for no information.

**What it changes in the installed system.**
- There is no `cc`, `c++`, `cpp` or `ld` in `/usr/bin`. The FreeBSD tests that compile code at run time skip in both R and G, and the 2x2 (12.6) is unaffected.
- `WITHOUT_CLANG_BOOTSTRAP` and `WITHOUT_LLD_BOOTSTRAP` stop the build from making its own cross compiler and linker, which it does not need when `CROSS_TOOLCHAIN` is given.

**The `full` row** (`SRCCONF=/dev/null`, weekly) builds LLVM with `g++` in both R and G, as FreeBSD's own GCC CI does. It catches the cases where the C side and the LLVM build interact. The main one is `lib/clang/libllvm` linking against rucc's libc, through the dynamic linker rucc built, in a link step of hundreds of megabytes.

**B10's self-hosting row** turns this around. rucc becomes the system's `cc` (16.11), so the graded image then has a C compiler again, and those run-time compile tests run on the rucc system with rucc.

## 10.4 `rb census`

The census says, for one build, which compiler made each object, and so how much of the system is rucc's.

**Inputs.**
- The `rb-cc` shim records every C compiler invocation: arguments, input files, output files, exit code, CPU time and peak memory.
- The `rb-c++` shim records every C++ compiler invocation the same way.
- In NetBSD, `.S` files go through `${CC}`, so they appear as `rb-cc` records with an assembly input.
- In FreeBSD, `${CC}` also assembles, and a direct `${AS}` call, if there is one, is recorded by an `rb-as` shim.
- `XCPP` and NetBSD `cpp` calls are recorded by an `rb-cpp` shim with their output files. This answers 05.4's question of whether preprocessor output feeds C.

**Outputs per build.**

| Report | Contents |
|---|---|
| objects | every `.o`, `.pico`, `.po` and `.So` file by origin: `rucc-c`, `rucc-asm`, `ref-cxx`, `ref-c` (bring-up delegations only, 00), `cxx-on-cc` (10.1), `other` (objects made by `ld -r`, `objcopy` or `ar` only) |
| linked files | for every installed executable, library, kernel and module: bytes of `.text` from each origin, through the link map (`-Wl,-Map`), which `rb-cc` adds when it links. `-Map` changes no output |
| share | the share of `.text` bytes and of objects that came from rucc, for the kernel, for `/lib` and `/usr/lib`, and for the whole image |
| preprocessor | `XCPP` uses and where each output goes |
| foreign | anything with origin `other` or `ref-c`, by file |

**Rules.**
- `ref-c` must be empty in a graded build. A delegated unit makes the build ungraded.
- `cxx-on-cc` must be empty, or each entry must be justified by the project's own Makefile.
- The share is reported, not gated. It is the number that tells a reader how much of a "rucc-built FreeBSD" rucc built.

B0 runs the census on G builds, where every origin is `ref`, to get the inventory before rucc compiles anything.

## 10.5 The C-only rows

The `c-only` rows show how small the C++ side can be made with knobs each system supports.

**FreeBSD `c-only`** = `graded` + `WITHOUT_GOOGLETEST` + `WITHOUT_PMC`.

What remains as C++:
- the C++ runtime;
- the `libunwind` parts of `libgcc_eh` and `libgcc_s`;
- `kyua`, `lutok` and ATF's C++ library;
- `devd`, `libdevdctl`, `ctld`, `dtc` and `libutil++`;
- `zfsd`.

There is no knob for any of those that leaves a working, tested system, so they stay `g++`'s.

What it loses:
- **The googletest suites.** `tests/sys/fs/fusefs` and `tests/sys/capsicum` are C++ tests of C kernel code, which is the reason this row is not the headline.
- **`libpmc`, `libpmcstat`, `libipt` and `libopencsd`.** Those are mostly C.

`WITHOUT_ZFS` is not set. It would remove `zfsd`, but it also removes OpenZFS (`share/mk/src.opts.mk:484-485` and `cddl/usr.sbin/Makefile:10-13`): the kernel module, `libzfs`, `zpool`, `zfs` and `zdb`. That is a large body of C, and its tests.

`tests/sys/fs/Makefile:14` decides whether to build the fusefs tests from `COMPILER_FEATURES:Mc++14`. That is the **C** compiler's feature list (05.3), so a wrong identity in rucc silently removes C++ tests in R. `rb mk-diff` catches this. It is also why identity is a B1 item.

**NetBSD `c-only`** = `MKCXX=no`. `bsd.own.mk:1674-1680` then forces off:
- `MKATF`;
- `MKGCCCMDS`;
- `MKGDB`;
- `MKGROFF`;
- `MKKYUA`.

The built system has no tests and no compiler. It is tested this way:
1. `rb build` makes the `c-only` sets.
2. `rb image` installs them, then installs over them the `tests` set and the ATF and kyua libraries and programs from the **graded R build of the same pin** (12.3). It records each borrowed file in the census as `borrowed`.
3. The tests run as usual. A test that fails here and passes on the graded R image is a difference in a `c-only` file, because the test binaries are the same.

This is the only place sets from two builds are combined. It is allowed because the borrowed files are rucc-built C and `g++`-built C++ from a graded build, and they are listed.

**The share.** The `c-only` rows are where the share of rucc's `.text` is largest. 02's claim is about the `graded` rows. The `c-only` share is reported next to it so the reader can see how much the C++ side moves the number.

## 10.6 What is out of scope, and what would change it

**Out of scope:**
- compiling C++ with rucc;
- grading `g++`'s objects;
- the C++ standard library's correctness.

A `g++` miscompile shows up in G as well and is not rucc's.

**In scope**, because the C side is rucc's: C++ code calling rucc-built C, and the reverse. Specifically:
- struct layout across the language line (`bsd-abi`, `rb cross-modules`);
- exceptions through rucc frames (08.4);
- `extern "C"` linkage of every libc symbol;
- the C headers compiling in `g++` exactly as before. rucc does not touch the headers, but `bsd-predef` must not leak rucc-only macros into the C++ compiler, which runs its own preprocessor.

What would change this: a decision outside this folder to give rucc a C++ front end. If that happens, the `full` and `graded` rows already list exactly what it would have to compile, and 10.4's census is the work list.
