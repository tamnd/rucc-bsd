# Code generation, ABI and the runtime

A base system is the hardest ABI customer a C compiler has. Every library rucc builds is linked against code that the reference built: `g++` objects, the C++ runtime, the boot loader, the project's own release packages (12.8). Every program rucc builds is started by start files that rucc also built, and is run by a dynamic linker that rucc built. This document lists what "the same ABI" means per system and how each part is checked.

## 08.1 What a BSD target is

A target is everything the reference GCC decides from its target triple. rucc must make the same decisions, read from the reference rather than written from documentation:

| Decision | FreeBSD amd64 | NetBSD amd64 | Checked by |
|---|---|---|---|
| Triple the build passes | `x86_64-unknown-freebsd15.1` (and per release) | `x86_64--netbsd` | `rb toolchain-diff` (05.4) |
| System macro | `__FreeBSD__=15` (the major version) | `__NetBSD__` defined as 1; the version comes from `<sys/param.h>`, not the compiler | `bsd-predef` (15.2) |
| Common macros | `__ELF__`, `__unix__`, `__x86_64__`, `__LP64__` | `__ELF__`, `__unix__`, `__x86_64__`, `__LP64__` | `bsd-predef` |
| `char` | signed | signed | `bsd-predef`, `bsd-abi` |
| `wchar_t` | `int` | `int` | same |
| `long double` | x87 80-bit in 16 bytes | same | same |
| PIE by default | yes for 64-bit (`share/mk/bsd.opts.mk:97-105`); the build passes `-fPIE` itself | yes on `x86_64` (`share/mk/bsd.own.mk:1255`, `MKPIE`); `bsd.prog.mk:38` passes `PIE_CFLAGS` | `rb flags-diff` |
| Dynamic linker | `/libexec/ld-elf.so.1` | `/usr/libexec/ld.elf_so` | the link half of `rb flags-diff` |
| Start files | `crt1.o`, `crti.o`, `crtbegin.o` or `crtbeginS.o` or `crtbeginT.o`, `crtend.o` or `crtendS.o`, `crtn.o` | `crt0.o`, `crti.o`, `crtbegin.o` or `crtbeginS.o`, `crtend.o`, `crtn.o` | same |
| Runtime library | `libgcc.a`, which is a symlink to `libcompiler_rt.a` (`lib/libcompiler_rt/Makefile:41`), and `libgcc_s.so.1` from LLVM `libunwind` | `libgcc.a`, `libgcc_eh.a` and `libgcc_s.so` from GCC's own sources, built in the world (03.14) | same |
| ELF OSABI and branding | `ELFOSABI_FREEBSD` (9) plus the `.note.tag` ABI note from `crtbrand.S` | `ELFOSABI_NONE` plus `.note.netbsd.ident` from `sysident.S` | 09.2 |

The architecture-specific rows for aarch64 and riscv64 differ:
- `char` is unsigned on both;
- `long double` is IEEE quad;
- FreeBSD's arm64 `wchar_t` is `unsigned int` (`sys/arm64/include/_types.h:65`).

They are document 11's. The table above is a starting point and the generated `bsd-predef` and `bsd-abi` lists replace it.

**The link line.** rucc drives the link itself. The ELF linker is the reference's BFD `ld`, found with `-B` (FreeBSD, 05.4) or in `TOOLDIR` (NetBSD). The rule is that rucc's link command for any set of flags is the reference driver's `-###` link command, with the same:
- start files in the same order;
- `-dynamic-linker` value;
- `--hash-style`, read from each reference, not assumed;
- `--eh-frame-hdr`;
- `-pie` or `-no-pie` default;
- default libraries, including the `-lgcc --as-needed -lgcc_s --no-as-needed` dance and `-lc` placement;
- library search path.

`rb flags-diff` compares compile lines. Its link half runs both drivers with `-###` on every link the build performs and compares the normalised commands. Any difference is a bug in rucc's target description and fails B1.

## 08.2 Kernel flags

Each kernel's flag line is accepted today (03.4, 03.8). What must also be true is that each flag *means* what GCC means:

