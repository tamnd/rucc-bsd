# Sibling repositories

The BSD work touches every repository in the family. The rule is the same as the kernel's: fixes go to `rucc`, tools go to `rucc-bsd`, reductions go to the corpora. This document lists what each one gains, and at which milestone of document 16.

## 15.1 `tamnd/rucc`

All compiler work in documents 03 and 05 to 11.

| Area | Issues to open at B0 | Milestone |
|---|---|---|
| BSD targets (03.1, 08.1) | the four operating systems in `rucc-target::Os`; predefined macros against the reference GCC's `-dM -E`; the link line, start files, dynamic linker path and default libraries per system; PIE and PIC defaults per system; `-freg-struct-return` and the other i386 defaults that differ from Linux; ELF OSABI and the ABI note handled by the system's own `crtbrand`; tier 4 to tier 2 in `docs/TARGETS.md` | B1 |
| rucc as a BSD host | rucc builds and passes its own test suite on FreeBSD amd64, with a release artifact | B1 |
| file-scope `asm` (07.3) | `.weak` across blocks, `.ident`, `.pushsection` flags and type; or the rule that any block the reader cannot translate exactly goes to the assembler | B1 |
| driver surface (05) | `-finline-limit=`; `-fdiagnostics-show-option`; `-mrtd` and `--param min-pagesize=` for FreeBSD's `boot2` (03.4); the rest of the `rb probes` list; `--sysroot` and `-B` as `buildworld` uses them; `-print-prog-name=` for `ld` | B1, B2 |
| formats (06.6) | the `freebsd_kprintf` format with `%b`, `%D`, `%r` and `%y`, accepted when the reference accepts it | B2 |
| warning parity (06.9) | every warning rucc gives under `-Werror` is one the reference gives on the same unit | B2, then continuous |
| kernel flags (08.2) | FreeBSD's and NetBSD's GCC kernel flag lines accepted, with the kernel code model, no FPU, the stack protector and `-fno-common` | B3 (NetBSD), B5 (FreeBSD) |
| boot code (08.6) | the BIOS boot blocks: `-m32`, `.code16`, `-mrtd` with `-mregparm=3`, `-Os` size limits that are hard errors in FreeBSD's `boot2` and NetBSD's `bootxx` | B4 (NetBSD), B6 (FreeBSD) |
| debug information (09.6) | DWARF that `ctfconvert` reads into the same CTF as the reference's; `-gdwarf-4` and `-gdwarf-5` as each system asks | B3 (NetBSD), B5 (FreeBSD) |
| libc shapes (06, 08.5) | symbol versioning through `.symver` and version scripts, `ifunc` resolvers in a static binary, TLS in `rtld` before relocation, `__thread` with every model | B4 (NetBSD), B6 (FreeBSD: versions and `ifunc`) |
| unwinder builtins (03.14) | `__builtin_eh_return`, `__builtin_unwind_init`, `__builtin_dwarf_cfa`, `__builtin_dwarf_sp_column` and `__builtin_init_dwarf_reg_size_table`, so NetBSD's `libgcc_eh` and `libgcc_s` build | B4 |
| feature-test macros (03.13) | `__has_extension` and `__has_feature` answers equal to the reference's, and absent under personas that lack them | B1 |
| aarch64 property note (11.2) | the BTI, PAC and GCS bits equal to the reference's for each `-mbranch-protection` value | B7 |

Two changes to rucc's own spec follow from this folder, proposed as spec edits at B0:

1. **The BSD rows of `spec/cross-compile/04-target-matrix.md`** move from plan tier 2 and 3 to plan tier 1 for `x86_64-freebsd` and `x86_64-netbsd`, and tier 2 for `aarch64-freebsd`, `aarch64-netbsd` and `riscv64-freebsd`. The table's "stub libraries, as zig does" stays true for linking user programs against a FreeBSD that is not there, and becomes untrue for the base system, which links against the libc it just built.
2. **Rung 4 of the target ladder** (parent document 14.5) names "the Linux kernel". It gains a sentence: "and a BSD kernel and base system, `tamnd/rucc-bsd`", with the levels of this folder: level A is B3 and B4 (NetBSD amd64 kernel and world boot and pass), level B is B5 and B6 (FreeBSD amd64), level C is B7 and B8 (aarch64 and riscv64, and performance).

