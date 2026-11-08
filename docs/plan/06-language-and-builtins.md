# Language and builtins

The C in a BSD base system is mostly conservative. It is written by projects that have built with more than one compiler for decades, through a `cdefs.h` that wraps every extension in a macro. The language risk here is smaller than Linux's. What is larger is the volume and the contributed code, and above all `-Werror` on nearly every unit. A BSD build is unforgiving of a compiler that has opinions the reference does not share.

## 06.1 Standards and dialect

| | FreeBSD | NetBSD |
|---|---|---|
| Default `-std` | `gnu17` (`CSTD?=gnu17`), and anything older than C99 is an error | `gnu11` for userland and kernel |
| C23 | headers gain C23 parts in 15.1 (`<stdbit.h>`, `memalignment`), "complete support anticipated for FreeBSD 16" | 11.0 claims "enhanced compliance with C23", with a `c17(1)` wrapper |
| Contributed code | some directories set their own `CSTD` | the same, through `external/` makefiles |

Because every unit gets an explicit `-std`, rucc's GCC 16 default (`gnu23`) never applies to a base build. Two things still depend on the persona:
- **GCC 14 made four diagnostics errors by default:** implicit function declarations, implicit `int`, `int` to pointer conversion and incompatible pointer types. Under `netbsd-11` and `netbsd-10` the reference is GCC 12.5 or 10.5, where they are warnings, and code that relies on that compiles with `-Wno-error` where needed. rucc under a persona older than 14 must give warnings there, not errors, and under 14 and later must give errors.
- **Headers test `__STDC_VERSION__` and `__GNUC_PREREQ__`**, and both must come out as the reference's do.

## 06.2 What the system headers ask

`<sys/cdefs.h>` is the contract between each system and its compilers. FreeBSD's has 675 lines; NetBSD's has 774, plus 254 in `cdefs_elf.h`. Every macro a base system uses for an extension is defined there, in branches on:
- `__GNUC_PREREQ__(major, minor)`, computed from `__GNUC__` and `__GNUC_MINOR__`;
- `__has_attribute`, `__has_builtin`, `__has_c_attribute`, `__has_feature`, `__has_extension` and `__has_include`.

In the trees of 7 November 2026, the headers in `sys/sys` of both systems ask about these:

| Query | Asked about |
|---|---|
| `__has_extension` | `c_generic_selections`, `c_thread_local`, `c_static_assert`, `c_alignas`, `c_atomic`, `c_thread_safety_attributes`, and their `cxx_` versions for C++ |
| `__has_feature` | `address_sanitizer`, `thread_sanitizer`, `memory_sanitizer`, `undefined_behavior_sanitizer`, `leak_sanitizer`, `coverage_sanitizer`, `safe_stack`, `nullability` |
| `__has_builtin` | `__builtin_align_up`, `__builtin_align_down`, `__builtin_is_aligned` |
| `__has_attribute` | `no_stack_protector`, `__nonstring__`, `__type_tag_for_datatype__`, `__argument_with_type_tag__` |
| `__has_c_attribute` | `__nodiscard__` |

**Rule: every answer must be the reference's answer** for the persona and language mode, not what rucc would like to say. A `1` that GCC gives as `0` sends the header into a branch GCC never compiled. A `0` that GCC gives as `1` does the same in the other direction. 03.13 shows rucc answers the extension queries wrongly today.

`bsd-predef` (15.2) holds the full query list per system and per persona, generated from the trees by `rb demands` and answered by the reference on the builder.

## 06.3 Attributes

`rb demands` will produce the full table at B0. From the cdefs macros, the set is:

