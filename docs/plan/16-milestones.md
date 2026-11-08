# Milestones

Thirteen milestones, B0 to B12. Each one gives:
- its scope;
- the work in each repository;
- an exit criterion that a command in `rucc-bsd` checks;
- a cost in engineer-weeks.

The order follows three decisions made earlier in this folder:
- **NetBSD goes first** (00, 04.1). Its own compiler is GCC, it builds from Linux, and it can drop C++.
- **The kernel goes before the world** on each system. A kernel is one link of C with no C++ in it, and the 2x2 (12.6) can grade it against G's world before rucc builds any userland.
- **amd64 goes before every other architecture,** because it is the only one with KVM and the performance gate.

## Summary

| | Scope | Rows | Cost | Critical path through |
|---|---|---|---|---|
| B0 | the instrument: `rucc-harness`, pins, G baselines, census | NetBSD trunk, FreeBSD 15.1 and `main`, amd64, G only | 4 | the FreeBSD builder VM and the first G baseline |
| B1 | rucc speaks BSD: targets, file-scope `asm`, identity, feature-test macros, driver flags, FreeBSD host | user programs on stock 15.1 and 11.1 | 6 | the link line and start files per system |
| B2 | configured the same: probes, `mk-diff`, `flags-diff`, `syntax`, warnings, `freebsd_kprintf`, the first `sections-diff` | NetBSD trunk and FreeBSD `main`, amd64, builds only | 6 | warning parity under `-Werror` |
| B3 | NetBSD amd64 kernel boots and passes under G's world | NetBSD trunk amd64 `graded` | 5 | CTF |
| B4 | NetBSD amd64 world: RR passes; `c-only`; boot code; unwinder builtins; `compat` | NetBSD trunk, `netbsd-11`, 11.1, amd64 | 8 | the unwinder builtins and the boot blocks |
| B5 | FreeBSD amd64 GENERIC with CTF, modules and `kgdb` | FreeBSD `main` and 15.1 amd64 `graded` | 5 | `-finline-limit` and the frame flags |
| B6 | FreeBSD amd64 world: RR passes; `c-only`; `lib32`; `boot2`; symbol versions; `ifunc` | FreeBSD `main`, `stable/15`, 15.1, amd64 | 7 | `boot2` size and `-mrtd` |
| B7 | aarch64 and riscv64 on both systems | the current release of each, two architectures | 6 | TCG test time |
| B8 | performance: the runtime and compile time gates of 02.5 | FreeBSD 15.1 and NetBSD 11.1 amd64 | 8 | whatever the profiles show |
| B9 | every version and branch of 02.4 | the whole current and releases sets | 3 | NetBSD 10's persona |
| B10 | rucc as the system compiler on a rucc system | FreeBSD amd64 RR image | 4 | the self-build fixed point |
| B11 | DragonFly; the OpenBSD decision | DragonFly master and 6.4.2 | 4 | a native builder |
| B12 | host tools graded; i386; steady state | NetBSD i386, FreeBSD `stable/14` i386 | 4 | the 32-bit kernels |

Total: 70 engineer-weeks before contingency, about 77 with it (16.15). That is about one and two thirds engineer-years. Of it, 19 to 26 weeks are the rucc work 03.15 can already see. The rest is the harness, the rigs, the baselines, and the code generation bugs no probe can find before a build.

## 16.1 B0: the instrument

**Scope.** Nothing is built by rucc. At the end of B0 the harness can fetch, build, image, boot and test G for one row of each system, three times, and say what the builds demand from a compiler.

**rucc-harness.** Created (15.6). The four crates `harness-shim`, `harness-agent`, `harness-stats` and `harness-elf` move out of `rucc-kernel` and `rucc-postgres` with their tests. `rk` and `rpg` are not required to move yet.

