# Where rucc stands

This is the gap inventory on 8 November 2026, against rucc 0.30.15 at `tamnd/rucc` main `a0d77899d`. Every claim here was checked one of three ways:
- with a release build of that commit (`/tmp/rucc-bsd-probe/release/rucc`);
- against clang 21 and the BSD source trees cloned on 7 November 2026;
- by reading the named file in rucc.

Nothing was built on a BSD yet, because rucc cannot target one (03.1).

Sizes follow `rucc-kernel` 03:

| Size | Engineer time, including corpus cases and reductions |
|---|---|
| **S** | under 3 days |
| **M** | 1 to 2 weeks |
| **L** | 3 to 6 weeks |
| **XL** | more than 6 weeks, or a new subsystem |

## 03.0 The headline

The C front end, the optimizer, the x86-64 and AArch64 back ends, the integrated assembler and the kernel features built for `rucc-kernel` are already most of what a BSD needs. Five things are missing, and a sixth is wrong:
- the operating systems themselves;
- a faithful reading of file-scope `asm`;
- the compiler identity FreeBSD parses;
- one FreeBSD-specific format checker;
- warning parity with the reference under `-Werror`.

Two more were found while writing this folder. rucc answers the feature-test macros that both systems' headers use differently from GCC (03.13). And it lacks the builtins GCC's own unwinder is written in, which NetBSD compiles with `${CC}` (03.14). None of them is large. The large unknowns are elsewhere, and only a build can show them. They are code generation bugs that only 30 million lines of new C will reveal, and the base system's dependence on the C++ compiler, which document 10 fences off.

| Area | State | Size | Milestone |
|---|---|---|---|
| BSD targets (03.1) | refused | L | B1 |
| file-scope `asm` (03.2) | three wrong-output bugs | M | B1 |
| compiler identity (03.3) | FreeBSD reads rucc as GCC 2.0.0 | S | B1 |
| driver flags (03.4) | two GCC kernel flags and two `boot2` flags refused | S | B1 |
| `freebsd_kprintf` (03.5) | refused, an error | M | B2 |
| warning parity (03.6) | unmeasured | L, then continuous | B2 |
| linker sets, symbol versions, `ifunc`, `genassym` (03.7) | work | none | none |
| kernel flag lines (03.8) | accepted | S | B3 |
| boot code (03.9) | `.code16`, `-m32`, i686 exist; size unmeasured | M | B4, B6 |
| debug information for CTF (03.10) | unmeasured | M | B3 |
| rucc as a FreeBSD host (03.11) | not built | S | B1 |
| the C++ boundary (03.12) | out of scope, by design | none in rucc | none |
| `__has_extension` and `__has_feature` (03.13) | answer 0 where GCC answers 1; defined under personas older than 14 | S | B1 |
| unwinder builtins (03.14) | `__builtin_eh_return`, `__builtin_unwind_init`, `__builtin_dwarf_cfa`, `__builtin_dwarf_sp_column` unknown | M | B4 |

## 03.1 BSD targets are refused

Every BSD triple fails before the first token is read:

```
$ rucc --target=x86_64-unknown-freebsd15.0 -c t.c
rucc: error: unsupported target triple `x86_64-unknown-freebsd15.0`: unknown operating system
```

The same happens for:
- `x86_64-freebsd` and `x86_64-freebsd15`;
- `aarch64-freebsd` and `i686-freebsd`;
- `x86_64-netbsd` and `x86_64-openbsd`.

The pieces that exist are scattered:
- **`rucc-tuple`** parses `freebsd`, `netbsd` and `openbsd` in `crates/rucc-tuple/src/os.rs`. Its comments already state the support floors ("FreeBSD 14 and later", "10.1 and later", OpenBSD dynamic libc only).
- **`rucc-stub`** knows the ELF OSABI values in `crates/rucc-stub/src/elf.rs`: NetBSD 2, FreeBSD 9, OpenBSD 12.
- **`rucc-cross`** has facts files for `x86_64-freebsd`, `aarch64-freebsd`, `x86_64-netbsd`, `aarch64-netbsd` and `x86_64-openbsd`, taken from zig 0.16.0. For x86-64 FreeBSD they say:
  - `char` is signed;
  - `long double` is x87 extended in 16 bytes;
  - `wchar_t` is a signed `int`.