| Family | Attributes | Notes |
|---|---|---|
| placement | `section`, `used`, `aligned`, `retain` | linker sets (09.1), `.init_array`, `SYSINIT` |
| linkage | `weak`, `alias`, `visibility`, `ifunc` | libc namespaces, `DEFINE_IFUNC`, `__sym_compat` via `.symver` (07.3) |
| startup | `constructor` and `destructor` with priorities | libc, libthr and rtld start-up order |
| checking | `format` (printf, scanf, strftime, strfmon, `freebsd_kprintf`), `format_arg`, `nonnull`, `nonstring`, `warn_unused_result`, `sentinel`, `deprecated`, `access` | `-Werror` makes every one of these a correctness question (06.9) |
| optimization | `noreturn` (`__dead2`, `__dead`), `pure`, `const` (`__pure2`), `malloc`, `alloc_size`, `alloc_align`, `returns_twice`, `noinline`, `always_inline`, `cold`, `hot` | |
| layout | `packed`, `aligned`, `may_alias`, `transparent_union`, `mode` | network headers, device registers, contributed code |
| kernel | `no_instrument_function`, `no_sanitize`, `no_stack_protector`, `naked` (few), `__nosanitizeaddress` and friends | `KASAN`, `KCSAN` and `KMSAN` builds |