| Flag | FreeBSD amd64 | NetBSD amd64 | Meaning to check |
|---|---|---|---|
| `-mcmodel=kernel` | `sys/conf/kern.mk:199` | `sys/arch/amd64/conf/Makefile.amd64:39` | sign-extended 32-bit addresses in the top 2 GB; no `movabs` for symbols |
| `-mno-red-zone` | `kern.mk:199` | yes | no access below `%rsp` in leaf functions; interrupts land on the kernel stack |
| `-mno-mmx -mno-sse -msoft-float` | `kern.mk:199` | yes, plus `-mno-avx` | no vector or x87 instruction anywhere, including in rucc's own `memcpy` and `memset` expansions and in struct copies (08.3) |
| `-ffreestanding` | yes | yes | no library call that was not written; `__builtin_memcpy` may still call `memcpy`, which the kernel provides |
| `-fno-common` | yes | yes | the default since GCC 10, checked anyway |
| `-fno-omit-frame-pointer` | with `-mno-omit-leaf-frame-pointer` (`sys/conf/Makefile.amd64:35`, `kmod.mk:166`) | `DEFCOPTS=-O2 -fno-omit-frame-pointer` (`Makefile.amd64:37`) | `%rbp` chains in every function, leaves included, for `DDB` and DTrace stack walks; checked by `rb frames`. Accepted (probed) |
| `-fno-strict-aliasing` | yes | yes | the optimizer must not use type-based alias analysis at all |
| `-fno-delete-null-pointer-checks` | no | yes | a load from address 0 is not undefined; the check after it stays |
| `-fno-zero-initialized-in-bss` | no | yes (`sys/conf/Makefile.kern.inc:107`) | zero-initialised objects with an explicit initialiser go to `.data`, not `.bss` |
| `-fstack-protector-strong` | yes | yes | canary placement and the `__stack_chk_guard` symbol name per system |
| `-mindirect-branch=thunk`, `-mindirect-branch-register` | no | yes | thunk names `__x86_indirect_thunk_rax` and family, which the NetBSD kernel provides |
| `-ftrivial-auto-var-init=pattern` | no | through `COPT_AUTOINIT` when `HAVE_GCC >= 12` (`Makefile.kern.inc:128`), so the persona decides whether the flag is passed at all | the same pattern bytes as GCC per type, so a use of uninitialised memory fails the same way in R and G |
| `-fstack-usage`, `-Wstack-usage=3584` | no | yes | GCC's `.su` format; a frame over the limit is an error under `-Werror`, so rucc's frame sizes must not be larger than GCC's where GCC's are near 3,584 bytes (08.7) |
| `-mcmodel=medany` on riscv64 | `kern.mk:175`, `CFLAGS.gcc` | module flags (`share/mk/bsd.kmodule.mk:68`) | 11 |

**Kernel modules.**
- **NetBSD** builds `.kmod` files with `ld -r` on every architecture (`share/mk/bsd.kmodule.mk:166-195`).
- **FreeBSD** amd64 builds `.ko` files with `ld -r` (`sys/conf/kmod.mk:82-86`, `__KLD_SHARED=no`).
- **FreeBSD on every other architecture** links them `-Bshareable` (`kmod.mk:246`).

In each case the kernel's own linker reads the module, so relocation types matter. A module rucc builds may contain only the relocation types the kernel linker handles. For FreeBSD amd64 `ld -r` objects that is the `switch` in `sys/amd64/amd64/elf_machdep.c:292-373`: `R_X86_64_64`, `PC32`, `PLT32`, `32S`, `GLOB_DAT`, `RELATIVE` and a few others. rucc preferring `GOTPCRELX` or `REX_GOTPCRELX` where GCC with `-mcmodel=kernel` emits none would make an unloadable module. `rb cross-modules` loads rucc modules into the reference kernel and the reverse (09.3).

## 08.3 No floating point in the kernel: `rb vec-audit`

On amd64 both kernels are built with no SSE and no x87, and neither saves the user's floating-point registers on kernel entry unless the code asks for it with `fpu_kern_enter`. One `movups` in a struct copy corrupts a user process's registers, silently, at some later context switch. `rucc-kernel` met this for Linux and built the check: decode every object compiled with the no-FPU flags and list every vector or x87 instruction.