**rucc-bsd.**
- The workspace of 14.2 on `rucc-harness`.
- `rb fetch`, `rb sets --write`, `rb personas` and the files `sets.toml`, `pins.toml`, `personas.toml`, `toolchains.toml`, `builders.toml` and `rows.toml`.
- The FreeBSD 15.1 builder VM image with the pinned `amd64-gcc15` and `binutils` packages, and the NetBSD `build.sh` container image (14.6).
- `rb-cc`, `rb-c++`, `rb-cpp` and `rb-as` in recording mode (14.4).
- `rb image`, `rb boot`, `rb-rc` and the smoke suite (12.3, 12.4).
- `rb baseline` and the `baselines/` format (12.5).
- `rb census` and `rb demands` over G builds (10.4, 14.5).

**rucc.** The issues of 15.1 opened, each with its milestone. The two spec edits of 15.1 proposed.

**Measurements B0 owes the rest of the plan.**
- Build and test times per row on gpc, replacing the estimates of 14.6.
- The size of `compile.jsonl` per build, and the disk a baseline takes (open question 9).
- Whether each FreeBSD branch has a G-good commit in the last 60 days (open question 1).
- The census of G: how many objects each slot builds, and the share of C units in the `graded` and `c-only` configurations.
- The demand census: the features rucc lacks, ranked by the units they block.

**Exit criterion.**
1. `rb sets --write` produces the pins of 04.5 from what the projects publish, with no hand edits.
2. `rb baseline netbsd-trunk --row amd64 --conf graded` and `rb baseline freebsd-15.1 --row amd64 --conf graded` each complete three G runs. Every suite of 12.2 that applies runs to the end, and the counts are within 2% of the project's published runs (12.1). A larger gap is a rig bug, fixed before B0 closes.
3. A fourth G build of each is byte-identical to the first (12.5).
4. `rb demands` and `rb census` have written `demands.toml` and a census for both builds. The list of missing features is attached to the issues of 15.1.

**Cost.** 4 weeks. Most of it is the FreeBSD builder VM and getting the two published test counts reproduced.

## 16.2 B1: rucc speaks BSD

**Scope.** rucc accepts the BSD targets, compiles and links user programs that run on stock FreeBSD 15.1 and NetBSD 11.1, runs on FreeBSD as a host, and answers the identity and feature questions the way the reference does.

**rucc.**
- **Targets (03.1, 08.1).** FreeBSD, NetBSD, DragonFly and OpenBSD in `rucc-target::Os`. Predefined macros, `char` signedness, `wchar_t`, `long double`, PIE defaults, the dynamic linker, the start files, `libgcc` against `libcompiler_rt`, and the link line, checked against the reference's `-###` for each system. **L.**
- **File-scope `asm` (03.2, 07.3).** Bugs A, B and C. The reader rule of 07.3, with `-frucc-asm-reader=off` as a test switch. B1 measures what the reader saves on NetBSD's userland; if it saves less than 1% of build time, it is removed and every block goes to the assembler. **M.**
- **Identity (03.3).** `cc --version` that `bsd.compiler.mk` parses as the persona's GCC version. **S.**
- **Feature-test macros (03.13).** `__has_extension` and `__has_feature` answer as the reference does under each persona. **S.**
- **Driver flags (03.4).** `-finline-limit=`, `-fdiagnostics-show-option`, `-mrtd` and `--param min-pagesize=`. **S.**
- **FreeBSD host (03.11).** rucc builds and passes its own suite on FreeBSD 15.1 amd64, and the release job publishes the artifact. **S.**

**rucc-compat.** `bsd-predef` and `bsd-abi` (15.2), generated on the builders against the reference GCC of each system.

**rucc-corpus.** The facets `file-scope-asm`, `linker-sets` and `crt-and-brand` (15.3).

**rucc-cross.** Every BSD fact in its tables checked against the reference GCC on the builder. Sysroots from the release's `base.txz`, and from `base.tgz` and `comp.tgz`. FreeBSD and NetBSD rows in the "run rucc's output under QEMU" job (15.5).

**rucc-bsd.** `rb-cc` able to run rucc. `conf/freebsd/rucc.mk` and the NetBSD `TOOLDIR` overlay (05.4). `rb toolchain-diff` and `rb probes`.