## 15.2 `tamnd/rucc-compat`

Differential corpora, comparing rucc with the reference GCC on the same input:

| Corpus | Contents | From |
|---|---|---|
| `bsd-predef` | `-dM -E` of an empty file for every BSD target and every `-m` and `-std` combination the builds use, against the reference GCC for that target. The expected list is generated on the builder, not written by hand | B1 |
| `bsd-abi` | struct layouts of every installed header under `/usr/include` of each system, as `sizeof`, `alignof` and `offsetof` probes compiled by both compilers. This is the largest single ABI check the project will have: FreeBSD installs about 2,000 headers | B1 |
| `bsd-probes` | the compiler probes of `bsd.compiler.mk`, `kern.pre.mk`, `bsd.sys.mk` and NetBSD's `bsd.own.mk` as a standalone differential, imported from `rb probes` | B2 |
| `bsd-pp` | `-E` output of about 300 units per system, chosen by `rb demands` to cover every header and every macro family | B2 |
| `bsd-asm` | every `.S` file of both systems' kernels, libc and boot code, preprocessed and assembled by GNU as and by rucc, compared section by section. Stored as a generator and a manifest, like `kernel-asm` | B3 |

`kernel-asm` already has the machinery for the last one. The difference is the macro packages: `<machine/asm.h>` and its `ENTRY`, `END`, `WEAK_REFERENCE` and `PIC_PLT` on FreeBSD, and `<machine/asm.h>` with `_ENTRY`, `RCSID`, `STRONG_ALIAS` and `WEAK_ALIAS` on NetBSD.

## 15.3 `tamnd/rucc-corpus`

Reduced tests, each a small program with an expected result. New facets, each motivated by a named file in a named tree, cited in a comment and never copied:

| Facet | Tests | From |
|---|---|---|
| `file-scope-asm` | `.weak` then `.equ` in two blocks; `.ident`; `.pushsection` with flags and type `@note`; `.symver` with `@` and `@@`; `.section` and `.previous`; every one of them checked against GNU as on the same text | B1 |
| `linker-sets` | FreeBSD `DATA_SET` and `SET_DECLARE` and NetBSD `__link_set_add_data` walks, with `__start_` and `__stop_`, `used`, `retain`, and the alignment that keeps a set an array of pointers | B1 |
| `kprintf-format` | `%b`, `%D`, `%r`, `%y` with the warnings the reference gives for each, and none it does not | B2 |
| `werror-shapes` | one test per warning class rucc gave and the reference did not, found by `rb warnings`, with the expected silence | B2 |
| `crt-and-brand` | the ABI note sections, `.init_array` order with priorities, `_init` and `_fini` in `crti` and `crtn`, a program with no libc | B1 |
| `symbol-versions` | default and hidden versions, a versioned `alias`, compatibility symbols built through `__sym_compat` | B6 |
| `ifunc-static` | resolvers in static binaries run by `__libc_start1` relocation processing before `main`, as FreeBSD's `libc` does for `memcpy` and its family, and NetBSD's `crt0-common.c` does | B4 (NetBSD), B6 (FreeBSD) |
| `tls-models` | the four TLS models, TLS in `rtld` itself, TLS variant II on amd64 and variant I on aarch64 and riscv64 | B4 |
| `boot-sizes` | functions built with the boot blocks' `-Os` flags, where the expected result is a size bound from the reference plus a margin | B4 |
| `netbsd-kernel` | the NetBSD kernel's flag line, `__link_set`, `__KERNEL_RCSID` | B3 |
| `unwinder` | C++ exceptions thrown through rucc-built C frames, with a rucc-built `libgcc_eh` and with the reference's; `pthread_cancel` unwinding; `cleanup` attributes during an unwind (08.4) | B4 |
| `boot-abi` | `-mrtd` with `-mregparm=3` as defaults, called from assembly that assumes them, as in FreeBSD's `boot2` | B6 |

