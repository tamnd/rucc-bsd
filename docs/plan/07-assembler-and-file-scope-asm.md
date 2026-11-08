# The assembler and file-scope `asm`

rucc has no external assembler, by design, and that does not change here. Both BSDs assemble their `.S` files through `${CC}`, so rucc's integrated assembler takes every assembly file in both base systems. The good news is that `rucc-kernel` built the whole GNU macro assembler for Linux, which is a harder customer than either BSD. The new problem is smaller and sharper: the BSDs' system headers put directives in file-scope `asm` blocks, one directive per block, and rucc's shortcut for those blocks gets three of them wrong (03.2).

## 07.1 What gets assembled

| | `.S` and `.s` files | Lines | Mostly |
|---|---|---|---|
| FreeBSD kernel `sys/` | 347 | 600,000 | generated OpenSSL `perlasm` in `sys/crypto`, then `amd64`, `arm64` and `riscv` locore, exception and copy routines |
| FreeBSD userland, without `contrib/llvm-project` | 416 | 61,000 | libc string and syscall stubs, `csu`, `rtld` trampolines, `libthr`, `stand` |
| NetBSD kernel `sys/` | 801 | 261,000 | `arch/*/` locore and trap code, `lib/libkern` string routines, `crypto` |
| NetBSD userland | 1,991 | 1,368,000 | generated crypto and GMP in `external/`, then libc, `csu`, `ld.elf_so`, `libpthread` |

Those are counts from the trees of 7 November 2026 (01). The generated OpenSSL files dominate both, and they are the same `perlasm` output that `rucc-real-corpus`'s OpenSSL project already assembles. That project assembles them for Linux. BSD flavours use the same instruction set and differ only in a few directives: `.type`, `.size` and the `.note.GNU-stack` section.

Each system's own assembly comes through a macro package in `<machine/asm.h>`:
- **FreeBSD:** `ENTRY`, `END`, `ALTENTRY`, `WEAK_REFERENCE`, `PIC_PLT` and `PIC_GOT`, plus `<machine/asmacros.h>` in the kernel.
- **NetBSD:** `_ENTRY`, `ENTRY`, `NENTRY`, `END`, `RCSID`, `STRONG_ALIAS`, `WEAK_ALIAS` and `_PROF_PROLOGUE`.

These are C preprocessor macros that expand to plain directives, with a few uses of `.macro` in boot code and libc. Nothing comes close to Linux's alternatives or exception table macros.

## 07.2 Directives and flags the BSDs need

From the `.S` files, the `asm.h` headers and the file-scope blocks (07.3):

| Directive or flag | Where | State |
|---|---|---|
| `.symver name, name@VER`, `@@VER`, `@@@VER` | FreeBSD `__sym_compat` and `__sym_default` (`sys/sys/cdefs.h:431-434`) and libc `.S` files | works, matches clang (probed) |
| `.section .gnu.warning.SYM` then `.asciz`, then `.previous`, **as three separate blocks** | FreeBSD `__warn_references` (`cdefs.h:426`) | works (probed). The section state carries across blocks |
| `.pushsection NAME,"MS",@progbits,1`, `.asciz`, `.popsection` | NetBSD `__RCSID`, `__KERNEL_RCSID`, `__COPYRIGHT` via `__SECTIONSTRING` (`sys/sys/cdefs_elf.h:139-175`) | **wrong flags** in the directive reader (03.2 bug C); correct through the assembler |
| `.weak` and `.equ` in two blocks | FreeBSD `__weak_reference` (`cdefs.h:423`) | **wrong binding** (03.2 bug A) |
| `.weak alias` with `alias = sym`, `.global alias` with `alias = sym` | NetBSD `__weak_alias` and `__strong_alias` (`cdefs_elf.h:55-61`) | works, because the reader declines `=` and hands the block to the assembler |
| `.ident "string"` | contributed code; old FreeBSD `__FBSDID` | **dropped** (03.2 bug B) |
| `.section .note.tag,"a",@note`; `.note.netbsd.ident` and `.note.netbsd.pax` | FreeBSD `crtbrand` and `ignore_init_note`; NetBSD `sysident.h` in `crt0` and `crti` | correct in `.S`; wrong in file-scope blocks (bug C) |
| `.code16`, `.code32`, `.code64` | boot blocks, kernel trampolines (`mpboot.S`, NetBSD `mptramp.S`) | exist (`crates/rucc-asm/src/sixteen.rs`) |
| `.set`, `.equ` with expressions; `.org`; `.fill`; `.space` | boot blocks, which must hit exact offsets | exist; byte exactness checked by `bsd-asm` |
| `.cfi_*` | everywhere with `-fasynchronous-unwind-tables` in userland | exists |
| `-Wa,--fatal-warnings` | NetBSD `bsd.sys.mk` and the kernel makefiles | must be accepted. With it, every assembler warning rucc gives is an error, the same issue as 06.9 |
| `-Wa,--noexecstack`, `-Wa,-many` and others | the census lists them (14.5) | the allow-list of `rucc-kernel` K1 |