**Exit criterion.**
1. `bsd-predef` shows no difference for `x86_64`, `aarch64` and `riscv64` on FreeBSD and NetBSD, under every persona of 04.3, apart from the lines that name the compiler.
2. `bsd-abi` shows no layout difference over every installed header of FreeBSD 15.1 and NetBSD 11.1 amd64.
3. `rucc-cross`'s QEMU job runs rucc's test programs on stock FreeBSD 15.1 and NetBSD 11.1, with the same results as on Linux.
4. rucc's own test suite passes on the FreeBSD 15.1 builder, built from the commit under test.
5. `rb toolchain-diff` is clean for both systems.
6. The three `rucc-corpus` facets pass, and the bug reproducers of 03.2 pass with the reader on and off.
7. The reader decision of 07.3 is recorded, with the measurement.

**Cost.** 6 weeks. The targets are the largest item; the link line per system is where the surprises are.

## 16.3 B2: configured the same

**Scope.** R reaches the same configuration as G, every C unit of both systems' amd64 builds goes through rucc's front end without an error, and the first R builds complete. Nothing has to boot yet. The two bring-up delegations of 00 may be used, so B2's builds are ungraded.

**rucc.**
- **`freebsd_kprintf` (03.5, 06.6)** with `%b`, `%D`, `%r` and `%y`, and the warnings the reference gives for each. **M.**
- **Warning parity, first pass (03.6, 06.9).** Every warning rucc gives under `-Werror` that G does not give is fixed, or shown to be a persona error. **L.**
- **The rest of the driver.** Every flag `rb probes` found refused; `--sysroot` and `-B` as `buildworld` uses them; `-print-prog-name=ld`.
- **Probe speed.** Every probe under 20 ms (05.8).

**rucc-compat.** `bsd-probes` and `bsd-pp` (15.2).

**rucc-corpus.** `kprintf-format` and `werror-shapes`.

**rucc-bsd.** `rb mk-diff`, `rb flags-diff`, `rb syntax`, `rb warnings`, `rb sections-diff`, `rb symbols-diff` (05.5 to 05.7, 06.9, 09.5). `mk-divergences.toml`, each entry with its reason.

**Exit criterion.**
1. `rb probes` shows no difference for NetBSD trunk and FreeBSD `main`, amd64.
2. `rb mk-diff` and `rb flags-diff` are clean beyond `mk-divergences.toml`, for the `graded` configurations of both.
3. `rb syntax` over every C unit of both G builds gives no error and no warning G did not give.
4. `rb build --cc rucc` completes for both, with `RB_BRINGUP` listing at most the units the census attributes to 03.14 and to the boot code. Each delegated unit has an issue.
5. `rb sections-diff` and `rb symbols-diff` have run on both builds. Every counted difference has an issue, and none of them is bug A, B or C.

**Cost.** 6 weeks, most of it warning parity. The number is a guess until B0's census says how many units carry `-Werror`.

## 16.4 B3: the NetBSD amd64 kernel

**Scope.** A NetBSD trunk amd64 kernel built by rucc, with its modules, boots and passes ATF under G's world. This is the RG cell of 12.6, and it is the first graded result in the folder.

**rucc.**
- **The NetBSD kernel flag line (08.2),** including `-fno-zero-initialized-in-bss` and `COPT_AUTOINIT`, confirmed in a real build. **S.**
- **DWARF for CTF (03.10, 09.6).** Base type names, bitfield encoding and qualifier order as `ctfconvert` reads them. **M.**
- Whatever the first boot finds.

**rucc-compat.** `bsd-asm` for the NetBSD kernel's `.S` files (15.2, 07.4).

**rucc-corpus.** `netbsd-kernel`.

**rucc-bsd.** `RB-TEST` for NetBSD (12.2). `rb test` with the 2x2. `rb ctf-diff`, `rb vec-audit`, `rb frames`, `rb cross-modules` and `rb mixed` for the kernel (12.7).

