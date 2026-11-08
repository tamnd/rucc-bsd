# Build systems and identity

A BSD base system is built by a make system written for itself, about thirty years old, and full of decisions keyed on which compiler is in use. Before rucc compiles a line of C, the build has identified it, chosen flags for it, decided which programs to build, and sometimes refused to continue. This document is about that phase: how each system finds and identifies its compiler, how rucc is put in place without a patch, and how `rb` proves that R and G were configured the same (02.2).

## 05.1 FreeBSD's build, as a compiler sees it

`make buildworld` runs in stages. Each stage uses a different compiler:

| Stage | Builds | Compiler |
|---|---|---|
| `_bootstrap-tools` | `bmake`, `m4`, `lex`, `yacc` and others, for the host | host `cc` |
| `_build-tools` | generators run during the build | host `cc` |
| `_cross-tools` | normally the bootstrap clang and lld. Skipped when `XCC` is given | none in our builds |
| `_includes`, `_libraries` | headers into `WORLDTMP`, then every library | `XCC` and `XCXX` |
| `everything` | every program | `XCC` and `XCXX` |
| `lib32` | the 32-bit compatibility libraries, with `-m32` | `XCC` and `XCXX` |

Then `make buildkernel` runs:
1. `config(8)`, a C++ program built for the host in `_build-tools`;
2. `make obj depend`;
3. `genassym`, which compiles C and reads it with `nm`;
4. the kernel objects;
5. the link;
6. `ctfconvert` and `ctfmerge`;
7. the modules.

Every compiler call in steps 3 to 7 is `${CC}`, which is `XCC` here.

**What happens with an external GCC.** With `X_COMPILER_TYPE=gcc`, `Makefile.inc1:900` passes no `-target` flag. The compiler must target FreeBSD on its own, as a `x86_64-unknown-freebsd15.1-gcc15` does. It always passes `--sysroot=${WORLDTMP}`. When `CROSS_BINUTILS_PREFIX` is not set, it also passes `-B${WORLDTMP}/usr/bin`. Later, `Makefile.inc1:4026` adds `-isystem` and `-L` for GCC when building the compiler-dependent parts.

So rucc must:
- be invoked under a name, or with a fixed flag, that implies the FreeBSD target and version;
- treat `--sysroot` as GCC does, for headers, start files and libraries;
- look for `ld` through `-B` as GCC does.

## 05.2 NetBSD's build, as a compiler sees it

`build.sh` builds a complete cross toolchain from the tree into `TOOLDIR` (`build.sh tools`). Everything after that uses it:

| Stage | Builds | Compiler |
|---|---|---|
| `tools` | `nbmake`, `nbmakefs`, `nbinstall`, `nbconfig`, binutils, and GCC as `x86_64--netbsd-gcc` | host `cc` and `c++` |
| `distribution` | headers, libraries including `libgcc` when `MKGCC=yes`, and every program | `TOOL_CC.gcc` and `TOOL_CXX.gcc` from `TOOLDIR` |
| `kernel=GENERIC` | `nbconfig`, then `genassym` (via `-S` and a template), the objects, the link, `ctfconvert` | same |
| `release` | the sets, the install images, the ISO | `nbmakefs`, `nbinstallboot` |

`bsd.own.mk:436` assigns `TOOL_CC.gcc= ${TOOLDIR}/bin/${MACHINE_GNU_PLATFORM}-gcc` unconditionally when `EXTERNAL_TOOLCHAIN` is unset, and `bsd.own.mk:748` sets `CC=${TOOL_CC.${ACTIVE_CC}}`.

**`EXTERNAL_TOOLCHAIN` is the obvious door, and it is the wrong one.** It forces `MKGCC:=no` (`bsd.own.mk:962`). The reason, in the file's own words, is that "we cannot build GCC's support libraries, since those are tightly-coupled to the version of GCC being used". The R build would then have no `libgcc`, `libgcc_eh` or `crtbegin.o` at all, and G with the in-tree GCC would. R and G would be configured differently by construction.

So rucc goes in through `TOOLDIR` instead, in 05.4. NetBSD then builds `libgcc` and the crt files from the tree's GCC sources, with rucc as the compiler. That is graded C like any other, and it is the GCC runtime that GCC's own objects in the base would link against too.

## 05.3 Identity and probes

### FreeBSD

`share/mk/bsd.compiler.mk` runs on every make invocation that does not inherit a cached answer, with one `${CC} --version` and one `${CC} -E -` each time. It answers four questions:

1. **Type** (lines 196-217):
   - the basename matching `*gcc*` or `*clang*`;
   - else a `--version` word equal to `gcc`, `(GCC)` or ending in `GNU`, or `clang`;
   - else `gcc version` in `${CC} -v`;
   - else an error.

   `X_COMPILER_TYPE` set in the toolchain file skips all of this for `XCC`.