`rb vec-audit` runs it per build on:
- every kernel object;
- every module;
- the boot code, which runs before the FPU is set up.

The allow-list holds only the files that use the FPU by design inside `fpu_kern_enter` (FreeBSD) or `fpu_kern_enter`/`kthread_fpu_enter` (NetBSD), named by the reference build: an instruction that appears in rucc's object of a file is allowed only if the reference's object of the same file also has it. **Gate: zero findings, at B3 and B5.**

## 08.4 Unwinding and C++ interop

The base systems mix C and C++ objects in one process (03.12, 10). For that to work:

1. **Every C object rucc builds has correct `.eh_frame`.** The `.eh_frame` must be correct whenever the build asks for unwind tables. FreeBSD builds libc's cancellation points and all of `libthr` with `-fexceptions` (`lib/libc/Makefile:53`, `lib/libthr/Makefile:51`), so `pthread_cancel` unwinds through them with the C++ unwinder.
2. **The unwinder itself.** On NetBSD the unwinder is GCC's, compiled by rucc (03.14). On FreeBSD it is LLVM's `libunwind`, whose C files rucc compiles.
3. **Personality routines and LSDA.** C code compiled with `-fexceptions` and `__attribute__((cleanup))` must emit `__gcc_personality_v0` and its tables as GCC does.

The tests:
- **`rucc-corpus` `unwinder` facet.** It throws through every combination of rucc and reference frames: a C++ throw through a rucc C callback (`qsort`, `bsearch`), `pthread_cancel` through rucc `libthr`, and a `cleanup` attribute running during a C++ unwind.
- **The systems' own tests.** These are ATF's C++ cases, FreeBSD's `lib/libthr` and `lib/libc/c++` tests, and NetBSD's `lib/libpthread` and `lib/libstdc++` tests, all with the C side built by rucc.
- **`rb frames --cfi`.** It checks that the `.eh_frame` of every function describes the same registers saved at the same offsets as `.debug_frame` and the code. This is `rucc-kernel`'s ORC cross-check, retargeted to DWARF CFI.

## 08.5 libc shapes

The C library is where a base system uses the most linker and loader features at once. Each feature below is used, was probed or is to be probed, and is gated by a test.

**Symbol versions.**
- FreeBSD versions every shared library through `Symbol.map` and `Versions.def`, plus `__sym_compat` and `__sym_default` in C (07.2).
- NetBSD does not version libc symbols. It renames them instead (`__RENAME` and `__weak_alias` to `__fooNN` names), which only needs aliases to work.

Gate: `rb abi-diff` (09.4) shows identical version definitions and identical symbol-to-version assignments for every shared library.

**`ifunc`.**
- FreeBSD's kernel and libc use `DEFINE_IFUNC` and `DEFINE_UIFUNC` (`sys/x86/include/ifunc.h:34-48`) in the 42 files of 03.7: string functions, `pmap`, and the kernel's CPU-specific paths.
- In static binaries, `__libc_start1` (`lib/libc/csu/libc_start1.c`) processes the `IRELATIVE` relocations between `__rela_iplt_start` and `__rela_iplt_end` before `main`.
- NetBSD's `crt0-common.c:148-166` does the same.

The object side works (`STT_GNU_IFUNC`, probed 8 November 2026 with the Linux target). The static start-up path is gated by `ifunc-static` (15.3) and by running FreeBSD's static `/rescue` binaries, which are a crunched static program built entirely by rucc.

**Thread-local storage.**
- FreeBSD builds libc with `-ftls-model=initial-exec` (`lib/libc/Makefile:59`), and jemalloc uses `tls_model("initial-exec")` directly.
- `rtld` has its own TLS and must not use TLS relocations of the general-dynamic kind.

rucc emits `GOTTPOFF` for the flag and for the attribute (probed). The other three models, and the variant I layout on aarch64 and riscv64, are gated by `tls-models` (15.3).

**Start files compiled by rucc.**
- FreeBSD's `crtbegin.c` and `crtend.c` (`lib/csu/common/`) are C, so in R they are rucc's, and so is everything that walks `.ctors`, `.dtors` and `__JCR_LIST__`.
- NetBSD's `crtbegin.c` and `crt0-common.c` are C too.
- These run before `main` in every program of the system.