**Exit criterion.**
1. `rb build netbsd-trunk --row amd64 --conf graded --cc rucc --targets kernel` succeeds with no `RB_BRINGUP`.
2. GENERIC and `RB-TEST` boot under KVM, and the kernel log has nothing G's three runs did not have (02.3, part 5).
3. `rb test` on RG passes every ATF case GG passes.
4. `rb ctf-diff` and `rb vec-audit` report nothing, and `ctfconvert` gives no warning G did not give.
5. `rb cross-modules` passes all four cells.
6. `rb sections-diff` has no counted difference on the kernel and module objects.
7. The nightly has run this row for seven days in a row with every check above green.

**Cost.** 5 weeks.

## 16.5 B4: the NetBSD amd64 world (level A)

**Scope.** All of NetBSD amd64 built by rucc: RR passes 02.3 on trunk, `netbsd-11` and 11.1. This closes level A of the target ladder (15.1).

**rucc.**
- **The unwinder builtins (03.14).** `__builtin_unwind_init`, `__builtin_dwarf_cfa`, `__builtin_eh_return`, `__builtin_dwarf_sp_column` and `__builtin_init_dwarf_reg_size_table`, so `libgcc_eh` and `libgcc_s` build. **M.**
- **Boot code (08.6).** `bootxx_*`, `/usr/mdec/boot` and `bootx64.efi`, within their size limits. **M.**
- **libc shapes (08.5).** `ifunc` in static binaries; TLS in `ld.elf_so` before it relocates itself; all four TLS models.
- **`compat` (11.1).** The `-m32` libraries on amd64, which is the first graded i686 code.

**rucc-corpus.** `unwinder`, `ifunc-static`, `tls-models` and `boot-sizes`.

**rucc-bsd.** `rb abi-diff` (09.4). `rb foreign` for NetBSD (12.8). The `c-only` row with the borrowed `tests` set (10.5). The SeaBIOS boot of every RR image (12.3).

**Exit criterion.**
1. 02.3 holds, parts 1 to 6, for NetBSD trunk, `netbsd-11` and 11.1 amd64, `graded`, on one rucc commit, with no `RB_BRINGUP`.
2. The same for the `c-only` row on trunk.
3. RR boots through rucc's BIOS boot blocks and through rucc's EFI loader.
4. `rb abi-diff` is clean for every shared library, including `compat`.
5. `rb foreign` passes on 11.1: the official userland on R's kernel, and the 30 packages on RR.
6. `rb census` shows every C unit and every `.S` file given to `${CC}` was built by rucc.

**Cost.** 8 weeks: 7 for the world and the boot code, 1 more for the unwinder. The unwinder is the item most likely to overrun, because the test that matters is every C++ program in the system.

## 16.6 B5: the FreeBSD amd64 kernel

**Scope.** FreeBSD `main` and 15.1 GENERIC built by rucc, with every module, CTF and debug files, boot and pass `kyua` under G's world (RG).

**rucc.**
- **The FreeBSD kernel flag line (08.2),** with `-finline-limit=8000`, `-fno-omit-frame-pointer -mno-omit-leaf-frame-pointer`, and the code model. **S.**
- **The module relocations** the kernel linker accepts (`elf_machdep.c:292-373`), for `ld -r` objects. **S** unless `rb cross-modules` finds otherwise.
- **Frames.** 24 KB kernel stacks: `rb frames` against G (08.7).

**rucc-compat.** `bsd-asm` for the FreeBSD kernel.

**rucc-bsd.** `RB-TEST` for FreeBSD. `rb ctf-diff` on FreeBSD's `ctfmerge` output. The `kgdb` script of 09.7.

**Exit criterion.**
1. `rb build freebsd-main --row amd64 --conf graded --cc rucc --targets kernel` succeeds with no `RB_BRINGUP`, and the same for 15.1.
2. RG passes every `kyua` case GG passes, including the DTrace suite.
3. `rb ctf-diff`, `rb vec-audit` and `rb cross-modules` are clean.
4. The five `kgdb` commands of 09.7 give the same output shape on an R core as on a G core.
5. Seven green nights in a row.

