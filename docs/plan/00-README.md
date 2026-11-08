# Spec 2131/corpus/bsd: FreeBSD and NetBSD, unmodified, built by rucc

rucc builds an unmodified FreeBSD and an unmodified NetBSD: kernel, modules, libc, the runtime linker, the boot code and the whole userland, with every C unit and every assembly file the build gives to the C compiler. The system boots, and it passes the system's own test suite: every test case that passes when the reference GCC builds the same tree with the same configuration. That holds for every supported release and every maintained branch of both systems, on amd64, aarch64 and riscv64, and on i386 where it is still supported. On amd64 the system runs as fast as the reference's, and rucc builds it at least twice as fast. Every compiler change lands in `tamnd/rucc`. The harness lives in a new repository, `tamnd/rucc-bsd`. The existing repositories grow to carry what the BSDs teach: `tamnd/rucc-compat`, `tamnd/rucc-corpus`, `tamnd/rucc-real-corpus` and `tamnd/rucc-cross`. Code the three harnesses share moves to a new one, `tamnd/rucc-harness`.

Written 8 November 2026, against rucc 0.30.15 (`tamnd/rucc` main, commit `a0d77899d`). The pins are FreeBSD `main` at `665afd2a9` (7 November 2026), 15.1-RELEASE and 14.5-RELEASE, with 15.2 in BETA; and NetBSD trunk (11.99.10) at `36eddebcc` (4 November 2026), 11.1 and 10.2. This folder sits beside `corpus/kernel` and follows its shape, its rules and most of its machinery. It adds a level to rung 4 of the parent target ladder (document 14.5), as 15.1 proposes. Where it disagrees with the kernel folder, this one is about a different system and says why.

## Why a BSD is different from the Linux kernel

The kernel folder made rucc a systems compiler. A BSD asks for four things the kernel did not.

**A whole operating system, not a kernel.** FreeBSD and NetBSD are each a kernel and a userland built from one tree by one command, `make buildworld buildkernel` or `build.sh release`. Together they are about 29 million lines of C (01.6): 11 million in the two kernels and 18 million in userland. They include libc, `rtld`, threads, the start files, the boot loaders and every tool in `/bin` and `/usr/bin`. A wrong struct layout in a header is wrong in a thousand programs at once. Document 08 covers what that means for the ABI, and 09 how it is caught before anything boots.

**The build asks the compiler, then configures itself by the answer.** FreeBSD parses `cc --version` with `awk`, and today it reads rucc as GCC 2.0.0 (03.3). NetBSD does not ask at all: it trusts `HAVE_GCC=14` and passes GCC 14's whole warning set. Both build almost everything with `-Werror`. A warning rucc gives and GCC does not is a build failure, so warning parity is a correctness property here, as it was not for Linux (03.6, 06.9).

**Both base systems contain C++, and rucc is a C compiler.** FreeBSD has had no switch for C++ since January 2023: `devd`, `config(8)`, `dtc`, `libc++` and LLVM are always built. NetBSD's GCC, ATF's C++ half, GDB and groff are C++, and its `MKCXX=no` drops them. Document 10 draws the line, and the census says exactly which objects fall on each side.

**There is a real test suite.** Linux tests need hardware and a choice among suites. Both BSDs ship ATF tests in `/usr/tests`, about 9,100 cases in FreeBSD and 11,600 in NetBSD, and run them in CI on virtual machines. That makes "passes" much stronger than "boots", and it lets the oracle be the reference's own results on the same machine.

There is a fifth difference, which shapes the plan: **FreeBSD's own compiler is clang, not GCC.** Its GCC build path is maintained, with ports and CI jobs, but those jobs were failing on all three branches in October 2026 (01.2). The reference for FreeBSD is therefore a second path that sometimes breaks, and 04.6 says what to do when it does.

## Where we start from

Document 03 is the full inventory. The short version:

**What already works.** The C front end, the optimizer, the x86-64 and AArch64 back ends, the integrated assembler with its macro language, i686, `.code16`, `asm goto`, the kernel code model, no-FPU code, the stack protector and `-fstack-usage` are already there from `rucc-kernel`. Linker sets, symbol versions, `ifunc` and TLS models all work in probes (03.7).