## 15.4 `tamnd/rucc-real-corpus`

- **Add the C programs both base systems take from outside**, as real-corpus projects built on Linux, before they meet the BSD build: `bmake`, `libarchive`, `libedit`, `byacc` (already there), `nvi`, `less`, `xz` (already there), `zstd` (already there), `bzip2` (already there), `sqlite` (already there), `libucl`, `libxo`, `openssh-portable`, `unbound`, `ntp`, `tcsh` and `bc`. A failure in one of them is then found in a cheap harness, not in a five hour world build. The pinned version of each follows the newest graded FreeBSD tree's `contrib/` copy.
- **Add OpenZFS** as a userland project (`libzfs`, `zfs`, `zpool`, `ztest`). It is the largest single contributed C component in FreeBSD's kernel and world, and `ztest` is a strong correctness test that runs in user space.
- **Keep the BSDs themselves out.** They are graded here, with their own harness, as Postgres and the kernel are.

## 15.5 `tamnd/rucc-cross`

- It already carries `x86_64-freebsd`, `aarch64-freebsd`, `x86_64-netbsd`, `aarch64-netbsd` and `x86_64-openbsd` facts, taken from zig 0.16.0. B1 checks every one of them against the reference GCC on the builder, and any difference is a bug in the facts file before it is a bug in rucc.
- **Sysroots for user programs.** A FreeBSD sysroot made from the release's `base.txz` (headers, `crt*.o`, and `libc.so` as a linker script with the real library), and the same for NetBSD from its `base.tgz` and `comp.tgz`. These are what rucc links against when it builds a program for a BSD from a Linux host, as it does for Linux targets today. They are not used by the base system builds, which build their own.
- **Run rucc's own output** under the BSDs: the existing "run rucc's output under qemu" job gains FreeBSD and NetBSD rows, booting the stock release and running the rucc test programs inside it. This is how B1 shows that a rucc program runs on a stock BSD before anything in this folder depends on it.

## 15.6 `tamnd/rucc-harness` (new, small)

The kernel folder copied the shim schema, the SSH job agent, the report writer and the benchmark statistics from `rucc-postgres` into `rucc-kernel`. A third copy is one too many. B0 moves them into a new workspace, `tamnd/rucc-harness`, with four crates:

| Crate | Contents | Users |
|---|---|---|
| `harness-shim` | the `compile.jsonl` record and the code that writes it, probe detection, `*_TWICE` | rpg, rk, rb |
| `harness-agent` | the SSH job runner and the scheduler | rpg, rk, rb |
| `harness-stats` | medians, interquartile ranges, ratio intervals and the rules of `rucc-kernel` 12 for what may be claimed | rpg, rk, rb |
| `harness-elf` | section and symbol diffs and the vector audit, which `rk` has and `rb` needs on a different object layout | rk, rb |

`rk` and `rpg` move to it at their own pace. `rb` starts on it. The repository has no knowledge of any system or program, so it does not violate any harness's rules.

## 15.7 `tamnd/rucc-kernel`

Nothing changes in its rules. Two things flow between it and this folder:

- **Compiler features.** Most of what a BSD kernel needs from rucc, the kernel code model, no FPU, the stack protector guard, `.code16`, i686, `asm goto`, the macro assembler, `-fstack-usage`, was built for Linux and is reused here. The census of 14.5 will show what is left, and 03 estimates it.
- **The Linux kernel's own BSD dependency.** None. The BSDs never build Linux code. The flow is one way.

## 15.8 `tamnd/rucc-postgres`

From B10, PostgreSQL gets a FreeBSD amd64 row, built by rucc on the rucc-built FreeBSD, using Postgres's own `meson` build. PostgreSQL's build farm has had FreeBSD members for decades, so this is a familiar configuration, and it is the first program other than the base system to run on the rucc base system with rucc as the system compiler.