**Cost.** 5 weeks. Much of the kernel work is already done by B3 and by `rucc-kernel`; the new parts are the FreeBSD module loader and CTF through FreeBSD's tools.

## 16.7 B6: the FreeBSD amd64 world (level B)

**Scope.** All of FreeBSD amd64 built by rucc apart from the C++ slot: RR passes 02.3 on `main`, `stable/15` and 15.1. This closes level B.

**rucc.**
- **`boot2` (08.6).** `-m32 -Os -mrtd -mregparm=3` as the build passes them, within `BOOT2SIZE=7680`. Then `gptboot`, `loader` and `loader.efi`. **M.**
- **Symbol versions** through `Symbol.map` and `__sym_compat` (08.5).
- **`ifunc`** in `libc` for `memcpy` and its family, in static and dynamic programs.
- **`lib32`** (11.1).

**rucc-corpus.** `symbol-versions`, `ifunc-static` (FreeBSD cases) and `boot-abi`.

**rucc-bsd.** The `c-only` row (10.5). `rb foreign` for FreeBSD: `base.txz` on R's kernel, the 30 packages, `misc/compat14x`. The weekly `full` configuration (02.1).

**Exit criterion.**
1. 02.3 holds for FreeBSD `main`, `stable/15` and 15.1 amd64, `graded`, on one rucc commit, with no `RB_BRINGUP`.
2. The same for `c-only` on `main`.
3. `boot2` built by rucc fits, and RR boots through `gptboot` under SeaBIOS and through `loader.efi` under OVMF.
4. `rb abi-diff` is clean, including symbol versions and `lib32`.
5. `rb foreign` passes on 15.1.
6. The weekly `full` build of `main` completes in R, with LLVM built by `g++` in both.

**Cost.** 7 weeks. `boot2` is small but has no margin: 7,680 bytes, and a compiler that is 3% larger than GCC at `-Os` does not fit.

## 16.8 B7: aarch64 and riscv64 (level C, first half)

**Scope.** The current release of each system on aarch64 and riscv64, `graded`, built by rucc and tested under QEMU.

**rucc.**
- **The GNU property note (11.2).** BTI, PAC and GCS bits equal to the reference's for each `-mbranch-protection` value. Open question 12 says whether GCC 15.2's `standard` sets GCS.
- **`x18`** never used in FreeBSD's kernel (`-ffixed-x18`), checked by `rb vec-audit`'s extension.
- **RISC-V relaxation relocations** on exactly the instructions GCC marks (11.3).
- **The census of inline `asm`** for `h` and `svinval` instructions.

**rucc-bsd.** aarch64 and riscv64 rows in `rows.toml`, with firmware. NetBSD riscv64 with `MKCOMPAT=no` (02.1). The TCG cadence of 11.5: nightly boot and smoke, weekly full suite, sharded.

**Exit criterion.**
1. 02.3 holds for FreeBSD 15.1 and NetBSD 11.1 on aarch64, and FreeBSD 15.1 and NetBSD 11.1 on riscv64, in a weekly run.
2. `rb sections-diff` shows identical `.note.gnu.property` contents on every aarch64 object.
3. Nightly boot and smoke have been green for 14 days on all four rows.

**Cost.** 6 weeks. The rucc work is small. The cost is TCG time and the riscv64 baseline, which nobody has published (12.5).

## 16.9 B8: performance (level C, second half)

**Scope.** The gates of 02.5 on FreeBSD 15.1 and NetBSD 11.1 amd64.

**rucc.** Whatever the profiles show. B8 starts from two lists, not from guesses:
- the 50 units with the worst compile time ratio (13.4);
- the benchmarks more than 2% behind, each narrowed by `rb mixed --perf` and `pmcstat` or `tprof` (13.3).

Each fix carries a `rucc-real-corpus` row or a `rucc-corpus` performance case.

**rucc-bsd.** `rb-bench` and `rb bench` (13.2). `rb speed` with the gates. The server3 setup of 13.3.