**What is missing or wrong:**
- **Every BSD target is refused** (03.1). Nothing in this folder has been built on a BSD yet.
- **File-scope `asm` has three wrong-output bugs** (03.2): `.weak` across blocks, `.ident`, and `.pushsection` flags. Every NetBSD file has a file-scope block through `__RCSID`.
- **FreeBSD reads rucc as GCC 2.0.0,** because of a version number in the license line (03.3).
- **Four flags are refused:** `-finline-limit=` and `-fdiagnostics-show-option` from FreeBSD's kernel, and `-mrtd` and `--param min-pagesize=` from FreeBSD's `boot2` (03.4).
- **The `freebsd_kprintf` format** is an error (03.5).
- **`__has_extension` and `__has_feature`** answer differently from GCC (03.13).
- **GCC's unwinder builtins** are unknown to rucc, and NetBSD builds `libgcc_eh` with `${CC}` (03.14).
- **Warning parity is unmeasured** (03.6).

About 19 to 26 engineer-weeks of rucc work is visible now. The unknown is the code generation bugs that 29 million lines of new C will find.

## The goal, stated so it can be falsified

Document 02 states each claim precisely. In short:

**Unmodified.** The source is the pinned upstream commit or the release's signed source distribution. Nothing is patched, added or regenerated. Configuration is a toolchain description the system documents, a short list of `src.conf` and `mk.conf` knobs each with a reason, a kernel configuration the tree ships or `RB-TEST`, and the build's documented make variables. All of it applies to G as well.

**Configured the same.** Before any C is compiled, R and G agree on every compiler-dependent make variable, the effective knob set, the kernel option headers and every unit's command line (`rb mk-diff`, `rb flags-diff`).

**Passes.** The whole graded build succeeds, the image boots under QEMU, the smoke suite passes, every test case G passes in all three of its baseline runs R passes, and the kernel log has nothing new. Every failure is attributed to the kernel or the world by the 2x2 of 12.6 before it is filed.

**All versions.** FreeBSD `main`, `stable/15`, `stable/14` and every supported release; NetBSD trunk, `netbsd-11`, `netbsd-10` and every supported release. A new release joins the day it is tagged.

**Performance.** On amd64, for the current release of each system: R's geometric mean within 2% of G's over about 60 benchmarks, none more than 5% behind, kernel and libc text within 5%. Compile CPU time over the C units at most half the reference's, with no rucc process over 1.5 GB.

**Architectures.** amd64 first, with the 32-bit compatibility libraries from the first world. Then aarch64 and riscv64, then i386 where it is still supported.

**Which rows carry the claim.** The `graded` rows of 02.1. For FreeBSD that configuration removes LLVM, which is C++ only and is built by `g++` in both R and G. Every report states the census next to the result: the share of objects built by rucc, by the C++ compiler, and by bring-up (always zero in a graded run). The `c-only` rows are graded too, and they are where rucc's share is largest, but they are not the headline (10.5).

## Settled decisions

**NetBSD first, FreeBSD second.** NetBSD's own compiler is GCC, so G is exactly what NetBSD ships. It builds from Linux by design, it has no `--version` parsing to get wrong, and it can drop C++ with a supported switch. FreeBSD follows one milestone behind on each half, kernel and world. The two share most of their compiler surface, so the second system costs less than the first (16).

**DragonFly at B11. OpenBSD is outside the claim.** DragonFly is GCC 12.5 on one architecture with a documented plug-in for other compilers, but it has no test suite, so its claim is weaker: builds, boots, passes smoke, builds itself. OpenBSD builds its kernel and libc with options that exist only in OpenBSD's clang, and has no GCC reference on any architecture in use. Open question 3 says what would change that (04.2).

**The reference is GCC, chosen per pin.** For NetBSD it is the in-tree GCC that `build.sh tools` builds: 14.4.0 on trunk, 12.5.0 on `netbsd-11`, 10.5.0 on `netbsd-10`. For FreeBSD it is the `amd64-gcc15` package, GCC 15.2.0 with the port's patches, because GCC 14 cannot build FreeBSD's C++ today. rucc wears the reference's version as its persona, which decides `__GNUC__`, the banner, and which warnings are on (04.3). The official clang-built FreeBSD release is reported beside G, never graded against.