What is missing:
- **The OS enum.** `rucc-target`'s `Os` enum (`crates/rucc-target/src/lib.rs:159`) has `Linux`, `Darwin`, `Windows`, `None` and `Wasi` only. The target layer is what decides the predefined macros, the ABI details and the link line.
- **The predefined macros.** `crates/rucc-pp/src/predef.rs` has no `__FreeBSD__` (GCC defines it to the major version, such as 15), `__NetBSD__`, `__OpenBSD__` or `__DragonFly__`.

The full list of what a target means for each system is document 08.1. The work is:

| Piece | Size |
|---|---|
| `Os::FreeBsd(major)` and `Os::NetBsd(major)` in `rucc-target`, threaded through ABI queries | M |
| predefined macros against the reference's `-dM -E`, per `-m` and `-std` (`bsd-predef`, 15.2) | S |
| the driver's link line per system: start files, dynamic linker, default libraries, `-pie` defaults | M |
| OpenBSD and DragonFly in the enum, unused until B11 | S |

**Total L.** None of it is novel, and Darwin and WASI show the pattern. The risk is the long tail of small ABI choices, which `bsd-abi` (15.2) exists to find.

## 03.2 File-scope `asm`: three wrong-output bugs

rucc has a fast path for file-scope `asm` blocks that contain only directives. `crates/rucc-lower/src/directives.rs` reads such a block and turns it into global declarations and linkage changes, without running the assembler. When a block holds something the reader does not know, the file goes through the integrated assembler instead, and that path is correct. The fast path has three bugs. All three give a wrong object without an error, which makes them the worst kind for a base system.

**Bug A: `.weak` and `.equ` in separate blocks make a local symbol.** FreeBSD's `__weak_reference` (`sys/sys/cdefs.h:423`) expands to two blocks:

```c
__asm__(".weak " #alias);
__asm__(".equ "  #alias ", " #sym)
```

| | rucc | clang and GNU as |
|---|---|---|
| `nm` of `__weak_reference(_real, real)` | `t real` | `W real` |

The `.weak` sets linkage on a name that does not exist yet (`directives.rs:436`), and the later `.equ` (`directives.rs:443`) makes a fresh local equate. The result is the same whether the blocks come before or after the definition. The same two directives in one block give the correct `W`.

FreeBSD uses `__weak_reference` in 264 files of `lib/` and `sys/`. libc's whole `_foo`/`foo` namespace scheme depends on it, as do libthr's interposition table and libc's `__sys_` aliases. A libc built this way links, since `real` is local. Every program that calls the weak name then fails to link or binds to the wrong definition.

**Bug B: a file-scope `.ident` is dropped.** `directives.rs:442` treats `.ident` as a no-op, alongside `.type`, `.file` and `.cfi_sections`. `__asm__(".ident \"$FreeBSD: x $\"")` produces a `.comment` section holding only `rucc 0.30.15`. FreeBSD's main branch no longer expands `__FBSDID` (it is `struct __hack` in `sys/sys/cdefs.h:466`), so FreeBSD is affected only in contributed code. Other systems' headers still emit `.ident` this way. `what(1)` and `ident(1)` read these strings, and so does `rb sections-diff`.

**Bug C: `.pushsection` loses its flags and type.** A block such as

```c
__asm(".pushsection .ident,\"MS\",@progbits,1\n.asciz \"$NetBSD: x.c,v 1.1 $\"\n.popsection");
```

gives `.ident` as `PROGBITS WA` (writable, allocated, no merge) where GNU as and clang give `PROGBITS MS`. In the same way, `.pushsection .note.netbsd.ident,"a",@note` gives `PROGBITS WA` where they give `NOTE A`. The same text inside a function's `asm`, or a top-level `.section` read by the assembler, is correct. So the fault is in the reader at `directives.rs:424`.

This is NetBSD's `__RCSID` and `__KERNEL_RCSID`, through `__SECTIONSTRING` (`sys/sys/cdefs_elf.h:139`). They appear in 3,256 userland files and 6,795 kernel files. Every one of those objects would put its RCS string in a writable allocated section. That moves `.ident` into the data segment of every NetBSD program and the kernel, and gives the linker input sections of one name with different flags, mixed with the correct ones from `.S` files and from `g++`. The `.note.netbsd.ident` form is how NetBSD's start files brand an executable. A note that is not `SHT_NOTE` does not reach a `PT_NOTE` segment, and the kernel's ELF probe then may not recognise the program as a NetBSD one (09.2).