**Exit criterion.** Two consecutive weekly runs on the same rucc commit, for each system:
1. the geometric mean of R over G is at least 0.98;
2. no gated benchmark is below 0.95;
3. kernel and `libc.so` text are within 5% of G's;
4. the compile CPU ratio over C units is at most 0.5 for the world and for the kernel;
5. no rucc process exceeded 1.5 GB.

**Cost.** 8 weeks, with high uncertainty. The compile time gate is the one most likely to move it: BSD userland is many small units, where start-up and header parsing dominate (13.4).

## 16.10 B9: every version

**Scope.** Every member of 02.4's current and releases sets, on every row graded so far.

**rucc.**
- **Personas 10.5.0 and 12.5.0** for NetBSD 10 and 11, with their warning sets (06.9). NetBSD 10's GCC 10 enables fewer warnings than rucc's native set, so this is the persona with the most to turn off.
- **FreeBSD `stable/14`** with the GCC 14 fallback reference where GCC 15 fails (04.3).

**rucc-bsd.** `rb sets` running daily. Releases joining on the day they are tagged (04.7): 15.2 in December 2026 and 14.6 in March 2027 fall inside the plan.

**Exit criterion.** One nightly and one weekly run, on the same rucc commit, green on every member of both sets for every graded row.

**Cost.** 3 weeks. The oldest member is NetBSD 10.2 with GCC 10.5. Everything else is within two years of the newest tree.

## 16.11 B10: rucc as the system compiler

**Scope.** rucc runs on a rucc-built FreeBSD and builds that same system and real software on it.

**rucc.** Nothing new is expected. Anything found here is a host bug: a rucc binary built for FreeBSD that behaves differently from the Linux one.

**rucc-bsd.**
- A rucc package for FreeBSD, built on the builder and installed into the RR image as a package, not into the base system.
- **The fixed point.** Inside RR, rucc as `XCC` builds the same pin's `graded` world and kernel. With `WITH_REPRODUCIBLE_BUILD` the result must be byte-identical to the R build made on the builder.
- **The run-time compile tests.** The `graded` image has no C compiler (10.3), so `kyua` cases that compile at run time are skipped in GG and RR alike. With the rucc package installed as `cc`, they run in RR. They are reported, not graded, because G has no result for them.
- **A ports row.** A fixed list of about 20 C ports with their own test suites, built by rucc inside RR, run there. Ungraded at first, then graded once green for a month.

**rucc-postgres.** The FreeBSD amd64 row of 15.8, built by rucc inside RR with `meson`, passing `make check-world`.

**Exit criterion.**
1. The self-built world and kernel inside RR are byte-identical to the builder's R.
2. PostgreSQL's FreeBSD row passes in `rucc-postgres`.
3. The ports list builds and passes its tests.

**Cost.** 4 weeks. A NetBSD fixed point depends on rucc running on NetBSD as a host, which depends on Rust's NetBSD support. It is attempted here and reported, not gated.

## 16.12 B11: DragonFly and the OpenBSD decision

**Scope.** DragonFly master and 6.4.2 build with rucc, boot, pass smoke, and build themselves (02.7). OpenBSD gets a written answer to open question 3.

**rucc.** Expected to be small: DragonFly's GCC 12.5 and GNU ld are already personas and tools the folder uses. The `DragonFly` arm of `rucc-target::Os` was added at B1 and is now tested.

**rucc-bsd.**
- A native DragonFly builder VM (14.6).
- rucc installed as a `CCVER` compiler through `compilers.conf`, which is a documented configuration file on the builder, not a change to the tree (open question 10).
- Smoke and the self-build, since DragonFly has no structured test suite.

**Exit criterion.**
1. DragonFly master and 6.4.2 build with rucc as `CCVER`, boot, and pass smoke.
2. The RR image builds its own world with rucc, and the result boots.
3. Open question 3 is answered in this folder, with the decision and the evidence.

**Cost.** 4 weeks: a week of builder and harness work, two of fixes, one for the OpenBSD investigation.

## 16.13 B12: host tools, i386 and steady state