All of these exist in rucc for Linux. The work is in the ones whose behaviour is checked against GCC's here and was not against Linux:
- `transparent_union` where contributed code uses it (neither system's own headers do);
- `mode` in contributed code;
- `access`, which GCC 11 and later uses for warnings that matter under `-Werror`.

## 06.4 Builtins

| Builtins | Used by | Note |
|---|---|---|
| `__builtin_object_size`, `__builtin_dynamic_object_size` | FreeBSD's `_FORTIFY_SOURCE` (`include/ssp/`, 51 files in headers and libc); NetBSD's `ssp` | must fold to the same constant GCC folds to at `-O2`, or fortified calls go to the checking path, which is slower but not wrong |
| `__builtin_constant_p` | kernel macros (`bitcount`, `__BIT`, `bswap` choices) | the answer decides inline versus call |
| `__builtin_va_*` | everywhere | `va_list` layout per ABI (08.1) |
| `__builtin_offsetof`, `__builtin_types_compatible_p`, `__builtin_choose_expr` | `__offsetof`, the `__generic` fallback | |
| `__builtin_frame_address`, `__builtin_return_address` | the kernels' stack traces and `ddb` | with `-fno-omit-frame-pointer` on FreeBSD's kernel |
| `__builtin_trap`, `__builtin_unreachable`, `__builtin_expect` | everywhere | |
| bit and byte builtins (`clz`, `ctz`, `popcount`, `ffs`, `bswap`) | `<sys/bitstring.h>`, `<sys/endian.h>`, `bitcount` | |
| overflow builtins | `__builtin_add_overflow` in libc's `reallocarray` and the kernels' `mallocarray` | |
| `__atomic_*`, `__sync_*` | userland `<stdatomic.h>` under GCC, libthr, contributed code | kernels use their own inline assembly for `atomic(9)` on amd64 and aarch64 |
| `__builtin_ia32_*`, `__builtin_aarch64_*` | almost none outside contributed crypto code | |
| `__builtin_align_up`, `__builtin_is_aligned` | queried in `cdefs.h`; GCC lacks them, so the fallback is used | rucc must also say no |

FreeBSD's `<stdatomic.h>` (`sys/sys/stdatomic.h:40`) picks clang's `__c11_atomic_*` only when `__has_extension(c_atomic)` holds and `__clang__` is defined, `__atomic_*` for GCC 4.7 and later, and `__sync_*` before that. rucc never defines `__clang__`, so it takes GCC's branch. The rule of 06.2 is what keeps it there: the branch is chosen by the compiler's answers, and the answers must be the reference's.

## 06.5 Inline assembly

The BSD kernels use inline assembly more conservatively than Linux:
- FreeBSD has one file with `asm goto`, and NetBSD none;
- neither has static calls, `ALTERNATIVE`-style self-patching on Linux's scale, or exception table macros in inline assembly. FreeBSD's `pcb_onfault` and NetBSD's `onfault` handlers are assembly functions.

The amd64 inline assembly is in a few headers:
- FreeBSD's `cpufunc.h`, `atomic.h`, `pcpu.h` and `counter.h`;
- NetBSD's `cpufunc.h`, `pio.h` and the `x86` headers;
- the per-CPU access through `%gs`.

All of those constructs were exercised by `rucc-kernel`.

The new things to check:
- FreeBSD's `pcpu.h` (`sys/amd64/include/pcpu.h:200-243`) reads and writes per-CPU fields as `%%gs:%c1`, an `__offsetof` constant printed through the `c` operand modifier;
- NetBSD's `genassym` reads operand values from `-S` output, with `%a`-style modifiers per architecture (03.7);
- aarch64 kernels use `mrs` and `msr` on named system registers, which `rucc-kernel` K6 covered.

`bsd-asm` (15.2) checks every `.S` file. Inline assembly is checked by `rb sections-diff` and by the tests.

## 06.6 Format checking

| Archetype | Where | State in rucc |
|---|---|---|
| `printf`, `scanf`, `strftime`, `strfmon` | everywhere, through `__printflike(m, n)`, `__scanflike`, `__strftimelike`, `__strfmonlike` | present |
| `format_arg` | `gettext`-style functions, `__format_arg` | present |
| `freebsd_kprintf` | FreeBSD's kernel and `FORMAT_EXTENSIONS` userland, via `-D__printf__=__freebsd_kprintf__` | refused (03.5) |

**`freebsd_kprintf`.** The FreeBSD GCC ports' patch adds it to GCC's `c-format.cc` as a copy of `printf` with four conversions:

| Conversion | Meaning in `kvprintf` | Arguments |
|---|---|---|
| `%b` | an integer printed with a bit-name decoding string | `int` and `const char *` |
| `%D` | a hex dump of a buffer, with a separator string | `const u_char *` and `const char *` |
| `%r` | an integer in the current radix, as `ddb` prints | `int`, with the usual length modifiers |
| `%y` | an integer in hex with a `0x` prefix, as `ddb` prints | `int`, with the usual length modifiers |

The exact argument types, flags and length modifiers each conversion accepts are in the patch, and the patched GCC 15.2 on the builder is the oracle. `kprintf-format` (15.3) holds a test per conversion, per modifier and per wrong argument. Its expected diagnostics come from the patched GCC, never from reading the patch.

Two more points:
- **The `-D__printf__=__freebsd_kprintf__` trick** means every `__attribute__((__format__(__printf__, ...)))` in the kernel, including those in headers shared with userland, becomes the FreeBSD archetype. rucc must accept the archetype in every position where `printf` is accepted.
- **NetBSD has no kernel archetype.** Its kernel prints bit fields through `snprintb(3)`, not a conversion. NetBSD is a plain `printf` user, under `-Wformat=2` at `WARNS=6` (`share/mk/bsd.sys.mk:134`).

## 06.7 Optimization as correctness

The BSDs depend less than Linux on the optimizer for correctness. There is no `BUILD_BUG_ON` built from dead code elimination and no link-time assertion idiom. What is left:
- **`CTASSERT` and `_Static_assert`** with integer constant expressions, including `sizeof` and `__offsetof` of structures, in both kernels. They are front end constant folding only.
- **`_FORTIFY_SOURCE`.** A fold that differs from GCC's is not a correctness bug, but it is a performance difference and a different code path. `rb symbols-diff` lists `__*_chk` calls per object in R and G.
- **Boot code size** (08.6). It is the one place where code size is a hard build error.
- **`genassym`** (03.7). The sizes of objects must be exactly the constants, which they are.

## 06.8 Undefined behaviour and the oracle

The flags that limit what the optimizer may assume are set by each system and must be honoured exactly:
- **FreeBSD's kernel:** `-fwrapv` and `-fno-common`.
- **NetBSD's kernel:** `-fno-strict-aliasing`, `-fno-delete-null-pointer-checks` and `-fno-zero-initialized-in-bss`.
- **Userland:** whatever each directory sets.

Base systems contain undefined behaviour that GCC happens not to exploit, as every large C program does. When `rb mixed` (12.7) narrows a failure to one function and the reduction shows undefined behaviour:
1. The finding is reported to the system's project with the reduction, through a person, as for 04.6.
2. It is recorded in `exclusions.toml` with the test it fails and the upstream report.
3. It is not a rucc bug, unless GCC documents the behaviour rucc broke, for example through `-fwrapv`. In that case it is a rucc bug, whatever the standard says.

The `rucc-kernel` rule applies: rucc may exploit no more undefined behaviour than the reference does at the same flags, where that is measurable. An exclusion therefore needs the reduction to show the reference also changes behaviour at some optimization level or version. Otherwise rucc is being stricter than its oracle for no gain.

## 06.9 Warning parity

**The problem.** Both systems build almost everything with `-Werror`:
- **FreeBSD:** `WARNS=1` and higher add `-Werror` (`share/mk/bsd.sys.mk`). `DEFAULTWARNS?=6` for the source tree (`share/mk/src.sys.mk:49`). The kernel uses `-Werror` through `kern.pre.mk`.
- **NetBSD:** `WARNS?=5` in the `Makefile.inc` of `bin`, `sbin`, `usr.bin` and `lib`, `-Werror` unless `NOGCCERROR` is defined (`share/mk/bsd.sys.mk:90`), and `-Werror` in the kernel.

Any warning rucc gives that the reference does not stops the build. Unlike every earlier target, warnings are correctness.

**What softens it.**
- **FreeBSD under GCC demotes 45 warning classes to non-errors** (`bsd.sys.mk:178-275`), from `-Wno-error=address` to `-Wno-error=unused-value`, plus a set of `-Wno-` flags. A rucc warning in those classes prints but does not fail.
- **NetBSD's `CC_WNO_*`** turns off a smaller set per `HAVE_GCC` value.

**The rule.** For every unit in a graded build, the set of warnings rucc gives must be a subset of the set the reference gives on the same unit with the same flags. Warnings are compared by option name (`[-Wfoo]`) and by line. A warning the reference gives that rucc does not is reported, but it is not a failure. rucc is allowed to be quieter. It is not allowed to be noisier, because noisier breaks the build.

**`rb warnings`.** Every unit's standard error is captured by the shim in both builds, and the two sets are compared per unit. It also runs inside `rb syntax`, because a front-end-only run gives most front-end warnings. Warnings that need optimization, such as `-Wmaybe-uninitialized`, `-Wstringop-overflow` and `-Warray-bounds`, need a full build. Those are exactly the warnings whose results depend on optimizer internals and therefore differ between any two compilers. That is why FreeBSD demotes them under GCC.

**What rucc needs.**
- **A persona-aware warning table.** Every warning carries:
  - the GCC version that introduced it;
  - the version in which it joined `-Wall` or `-Wextra`;
  - the version in which it became on by default or an error by default.

  Under a persona, rucc enables what that GCC version enables. Without this, every NetBSD branch with GCC 12.5 or 10.5 is graded against a warning set three to five years newer than its reference.
- **Flow-sensitive warnings no more eager than GCC's.** rucc's `-Wmaybe-uninitialized`, `-Wnull-dereference`, `-Warray-bounds` and the `-Wstringop-*` family must have fewer false positives than GCC's on the same code, or be off. When rucc cannot match GCC's precision for one of them, it reports nothing for it rather than something extra. That is allowed by the rule, and it is the honest choice when the alternative is a build failure the reference never had.
- **`-Wsystem-headers`.** FreeBSD adds it at `WARNS=1` and higher, so warnings in system headers count too.

Measured at B2, with a first-pass cost of **L** (03.6), then continuous: every new pin and every GCC import into NetBSD trunk is a new warning set.