**Why the bugs hid until now.** A file with a block the reader cannot parse goes entirely through the assembler. NetBSD's `__weak_alias` writes `".weak alias\nalias = sym"`, and the reader does not handle `=`, so a file with one `__weak_alias` and one `__RCSID` comes out right. Linux uses `asm` at file scope mostly inside functions or in `.S` files, so `rucc-kernel` never reached this path.

**The fix (07.3).** The reader translates a block only when it can translate every directive exactly, including flags, types, entry sizes, groups, `.weak` across blocks and `.ident`. Otherwise it hands the block to the assembler. The rule needs a differential test, which `bsd-asm` and the `file-scope-asm` facet (15.2, 15.3) provide. **M.** These are not filed yet; B0 files them as three issues, with the reproducers above, into `rucc-corpus`.

## 03.3 FreeBSD reads rucc as GCC 2.0.0

`share/mk/bsd.compiler.mk` identifies a compiler from its `--version` text (05.3). It takes every word matching `[1-9]*.[0-9]*` and passes them to `awk -F.`. rucc's default banner is:

```
rucc 0.30.15
A C compiler for the GNU C dialect of GCC 16 from the Free Software Foundation.
This is free software under the Apache License 2.0. There is NO warranty.
```

`0.30.15` starts with 0, and `16` has no dot. The only matching word is `2.0.`, from the license line. The result:
- the type is gcc, because a word ends in `GNU`;
- `COMPILER_VERSION` is 20000, which is GCC 2.0.0.

`bsd.sys.mk` then skips every warning block gated on GCC 5.2 and later. That includes the `-Wno-error=` demotions FreeBSD relies on under GCC (06.9). `kern.mk` picks `-fformat-extensions` (the pre-12.1 path), which no GCC since 4.2.1 accepts.

With the persona, `rucc -fgnuc-version=15.2.0 --version` prints `gcc (rucc 0.30.15, GNU C persona 15.2.0) 15.2.0` and the same license line. The matching words are `15.2.0)`, `15.2.0` and `2.0.`, and `awk` happens to compute 150200. That is correct, but only because `awk` reads `0) 15` as 0. The fix makes the banner correct by construction, not by luck (05.3):
- under a persona, no word other than the version may look like a version;
- `-print-resource-dir` is accepted;
- `echo __FreeBSD_cc_version | rucc -E -` echoes the name back, as GCC's does.

**S.**

## 03.4 Driver flags

The FreeBSD kernel's GCC flag line (`sys/conf/kern.pre.mk:81-85`) was run through rucc with refused flags removed one at a time:

| Flag | rucc | Note |
|---|---|---|
| `-finline-limit=8000` | refused | `CFLAGS.gcc`, every FreeBSD kernel unit. GCC maps it to `max-inline-insns-single` and `max-inline-insns-auto` |
| `-fdiagnostics-show-option` | refused | GCC's default since 4.6, still accepted |
| `--param inline-unit-growth=100`, `--param large-function-growth=1000` | accepted | `--param` is parsed in `crates/rucc-driver/src/lib.rs:1147` |
| `-mretpoline` | refused | clang only, and FreeBSD sends it only to clang. Correct |
| `-mrtd` | refused | FreeBSD `stand/i386/boot2/Makefile:21`, with `-mregparm=3`, which is accepted. It makes `stdcall` the default convention; rucc already implements `stdcall` (`ret $8` probed), so only the flag is missing |
| `--param min-pagesize=1024` | refused | `CFLAGS.gcc` of `boot2` (`:34-37`). It tells GCC's warnings that addresses below 1024, not below 4096, are null-like, so low-memory BIOS pointers do not warn. rucc must accept it and apply it in its own null-address warnings |

Accepted with no change:
- **NetBSD's whole amd64 kernel line:** `-std=gnu11 -ffreestanding -fno-zero-initialized-in-bss -fno-strict-aliasing -fno-common -fno-delete-null-pointer-checks -fstack-usage -Wstack-usage=3584 -ftrivial-auto-var-init=pattern -mindirect-branch=thunk -mindirect-branch-register -mcmodel=kernel -mno-red-zone -mno-mmx -mno-sse -mno-avx -msoft-float -fstack-protector-strong -Werror`. The `.su` file came out in GCC's format.
- **The 45 `-Wno-error=` and `-Wno-` flags FreeBSD's `bsd.sys.mk` adds for GCC 5.2 to 13.1 (`share/mk/bsd.sys.mk:178-275`).**