## 07.3 File-scope `asm`

GCC emits a file-scope `asm` block's text into its assembly output in source order, between the definitions around it. GNU as then processes it with the same section stack and symbol table as the compiler's own output. That gives three properties, and every BSD header idiom relies on them:

1. **Order.** A block is processed after everything before it in the file and before everything after it.
2. **Shared state.** The section stack, the current section and the symbol table are shared across blocks and with compiled code. A `.weak foo` in one block and `.equ foo, bar` in the next act as one. `.section x` in one block and `.previous` in a later block return to whatever section the compiler was in.
3. **Full fidelity.** Every operand of every directive counts: section flags, type, entry size, group name and linkage, and every symbol attribute.

rucc's design has two paths:
- **The assembler path.** All the file's file-scope blocks go through the integrated assembler, sharing the object writer with the code generator. This path has all three properties.
- **The directive reader** (`crates/rucc-lower/src/directives.rs`). Directive-only blocks are turned into IR globals and linkage edits, so they take part in rucc's own global handling. It is fast and it serves the common Linux case. It breaks property 2 for `.weak` (bug A) and property 3 for `.pushsection` flags (bug C) and for `.ident` (bug B).

**The rule from here on.** The reader may translate a block only if it can reproduce, exactly, everything GNU as would do with that block in that position, given the state the earlier blocks left. Anything else goes to the assembler path for the whole file. In practice:

| Directive | Reader may translate it when | Otherwise |
|---|---|---|
| `.globl`, `.local`, `.hidden`, `.protected` on a name defined in C | always | |
| `.weak NAME` | NAME is defined in C in this file, or is declared but never defined | assembler path. This covers bug A: a `.weak` on a name that a later block defines |
| `.set` and `.equ` | both names are defined in C and no earlier block touched them | assembler path |
| `.section`, `.pushsection`, `.popsection`, `.previous` with data directives | never, at first | assembler path. This covers bug C. The reader may later learn flags, types, entry sizes and groups if profiling shows the assembler path costs measurable time, with `bsd-asm` as its test |
| `.ident`, `.symver` | the reader passes them through to the object writer unchanged | |
| anything else | never | assembler path |

**The test.** Every BSD idiom of 07.2, and every combination of two of them in one file in both orders, is compiled twice in `rucc-corpus`'s `file-scope-asm` facet:
- by rucc's normal path;
- by rucc with `-frucc-asm-reader=off`, a debugging flag that forces the assembler path.

The two objects must be identical, section by section and symbol by symbol, and both must match GNU as on GCC's `-S` output. `bsd-asm` (15.2) runs the same comparison over every file-scope block found in either tree by `rb demands`.

**Why not drop the reader.** It may be the right answer. Nobody has measured what the assembler path costs per file against the reader. A file-scope block in every file, which is the BSDs' case through `__RCSID` and `__FBSDID`, would send every file down the slower path. B1 measures both ways on NetBSD's userland. If the reader saves less than 1% of build time, it is removed rather than fixed.

## 07.4 Boot code assembly

The boot blocks are the only assembly here where the byte layout is checked by hardware:
- **FreeBSD:** `stand/i386/boot0`, `boot2`, `btx`, `cdboot` and `pxeboot`, plus `stand/efi` for UEFI.
- **NetBSD:** `sys/arch/i386/stand/mbr`, `bootxx`, `boot` and `efiboot`.

They mix `.code16`, absolute `.org` offsets, `.fill` to a sector boundary, and C built with `-m32 -Os` (08.6). They are linked with explicit addresses and checked by size:
- FreeBSD's `boot2` build fails if `boot2` exceeds its space;
- NetBSD's `bootxx` must fit its sectors.

`bsd-asm` compares them byte for byte with GNU as. Any difference there is a bug, because a boot sector has no room for "equivalent".

## 07.5 What the census will add

`rb demands` lists every mnemonic in every `.S` file and every inline `asm` template, per row. Against `rucc-kernel`'s inventory, the expected additions are:
- the **aarch64 and riscv64 locore** of both systems;
- **NetBSD's `ld.elf_so` trampolines**, written for `as` with a few local macros;
- **FreeBSD's `stand/` loader**, with its 16-bit BIOS interfaces.

Each missing mnemonic class gets an issue at B0, as `rk asm-inventory` did for Linux.