Gate: `crt-and-brand` (15.3), which checks constructor order with priorities, `.init_array` against `.ctors`, and `_init` and `_fini`. The brand side is checked by every boot, because a misbranded `init` does not start.

**Aliases.**
- FreeBSD's `__weak_reference` must give `W` (bug A, 03.2).
- NetBSD's `__weak_alias` and `__strong_alias` already do.

Gate: `rb symbols-diff` (09.5) shows the same binding, type, visibility and size for every symbol of every library.

## 08.6 Boot code

The boot blocks are built in the world build by the world compiler, so in R they are rucc's.

| Block | Built with | Bound |
|---|---|---|
| FreeBSD `stand/i386/boot2` | `-m32 -Os -fomit-frame-pointer -mrtd -mregparm=3 -fno-asynchronous-unwind-tables --param max-inline-insns-single=100 --param min-pagesize=1024` (`stand/i386/boot2/Makefile:20-37`) | `BOOT2SIZE=7680` bytes (`:59`); the build computes the space left and fails if it is negative |
| FreeBSD `boot0`, `cdboot`, `pxeboot`, `gptboot`, `gptzfsboot`, `btx` | assembly plus C at `-Os` | `boot0` is one sector; the others have limits set by their loaders, read from each Makefile by `boot-sizes` |
| FreeBSD `loader.efi` | C with `-fshort-wchar -mno-red-zone` and `-fPIC` (`stand/efi/Makefile.inc:9-20`), linked to ELF and turned into PE32+ by `objcopy` | none, but must load on real UEFI firmware and in QEMU's OVMF |
| NetBSD `sys/arch/i386/stand/mbr`, `bootxx`, `boot` | `-m32` with `OPT_SIZE.gcc` (`bootxx/Makefile.bootxx:57-65`) | `bootxx` fits its sectors |
| NetBSD `efiboot` | C, `bootx64.efi` | firmware |

Three rucc gaps show here:
- **The two refused flags.** `-mrtd` and `--param min-pagesize` (03.4).
- **`-mregparm=3` together with `-mrtd`.** Every call in `boot2` is callee-pops with the first three arguments in registers. rucc accepts `-mregparm=3` and implements `stdcall`, but the combination of both as defaults has never been tested. It is also an ABI between rucc's `boot2` C and `boot2`'s assembly, which assumes it.
- **Size.** rucc's `-Os` has never been held to a byte bound. FreeBSD's GCC build of `boot2` has 7,680 bytes and uses most of them. If rucc's code is 2% larger, the build fails.

**Gate (B4 for NetBSD, B6 for FreeBSD).**
1. Every boot block builds within its bound.
2. `boot-sizes` (15.3) records each block's size against the reference's, and rucc may be at most the bound. It need not match the reference's size.
3. The images boot, BIOS and UEFI, in QEMU (12.3).

The `-Os` work is the part that could be large. If `boot2` does not fit, rucc's size heuristics under `-Os` are tuned on the `boot2` units until it does. Tuning must not change rucc's output for any other `-O` level, and `rucc-real-corpus`'s size row must not regress.

## 08.7 Frames and stack usage

NetBSD's kernel is built with `-fstack-usage -Wstack-usage=3584`. `rb frames` compares, per function:
- the `.su` frame size from both compilers;
- `static`, `dynamic` and `bounded` qualifiers;
- the CFI's saved-register layout.

Rules:
- **A rucc frame larger than the reference's by more than 25% and 128 bytes is a finding.** Kernel stacks are small: `KSTACK_PAGES=6`, 24 KB, on FreeBSD amd64 (`sys/amd64/include/param.h:135`); three pages, 12 KB, of stack on NetBSD amd64 (`sys/arch/amd64/include/param.h:80`).
- **A frame over the `-Wstack-usage` limit fails the build in R** while G passes, which is a build failure attributed to rucc.
- **A `dynamic` frame where the reference's is `static`** means rucc made a variable-length allocation the reference did not, and is a finding.

FreeBSD has no stack usage limit in its build, but its kernel stack is fixed, and deep paths such as ZFS and the network stack can come close to it. `rb frames` reports the 100 largest frame differences for FreeBSD as well, informational until a test fails with a double fault.