**The C++ line.** In R, every C unit and every `.S` file given to the C compiler slot is compiled by rucc. Every C++ unit is compiled by the reference `g++`, the same binary G uses. The `rb-c++` shim records each call so the census can say exactly what rucc did not build. rucc gets no C++ front end in this folder (10).

**rucc is the compiler and the assembler. The linker is binutils.** Both systems link with BFD `ld` under GCC, and so does R. Every `.S` file goes through `${CC}`, so through rucc's integrated assembler. `rb-as` records any direct assembler call, so nothing bypasses rucc unseen.

**Bring-up may hand units to the reference, and nothing measured that way counts.** There are exactly two delegations, both through `RB_BRINGUP` (14.4):
- **`as`** sends `.S` files and units with inline or file-scope `asm` that rucc cannot assemble to the reference GNU as.
- **`cxx-c`** sends named C units to the reference GCC. The census records them with origin `ref-c`. It exists for the few C units rucc cannot yet compile at all, such as NetBSD's unwinder before B4 (03.14).

Either one makes the whole run ungraded, and the report says so in its first line. B2's first complete builds are expected to use them; nothing from B3 on may.

**The oracle is G, run three times on the same machine.** A test case counts only if G passed it in all three runs. The projects' published runs are cross-checks on our rig, not the oracle (12.1).

**Performance is a gate,** for the same reason as in the kernel folder: a base system is branchy, pointer-chasing, lock-taking C, which is where a code generator shows (13).

**Compiler fixes go to rucc, tools go to rucc-bsd, reductions go to the corpora.** `rucc-bsd` contains no compiler code, no source patches and no copied source files. Every miscompile found through a BSD is reduced, and the reduction lands in `rucc-corpus` or `rucc-compat` with the fix (14.1, 15).

**`rucc-harness` is factored out at B0.** The kernel folder copied shared code from `rucc-postgres` and promised to factor it out when a third harness needed it. `rucc-bsd` is the third (15.6).

## The documents

| | | |
|---|---|---|
| 00 | this file | the goal, the settled decisions, the reading order |
| 01 | `01-research-2026.md` | releases, base compilers, GCC build paths, tests, sizes, prior art |
| 02 | `02-the-goal.md` | the claims, stated precisely, and what does not count |
| 03 | `03-where-rucc-stands.md` | the gap inventory, probed against rucc 0.30.15, with sizes |
| 04 | `04-systems-versions-and-references.md` | the four systems, the sets and pins, references and personas, where builds run |
| 05 | `05-build-systems-and-identity.md` | how each build identifies a compiler, putting rucc in place, the configuration differentials |
| 06 | `06-language-and-builtins.md` | the C dialect, attributes, builtins, inline `asm`, formats, warnings |
| 07 | `07-assembler-and-file-scope-asm.md` | the `.S` files, file-scope `asm` and the directive reader, boot code assembly |
| 08 | `08-codegen-abi-and-runtime.md` | targets and link lines, kernel flags, no-FPU code, unwinding, libc shapes, boot code, frames |
| 09 | `09-linking-images-modules-debug.md` | linker sets, ABI notes, modules, library ABI, object diffs, CTF, debug files |
| 10 | `10-the-cxx-boundary.md` | the C and C++ line, the census, the `c-only` rows |
| 11 | `11-architectures.md` | amd64, `lib32`, aarch64, riscv64, i386 |
| 12 | `12-boot-and-tests.md` | the oracle, images and boots, `rb-rc`, baselines, the 2x2, mixed objects, foreign binaries |
| 13 | `13-performance.md` | the benchmark set, the method, compile time |
| 14 | `14-rucc-bsd.md` | the new repository, its commands, shims, machines and reports |
| 15 | `15-sibling-repos.md` | what each sibling repository gains, and when |
| 16 | `16-milestones.md` | B0 to B12, exit criteria, costs |
| 17 | `17-open-questions.md` | the ranked list, and when each has to be answered |

Read 02, 03 and 16 first. Then read 05 and 10, because configuration and the C++ line are where a BSD differs most from the kernel. The rest is reference for whoever picks up a milestone.