One more thing to fix: on the Linux target, `-mcmodel=kernel` needs `-fno-pie`, because rucc defaults to PIE there. A BSD target must take the reference GCC's default for that system, whatever it is, and the build systems then add `-fPIE` or `-fno-pie` themselves where they want them. B1 reads each default from the reference with `-Q --help=common` and `-dM -E`, and checks it against the `rucc-cross` facts.

**S**, plus whatever `rb probes` (05.3) finds in the full option surface.

## 03.5 The `freebsd_kprintf` format

FreeBSD's kernel builds with `-D__printf__=__freebsd_kprintf__` when the compiler is clang, or GCC 12.1 or later (`sys/conf/kern.mk:110`). The same applies to userland code built with `FORMAT_EXTENSIONS` (`share/mk/bsd.sys.mk:279`). The archetype is a FreeBSD-local patch to GCC, carried by the `devel/freebsd-gcc14` and `-gcc15` ports in `files/patch-gcc_c-family_c-format.cc`. Upstream GCC does not have it. rucc:

```
error: 'freebsd_kprintf' is an unrecognized format function type [E0747]
```

`crates/rucc-sema/src/check/attr.rs:149-160` already lists `freebsd_kprintf` among the Darwin archetypes it knows by name, but does not accept it. With plain `printf` checking instead, `%b` (bit fields with a decoding string) and `%D` (hex dump with a separator) are warnings. Under the kernel's `-Werror` they become errors.

The work is to implement the archetype as the port's patch defines it:
- the conversions `%b`, `%D`, `%r` and `%y`, with the argument types the patch requires;
- the warnings it gives, and no others.

The `kprintf-format` facet tests it (15.3). **M**, because the specification is a patch file, not a manual, and the behaviour to copy is the patched GCC's.

## 03.6 Warning parity: unmeasured

Both systems build almost everything with `-Werror`:
- **FreeBSD:** `WARNS` levels in `bsd.sys.mk`, and `-Werror` in `kern.pre.mk`.
- **NetBSD:** `WARNS?=5` in most of base, and `-Werror` in the kernel.

**A warning rucc gives and GCC does not is a build failure.** That makes warning parity a correctness property here, as it was not for Linux.

The two systems handle GCC's warnings differently:
- **FreeBSD** demotes about 40 warning classes to non-errors under GCC (03.4), which narrows the surface.
- **NetBSD** demotes few, and keys its `-Wno-` set to `HAVE_GCC` (05.3).

Nothing is known yet about how often rucc warns where GCC 14 or 15 does not. `rb warnings` (06.9) measures it at B2. Expect **L** for the first pass, then continuous work. Each new tree brings new code, and GCC's warnings move with each release.

## 03.7 What works

These were probed and match clang, and through clang GNU as:

| Construct | Used by | Probe |
|---|---|---|
| sections named `set_*` with `__start_set_*` and `__stop_set_*` | FreeBSD `DATA_SET`, `SYSINIT` and `SET_DECLARE`, in 554 kernel files | symbols, alignment and the walk at run time |
| NetBSD `__link_set_*` | NetBSD kernel and `crt0` | same mechanism |
| `.symver foo,foo@FBSD_1.0` and `@@` in a file-scope block | FreeBSD `__sym_compat`, in 38 libc and libthr files | symbol table matches |
| `__attribute__((ifunc))` | FreeBSD `DEFINE_IFUNC` and `DEFINE_UIFUNC`, in 42 files | `STT_GNU_IFUNC`, PLT relocations |
| the `genassym` arrays | FreeBSD `assym.inc` via `nm -P` sizes | sizes exact: `PCB_Cw0` 10010, `NEGsign` 10001, `NEGw0` 10005, same as clang |
| `.weak`, `.equ`, `.pushsection` flags, `@note`, COMDAT groups in a `.s` file | every `.S` file | correct |
| `__weak_alias` in one block with `=` | NetBSD libc, in 600 files | `W` symbol |
| `-fstack-usage` | NetBSD kernel | GCC's `.su` format |

NetBSD's `genassym` takes a different route, through `-S` and a `#define` template. It needs the `asm` operand modifier `%c` (or `%a`/`%n` per architecture) to print integer constants without `$`. That already works for Linux's `asm-offsets.c`. B1 confirms it on NetBSD's `sys/kern/genassym.sh` input.

