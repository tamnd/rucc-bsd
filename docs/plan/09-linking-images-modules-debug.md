# Linking, images, modules and debug information

Most failures that a C compiler causes in a base system show up as a wrong *object*, not as wrong instructions: a symbol with the wrong binding, a section with the wrong flags, a note the kernel does not recognise, a relocation the module loader refuses. They are cheap to find before booting anything, by comparing objects. This document describes the comparisons. Each has a command in `rb` (14.3) and each runs on every graded build, before the image is made.

## 09.1 Linker sets

Both kernels build their configuration out of linker sets: arrays of pointers that many files contribute to and one loop walks.

**FreeBSD.** `__MAKE_SET_QV` (`sys/sys/linker_set.h:66`):
- puts a `static const void *` in section `set_NAME` with `__used`;
- declares `__start_set_NAME` and `__stop_set_NAME` weak through `__WEAK`, which is a file-scope `asm(".weak ...")` (`sys/sys/cdefs.h:453`).

`SYSINIT`, `DATA_SET`, `SYSCTL`, `MODULE_DEPEND`, `DRIVER_MODULE` and `SET_DECLARE` are all built on it, in 554 kernel files (03.7). The kernel linker walks the sets of each loaded module too.

**NetBSD.** `__link_set_make_entry` (`sys/sys/cdefs_elf.h:182`) does the same in section `link_set_NAME`. It is used by the kernel (`MODULE`, `SYSCTL_SETUP`, `__link_set_add_text`) and by userland, where `crunchgen` and `rump` rely on it.

**What must hold.**
- The entry is a pointer-sized object, aligned to a pointer, with no padding between entries from different objects. rucc must not raise the alignment of a `static const void *` in a named section, which it might otherwise do for "large" data or under `-falign-data`.
- The entry is kept at every optimisation level even though nothing in the file refers to it (`__used`), and is not merged with an identical entry in the same file.
- `.weak` on a symbol the linker defines (`__start_set_NAME`) gives an undefined weak reference, so the reader rule of 07.3 must allow `.weak` on a name never defined in the file. It does today; 03.7's probe covers it.
- The section's flags are the default for a writable or read-only data section, as GCC chooses them for the qualifiers given. In FreeBSD the qualifier is `const` only when `__MAKE_SET_CONST` is `const` (`linker_set.h:47-49`). Different flags for one section name across objects give a linker warning, which is an error under `--fatal-warnings`.

**Checked by** the `linker-sets` facet (15.3) at B1, by `rb sections-diff` on every build, and at run time by every boot: a missing `SYSINIT` entry is a kernel that does not reach `init`.

## 09.2 ABI notes and branding

The kernel decides whether a binary is a native one from its notes.

| | FreeBSD | NetBSD |
|---|---|---|
| Note | `NT_FREEBSD_ABI_TAG` in `.note.tag`, from `lib/csu/common/crtbrand.S:43`, carrying `__FreeBSD_version`; `NT_FREEBSD_FEATURE_CTL` from `feature_note.S:36`; `ignore_init_note.S` | `.note.netbsd.ident` from `lib/csu/common/sysident.S`, carrying `__NetBSD_Version__`; `.note.netbsd.pax`, and the `compident.S` notes such as `.note.netbsd.march` where used |
| ELF header | `EI_OSABI` = `ELFOSABI_FREEBSD` (9), set by `ld` from the emulation | `ELFOSABI_NONE` |
| Kernel side | `sys/kern/imgact_elf.c` matches brand notes (`freebsd_brandnote`, `:239`) and OSABI, with a sysctl brand of last resort (`:119`) | `sys/kern/exec_elf.c:900`, `netbsd_elf_signature`, reads the notes when the kernel probes the binary (`:1125`) |

The notes come from `.S` files, assembled by rucc in R, so they are correct today (03.7). They break in two ways:
- **A file-scope `asm` with a note.** A note in a file-scope block goes through the directive reader and loses `@note` (03.2, bug C). Then `SHT_PROGBITS` is not merged into `PT_NOTE`, and the kernel does not see the brand.
- **A wrong version value.** `__FreeBSD_version` or `__NetBSD_Version__` comes from `<sys/param.h>` through the preprocessor. A wrong include path in R, such as the host's headers instead of `WORLDTMP`'s, gives the wrong version silently.

**Checked by** `crt-and-brand` (15.3), by `rb sections-diff` comparing the note contents of `crt1.o`, `crti.o` and every executable, and by `readelf -n` on `/sbin/init` in every image before boot.

## 09.3 Kernel modules and `rb cross-modules`

How modules are linked (08.2):
- FreeBSD amd64 builds `.ko` with `ld -r` (`sys/conf/kmod.mk:82-86`), and every other FreeBSD architecture uses `-Bshareable` (`kmod.mk:246`).
- NetBSD builds `.kmod` with `ld -r` everywhere (`share/mk/bsd.kmodule.mk:166-195`).

In both, the kernel's own linker resolves the module's relocations and its linker sets, so a module is an ABI artifact between two builds.

`rb cross-modules` runs, for each pair of builds of the same pin:
1. **Load** every module of build X into the kernel of build Y, in a VM, with `kldload` or `modload`, for X and Y in {R, G}.
2. **Exercise** each loaded module through its smoke test where one exists (12.2), otherwise through `kldstat -v` or `modstat` and an unload.
3. **Compare** the four results. G-in-G is the baseline. A failure in R-in-G or G-in-R that G-in-G does not have is a rucc ABI bug in the module or the kernel, and the matrix says which side.

Struct layout differences between the two compilers show up here, and only here, before any test runs. `bsd-abi` (15.2) finds them in installed headers. `rb cross-modules` finds them in kernel-private headers that are never installed.