2. **Version** (line 222). Every `--version` word matching `[1-9]*.[0-9]*` goes to `awk -F. '{print $1*10000+$2*100+$3}'`.
3. **FreeBSD version.** `echo __FreeBSD_cc_version | ${CC} -E -`. A literal echo means not FreeBSD's own clang, which is the right answer for GCC and for rucc.
4. **Resource directory.** `${CC} -print-resource-dir`, with failure tolerated.

The version then decides:
- **`COMPILER_FEATURES`** (01.2): `c++17` at GCC 7 or later, `c++20` at 10.1, `init-all` at 12, `fileprefixmap` at 8.1, `zeroregs` at 11.
- **The warning blocks of `bsd.sys.mk`.** These are the `-Wno-error=` demotions at 5.2, 6.1, 7.1, 8.1, 9.2, 12.1 and 13.1.
- **`kern.mk`'s `FORMAT_EXTENSIONS`** (03.5).

`bsd.linker.mk` does the same for `ld -v`. In R and G the linker is the same BFD `ld`, so it gives the same answer.

**What rucc prints.** Under a persona, rucc's `--version` must be readable by this code by construction:
- The first line is `gcc (rucc 0.30.15, GNU C persona 15.2.0) 15.2.0`. That gives type gcc from the word `gcc`, and the version from `15.2.0`.
- **No other word anywhere in the output may match `[1-9]*.[0-9]*`.** Today the license line's "Apache License 2.0." matches. Without a persona it is the only match, and the version comes out as GCC 2.0.0 (03.3). The fix is to say "Apache License, version 2" or to drop the line under a persona. The `awk` arithmetic should never again be what makes the answer right.
- `-print-resource-dir` prints a directory rucc owns, rather than being refused.

The kernel also records the compiler. `sys/conf/newvers.sh:210` takes `${CC} -v 2>&1 | grep -w version` into `kern.compiler_version` and the `uname -v` string. rucc's `-v` line is `rucc version 0.30.15 (gcc version 16.0.0 compatible)`. Under a persona it must say the persona's version. This string is one of the declared differences in `mk-divergences.toml`, because naming itself is the one thing rucc's identity must do differently.

### NetBSD

NetBSD never asks the compiler what it is:
- `HAVE_GCC?=14` on trunk (`bsd.own.mk:93`) is the identity;
- the `CC_WNO_*` variables (`bsd.own.mk:741-785`) are chosen by its value;
- the kernel's GCC-only flags of 03.4 come from the same place.

The persona's job on NetBSD is narrower:
- `__GNUC__` and `__GNUC_MINOR__` must match the tree's GCC, because headers and contributed code test them;
- the warning set must match (04.3, 06.9).

The `--version` text matters only to `build.sh`'s host compiler check (`build.sh:1710`), which looks at the host compiler, not at rucc.

### DragonFly (B11)

The identity is the `CCVER` name. `gcc120` gives the GCC warning sets and `-Werror`. rucc is installed as a custom `CCVER`, `rucc120` or similar, through `compilers.conf`. Whether `bsd.sys.mk`'s `${_WCCVER:Mgcc*}` then still matches is a B11 question. If it does not, the build takes the non-GCC path, which is configured differently from G and is not acceptable. Open question 10.

### The probe census

`rb probes` (14.3) runs every probe the build makes against both compilers, without building. It collects the probes from the shim's records of a G build, and answers "same" or "differs" for each one:
- the `--version` and `-E` probes above;
- the `bsd.compiler.mk` feature tests;
- every other call the shim classifies as a probe (05.8), wherever in the tree it comes from. Both systems ship contributed software with pre-generated configuration, so few such probes are expected, but how many there are is a B0 measurement, not an assumption.

A differing probe is a B2 bug or a divergence with a reason.

## 05.4 Putting rucc in place

### FreeBSD: `conf/freebsd/rucc.mk`

The port's `xtoolchain.mk` is the template. G's file, generated by `rb` from the installed package, is:

```make
XCC=/usr/local/rb/bin/x86_64-unknown-freebsd15.1-rb-cc-ref
XCXX=/usr/local/rb/bin/x86_64-unknown-freebsd15.1-rb-c++
XCPP=/usr/local/bin/x86_64-unknown-freebsd15.1-cpp15
CROSS_BINUTILS_PREFIX=/usr/local/x86_64-unknown-freebsd15.1/bin/
X_COMPILER_TYPE=gcc
```

R's file differs in one line:

```make
XCC=/usr/local/rb/bin/x86_64-unknown-freebsd15.1-rb-cc-rucc
```