**Scope.**
- **Host tools graded.** `bmake`, `nbmake`, `mkimg`, `makefs`, `config(8)` and the rest of 02.8's host list are built by rucc on the builder, in an R-host build. G-host stays the builder's own compiler. Ungraded for a month, then graded.
- **i386** (11.4). NetBSD i386 and FreeBSD `stable/14` i386, with their 32-bit kernels, `-mregparm` and the 64-bit helper calls.
- **Steady state.** The harness runs itself, as `rucc-kernel` K12 does: a new tag graded the day it appears, each regression bisected to a rucc commit automatically.

**rucc.** The i386 kernel flag lines, PAE and non-PAE page tables, `gs`-based per-CPU access. Expected small, because the i686 back end has been graded through `compat` and `lib32` since B4 and B6.

**Exit criterion.**
1. 02.3 holds for NetBSD 11.1 i386 and FreeBSD 14.5 i386.
2. An R-host build of each amd64 row produces the same G-equivalent output as the G-host build.
3. Four consecutive weeks with every set green, and every regression in those weeks bisected and fixed within the week.

**Cost.** 4 weeks, then upkeep of about 10% of one engineer, rising when a system imports a new GCC (NetBSD) or a new hardening flag (both).

## 16.14 Parallel tracks

With two engineers, one per system after B2:

```
Engineer A:  B0 ── B1 (targets, host) ── B2 (probes, mk-diff) ── B3 ── B4 ── B7 ── B9 ── B11
Engineer B:  B0 ── B1 (asm, identity, flags) ── B2 (kprintf, warnings) ── B5 ── B6 ── B8 ── B10 ── B12
```

- B0 to B2 split cleanly by the rucc areas of 03.15. Both engineers need every result of B2 before either system boots.
- B5 needs B2, not B3. B3 helps it, because CTF and most kernel bugs are shared, so B5 starts a week after B3.
- B6 does not need B4's unwinder: FreeBSD's is LLVM's `libunwind` (03.14).
- B7, B8 and B9 need both worlds, B4 and B6.
- B10 needs B6. B11 and B12 need B4.

**Dates, if work starts on 16 November 2026.** B0 to B2 take about 8 weeks elapsed, to mid-January 2027. NetBSD amd64 (B4, level A) and FreeBSD amd64 (B6, level B) both land in April 2027. B7 and B8 (level C) land in June 2027. The plan ends about August 2027, about 9 months with two engineers, or about 10 with contingency. FreeBSD 14.6, 15.3 and possibly NetBSD 12.0 are tagged during it, and B9 takes them in.

## 16.15 Cost table

| | Weeks | Cumulative | Uncertainty |
|---|---|---|---|
| B0 | 4 | 4 | low |
| B1 | 6 | 10 | medium (the link line on four systems) |
| B2 | 6 | 16 | **high** (warning parity, unmeasured until B0's census) |
| B3 | 5 | 21 | medium |
| B4 | 8 | 29 | **high** (the unwinder, boot blocks, first full world) |
| B5 | 5 | 34 | low to medium |
| B6 | 7 | 41 | **high** (`boot2` size, first full FreeBSD world) |
| B7 | 6 | 47 | medium |
| B8 | 8 | 55 | **high** (compile time on small units) |
| B9 | 3 | 58 | low |
| B10 | 4 | 62 | medium |
| B11 | 4 | 66 | medium |
| B12 | 4 | 70 | low |

With 25% contingency on B2, B4, B6 and B8 (29 weeks, so about 7 more), the plan is about 77 weeks: one and two thirds engineer-years. B4 already includes a week for the unwinder builtins; if they take two, B4 is 9.

For comparison:
- `rucc-kernel`'s plan is about 95 weeks. It has a museum, ten years of versions and an objtool to satisfy; this folder has one decade less history and two systems that share most of their compiler surface.
- Level A alone, B0 to B4, is 29 weeks. That is the smallest piece of this plan that stands on its own as a claim: one BSD, built entirely by rucc apart from C++, passing its own tests on amd64.