Both systems also check module ABI themselves. FreeBSD uses `MODULE_VERSION` and `MODULE_DEPEND` in linker sets, and NetBSD uses the `modinfo` structure. Those checks catch version mismatches, not layout ones.

## 09.4 Library ABI: `rb abi-diff`

For every shared library in R and G, `rb abi-diff` compares:
- **the dynamic symbol table**: names, binding, type, visibility, size and version;
- **version definitions and dependencies** (`.gnu.version_d`, `.gnu.version_r`) and which version each symbol has;
- **`DT_NEEDED`, `DT_SONAME`, `DT_FLAGS` and `DT_FLAGS_1`**;
- **`PT_GNU_STACK`, `PT_GNU_RELRO`** and the program header list.

The rules:
- The symbol and version sets must be **identical**. One extra symbol is a finding. A missing one is a bug, because a program built against the project's own release (12.8) can need it.
- A size difference on a data symbol is a layout bug.
- A size difference on a function symbol is expected and ignored.

FreeBSD's `Symbol.map` files make version mismatches loud at link time. NetBSD has no versions, so its exported set is whatever is global. One wrong `static` decision, or a `.hidden` from the reader, changes the export list silently, which is why this comparison exists.

## 09.5 Objects: `rb sections-diff` and `rb symbols-diff`

These run on every object of a build, matched by path, R against G:

| Compared | Must be | Allowed to differ |
|---|---|---|
| section names | same set | `.text.*`, `.rodata.*` suffixes and `.su`-related or comment sections; a section only GCC makes for its own use, such as `.note.GNU-stack` with different content |
| section flags, type, entry size, group | same for every named section that both have | none |
| section sizes | `.text` and `.rodata` free; data sections that hold declared objects must be the same size | code and literal pools |
| symbols | same names, binding, type and visibility for every non-local symbol; same size for data | function sizes, local labels, compiler-generated names (`.LC0`, `*.cold`, `*.isra.0`, `*.constprop.0`) |
| linker set contents | same symbols in the same `set_*` and `link_set_*` sections | order within an object |
| `.ident` and `.comment` | the reference's string replaced by rucc's | |

**Output** is one line per difference, grouped by kind, with the source file. A difference in an allowed column is listed but not counted.

**Gate:** zero counted differences in the C objects of a graded build. The first full run is at B2 on a build that only needs to compile, not boot, so this catches bugs A, B and C (03.2) and anything like them two milestones before a test could.

## 09.6 CTF: `rb ctf-diff`

Both systems turn DWARF into CTF so that DTrace can see kernel types (03.10):
- **FreeBSD** forces `-gdwarf-4` on kernel builds with `-g` (`sys/conf/kern.mk:350-351`). It runs `ctfconvert` on every kernel object (`sys/conf/kern.pre.mk:260`) and `ctfmerge` on the kernel (`kern.post.mk:209`) and on every module (`kmod.mk:270`).
- **NetBSD** does the same through `COMPILE_CTFCONVERT` (`sys/conf/Makefile.kern.inc:174-195`) and `CTFMERGE` (`:249`), with `MKCTF`.

CTF is rebuilt from DWARF, so it reflects the DWARF the compiler wrote, not the source. Two compilers describing the same types differently give two different CTF graphs. DTrace scripts that name members, and the kernel's own `ctf_*` consumers, then see different types.

`rb ctf-diff` compares, for the kernel and every module:
1. **Types:** the type graph after `ctfmerge`, as a canonical dump (`ctfdump -t`), per type name. A struct's members, offsets, bit offsets and sizes must be identical. Anonymous types are matched by the path that reaches them.
2. **Functions and data objects:** the CTF function and object sections, which say which symbols have types and what they are.
3. **Losses:** `ctfconvert` warnings. A warning in R that G does not have, such as an unsupported DWARF form or a missing base type, is a finding.

The rule is the same as for warnings: rucc's DWARF may be better than GCC's for a debugger, but it must not make CTF that differs. Known risks:
- **base type names.** GCC writes `long unsigned int` and `char`, while rucc may write `unsigned long`. CTF keys base types by name.
- **bitfield encoding.** DWARF 4 `DW_AT_data_bit_offset` against the older `DW_AT_bit_offset`. `ctfconvert` supports both in current FreeBSD, but its NetBSD copy may not.
- **`DW_TAG_restrict_type`, `DW_TAG_atomic_type`**, and the order of qualifiers in a chain.
- **declarations against definitions** of the same struct across units, which `ctfmerge` unifies by name.

**Gate (B3 for NetBSD, B5 for FreeBSD).** For every graded kernel, `ctf-diff` reports no type, function or object difference, and `ctfconvert` gives no warning that G does not give. FreeBSD's DTrace test suite (`cddl/usr.sbin/dtrace/tests`) runs as part of 12.2. NetBSD has no DTrace suite in `tests/`, so `ctf-diff` is its only check, plus a scripted `dtrace -l` and one `fbt` probe per boot.

## 09.7 Separate debug files

FreeBSD splits debug information out of every kernel module and the kernel (`kmod.mk:238-249`, `objcopy --strip-debug --add-gnu-debuglink` and `--only-keep-debug`), and of every userland binary with `MK_DEBUG_FILES`. NetBSD does the same with `MKDEBUG`. These steps use `objcopy` from the reference binutils on rucc's objects, which is a third tool reading rucc's ELF.

Checked by the `debug` row of `rb sections-diff`, which compares the section list of every `.debug` file. `kgdb` and `gdb` are run against a rucc kernel core at B5 with five scripted commands: `bt`, `info registers`, `p` of a struct, `list`, and `frame`. They must give the same output shape as for a G kernel, in the order the reference's debug files give it.