Each `rb-cc-*` is a hard link to the same shim binary. The shim reads its own name to find:
- the compiler: rucc, or the reference `x86_64-unknown-freebsd15.1-gcc15`;
- the fixed arguments: for rucc, `--target=x86_64-unknown-freebsd15.1 -fgnuc-version=15.2.0`.

Both names contain neither `gcc` nor `clang` as a basename match. That is deliberate. It makes `X_COMPILER_TYPE=gcc` the only route to the type, the same for R and G.

`XCPP` stays the reference's preprocessor in both at first, and `rb census` counts its calls (10.4). Whatever `CPP` preprocesses in FreeBSD's build, such as RPC interface definitions and linker scripts, is not compiled C, so this is not where rucc's preprocessor is graded; that happens through `XCC`. If the census shows `XCPP` output feeding C units, B2 moves `XCPP` to the rucc shim in R, as NetBSD's `cpp` slot is from the start.

### NetBSD: a `TOOLDIR` overlay

1. `build.sh -T $TOOLDIR_G tools` builds the tools once per pin.
2. `rb` makes `TOOLDIR_R` as a copy of `TOOLDIR_G` in which exactly two files differ:
   - `bin/x86_64--netbsd-gcc` is the shim, running rucc with `--target=x86_64-unknown-netbsd11.99 -fgnuc-version=14.4.0`;
   - `bin/x86_64--netbsd-cpp` is the shim in `-E` mode.

   In `TOOLDIR_G`, the same two names are the same shim running the real `nbgcc`. That keeps both builds recorded the same way.
3. Both builds run `build.sh -T <dir> -u -O <obj> distribution kernel=GENERIC release`. The `-u` flag tells `build.sh` not to rebuild the tools.

`x86_64--netbsd-c++` is untouched in both. The `cpp` slot is rucc in R here, unlike FreeBSD. `TOOL_CPP` is a NetBSD tool slot in `bsd.own.mk:437` like `TOOL_CC`, and whatever uses it gets rucc's preprocessor, which is the stricter choice.

### `rb toolchain-diff`

Before every graded build, `rb toolchain-diff` checks the toolchain descriptions:
- **FreeBSD:** the two `.mk` files differ only in `XCC`.
- **NetBSD:** the two `TOOLDIR`s differ only in the two shim names' targets. Every other file is identical by hash.

It also checks that each shim's target compiler is the one `toolchains.toml` and the rucc pin name. A failed check stops the build.

## 05.5 `rb mk-diff`

`rb mk-diff` runs `make -V` for the variables of 02.2 at the points where they are computed:
- the top level;
- one library;
- one program;
- the kernel build directory;
- the `lib32` and `stand` subtrees on FreeBSD.

It runs them in R's and G's trees with their toolchain descriptions, and prints the differences. It also compares:
- **FreeBSD:** `make showconfig`, which is every `MK_*` after the compiler-dependent overrides, such as `src.opts.mk` marking `LIB32` broken on aarch64 for GCC.
- **NetBSD:** `make -V` of every `MK*` and `HAVE_*` from `bsd.own.mk`.
- **Both:** the kernel's generated `opt_*.h` files.

The output is grouped by cause. A difference is either:
- in `mk-divergences.toml` with a reason, such as the resource directory or the `kern.compiler_version` string; or
- a bug.

## 05.6 `rb flags-diff`

The shim records every unit's argument vector in both builds. `rb flags-diff` pairs units by output path and compares the vectors with three things normalized:
- the compiler path;
- the persona flag;
- the target flag rucc's shim adds.

Anything else that differs means a make decision went differently. It is traced back to the variable through `make -dv` on that one directory, and reported with the variable's name. This is the per-unit form of `mk-diff`, and it catches what `mk-diff` cannot see, such as a per-directory `CFLAGS.gcc` or a `.if ${COMPILER_VERSION} >= ...` buried in a leaf `Makefile`.

## 05.7 `rb syntax`

`rb syntax` replays every C unit of a G build through rucc with `-fsyntax-only` and the unit's exact flags, from the shim's records. For `.S` files it replays them with `-E`. It runs in parallel and needs no link, no image and no VM. It finds three kinds of failure:
- front end failures;
- refused flags;
- warnings that would be errors.

Each failure is reported with the unit and its first diagnostic. B2's exit criterion is that it passes for every C unit of both systems' graded configurations (16.3).

## 05.8 Probes are not units

FreeBSD's `bsd.compiler.mk` alone runs a `--version` and an `-E` probe per make invocation without a cached answer, and a world build makes tens of thousands of make invocations. The shim marks a call as a probe when its output is `/dev/null`, a pipe, or a file in a temporary directory, or when it has no input file. Probe calls are:
- counted separately;
- excluded from the compile-time measures of 13.4;
- compared by `rb probes`.

A probe that takes rucc longer than 20 ms is a performance bug, because the build makes so many of them.