## 03.8 Kernel features reused from `rucc-kernel`

The Linux kernel work built most of what a BSD kernel asks of a compiler. These were all shipped for `rucc-kernel` K2 to K4:
- `-mcmodel=kernel`, `-mno-red-zone`, the no-FPU modes (`-mno-sse -msoft-float` and `-mgeneral-regs-only`);
- the stack protector with `-fstack-protector-strong`;
- `-mindirect-branch=thunk`;
- `asm goto`, the full GNU macro assembler, `.code16` and `.code16gcc` (`crates/rucc-asm/src/sixteen.rs`);
- the i686 back end, with the X32 row passing 2824 of 2824 tests at 7.2.8.

Two differences matter here:
- **The BSD kernels are smaller users of these features than Linux.** Neither has `objtool`, alternatives patching on the scale of Linux, or static calls.
- **They use features Linux does not.** These are CTF (03.10), `ld -r` modules on amd64, and the `freebsd_kprintf` format (03.5).

What remains is **S** to confirm per flag (08.2).

## 03.9 Boot code

Both systems build their BIOS boot blocks during the world build, as C with `-m32` and `-Os`, under hard size limits:
- FreeBSD's `stand/i386/boot2` must fit in 7.5 KB, and the build fails if it does not;
- NetBSD's `bootxx` fits in the first sectors of the partition.

Both also build EFI loaders:
- FreeBSD's `loader.efi` is PE32+ made from ELF by `objcopy`;
- NetBSD's is `bootx64.efi`.

rucc has every instruction-level piece, but its `-Os` code size has never been held to a byte bound. **M**, measured at B4 by `boot-sizes` (15.3) and fixed where needed. 08.6 has the detail.

## 03.10 DWARF for CTF

Both systems convert DWARF to CTF with `ctfconvert` for every kernel object, because DTrace reads CTF:
- **FreeBSD:** GENERIC has `WITH_CTF=1`, and `kern.mk` forces `-gdwarf-4`.
- **NetBSD:** `MKCTF` defaults to yes on x86_64 and aarch64.

`ctfconvert` reads less of DWARF than GDB does, but reads it strictly:
- base types by name and encoding;
- bitfields;
- `DW_TAG_restrict_type` and `volatile` chains;
- anonymous unions.

rucc writes DWARF 4 and 5 for debuggers. Nobody has run `ctfconvert` on it. **M**, measured by `rb ctf-diff` at B3 (09.6).

## 03.11 rucc as a FreeBSD host

The FreeBSD reference cross GCC is a FreeBSD package (`devel/freebsd-gcc15`), so the FreeBSD build runs on a FreeBSD builder (04.4). rucc must then build, test and run on FreeBSD amd64. Rust supports `x86_64-unknown-freebsd` as a tier 2 host with tools. Whether rucc's own test suite has Linux-only assumptions is unknown until it runs there. **S**, at B1, unless that run says otherwise. NetBSD builds run on Linux, so NetBSD hosting is never needed. It becomes interesting only for B10's self-hosting row.

## 03.12 The C++ boundary

Both base systems contain C++:
- **FreeBSD** has `devd`, `config(8)`, `dtc`, `ctld`, `zfsd`, libc++ and LLVM. It no longer has a switch to turn C++ off.
- **NetBSD** has GCC, libstdc++, ATF's C++ half, GDB and groff. Its `MKCXX=no` switch turns all of them off.

rucc is a C compiler, and this folder does not change that. Document 10 draws the line: the C++ compiler in both builds is the reference `g++`, and `rb census` says exactly which objects that is. rucc's part of the work is that the C objects it builds link with `g++`'s objects and libraries:
- C++ calls into C libraries;
- C++ throws through C frames compiled with `-fexceptions`, so the unwind tables must be right (08.4).

There is no compiler code to write for this, only tests.

## 03.13 Feature-test macros answer differently

GCC 14 added `__has_feature` and `__has_extension`. Both systems' headers use them to choose between a C11 construct and a GNU fallback. FreeBSD's `sys/sys/cdefs.h` does this at:
- line 197, `#if !__has_extension(c_thread_local)`, which defines `_Thread_local` as a macro;
- lines 221 and 244, `__has_extension(c_generic_selections)`, which decides how `__generic` and `__qualsel` are spelled.

Compared with GCC 16.2.0 on the same input with `-std=gnu11`:

| Query | GCC 16.2 | rucc 0.30.15 |
|---|---|---|
| `__has_extension(c_static_assert)`, `(c_alignas)`, `(c_atomic)`, `(c_generic_selections)`, `(c_thread_local)` | 1 | 0 |
| `__has_feature(c_static_assert)`, `(c_generic_selections)` | 1 (0 under `-std=gnu99`) | 0 |
| `__has_feature(address_sanitizer)`, `(nullability)` | 0 | 0 |
| `__has_builtin(__builtin_align_up)` | 0 | 0 |
| `__has_attribute(__nonstring__)`, `(no_stack_protector)` | 1 | 1 |

So the same header takes a different branch under rucc than under the reference. The fallback branches compile, which is why nothing has failed, but R and G no longer build the same code from the same source. That breaks 02.2's premise without a single diagnostic.

There is a second difference. Under a persona older than 14, as on `netbsd-11` (12.5.0) and `netbsd-10` (10.5.0), the reference GCC has no `__has_feature` at all, and `#ifdef __has_feature` is false. rucc defines it under every persona.

The fix:
- answer every `__has_feature` and `__has_extension` query as GCC 16 does for the language mode;
- define neither macro under a persona older than 14.

The `bsd-predef` corpus (15.2) gains a query file for every feature and extension name either system's headers ask about. **S**, at B1.

## 03.14 The unwinder builtins

NetBSD builds GCC's runtime libraries as ordinary base libraries, with `bsd.lib.mk` and therefore with `${CC}`: `external/gpl3/gcc/lib/libgcc/libgcc_eh/Makefile` compiles `${LIB2_EH}`, which is GCC's `unwind-dw2.c`, `unwind-dw2-fde.c` and `unwind-c.c`, and `libgcc_s` does the same. In the rucc build that compiler is rucc. GCC's unwinder is written with builtins that exist only to write an unwinder:

| Builtin | Used for | rucc |
|---|---|---|
| `__builtin_unwind_init` | `uw_init_context`: force every call-saved register to the stack so the unwinder can find it | unknown, an implicit-declaration error |
| `__builtin_dwarf_cfa` | the canonical frame address of the unwinder's own frame | unknown |
| `__builtin_eh_return` | `uw_install_context`: adjust the stack and jump to the landing pad, restoring the saved registers | unknown |
| `__builtin_dwarf_sp_column` | the DWARF column of the stack pointer | unknown |
| `__builtin_init_dwarf_reg_size_table` | the register size table, in GCC versions that still use it | unknown |
| `__builtin_frob_return_addr`, `__builtin_extract_return_addr` | return address adjustment | accepted |
| `__attribute__((mode(TI)))`, `mode(XF)`, `mode(TF)`, `mode(__word__)` | `libgcc2.c` types | accepted |

Probed on 8 November 2026 with the x86-64 Linux target, because the BSD targets are refused (03.1).

Without them NetBSD's `libgcc_eh.a` and `libgcc_s.so` do not build, so neither does anything in the world that throws or links C++. They are not hard to build: `__builtin_eh_return` needs a function epilogue that restores every call-saved register and adds an offset to the stack pointer before the return. But the unwinder is the one library where "nearly right" means every C++ exception in the system crashes. The test is `rucc-corpus`'s `unwinder` facet, which throws through GCC-built frames using a rucc-built `libgcc_eh` and the reverse, plus NetBSD's own `lib/libc/c++` and ATF C++ tests at B4. **M**, at B4.

FreeBSD is different. Its `libgcc_eh` and `libgcc_s` are LLVM's `libunwind` with the C files built with `-funwind-tables` (`lib/libgcc_eh/Makefile.inc`), and `libunwind` does not use these builtins. Its C files `UnwindLevel1.c` and `UnwindLevel1-gcc-ext.c` are rucc's in R and are covered by the same tests.

## 03.15 Sizes in total

| Milestone | rucc work from this document |
|---|---|
| B1 | targets (L), file-scope `asm` (M), identity (S), driver flags (S), feature-test macros (S), FreeBSD host (S) |
| B2 | `freebsd_kprintf` (M), warning parity first pass (L) |
| B3 | DWARF for CTF (M), kernel flag confirmation (S) |
| B4 | unwinder builtins (M) |
| B4, B6 | boot code size (M) |

About 19 to 26 engineer-weeks of rucc work is visible now. Unseen code generation bugs come on top of that, and so does the census (14.5), which has not run. Document 16 adds the harness work and a contingency.
