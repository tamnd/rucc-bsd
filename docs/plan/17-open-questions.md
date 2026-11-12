# Open questions

Each question says:
- what we believe now;
- what goes wrong if the belief is wrong;
- how to find out;
- the latest milestone by which it must be settled.

A question past its deadline blocks that milestone. The numbers are stable, because other documents cite them. The table ranks them by how much of the plan changes with the answer.

| Rank | Question | Deadline | If the belief is wrong |
|---|---|---|---|
| 1 | 2. Warning parity's size | B2 | B2 doubles, and every later milestone moves |
| 2 | 1. A G-good FreeBSD head | B0 | FreeBSD's current set loses its nightly grade |
| 3 | 4. The C++ share | B2 | the FreeBSD claim needs a different headline |
| 4 | 5. Boot code under G | B4 | the boot code is ungraded on one system |
| 5 | 6. The `TOOLDIR` overlay | B1 | NetBSD's build path changes |
| 6 | 13. The directive reader | B1 | either a speed loss or a correctness risk stays |
| 7 | 12. GCS in GCC 15.2's `standard` | B7 | one aarch64 difference to explain or fix |
| 8 | 14. The riscv64 baseline | B7 | NetBSD riscv64 grades little |
| 9 | 15. A GCC import into NetBSD trunk | continuous | a persona and warning set change mid-plan |
| 10 | 7. arm64 hardware | B7 | aarch64 stays weekly and has no performance gate |
| 11 | 9. Storage on gpc | B0 | fewer pins kept, or another disk |
| 12 | 10. DragonFly's `CCVER`, answered at B11 | B11 | DragonFly needs a different entry point |
| 13 | 8. rucc on a NetBSD host | B10 | no NetBSD fixed point |
| 14 | 3. OpenBSD, answered at B11 | B11 | none for this plan |
| 15 | 16. Reporting results and bugs upstream | B4 | none for the plan; a lot for how it is seen |
| 16 | 11. The museum | none | none for this goal |

## 1. Whether every FreeBSD branch has a G-good commit

**Belief.** FreeBSD's GCC build path breaks often and is fixed within weeks. In October 2026 all three GCC CI jobs were reported failing. `main` failed in `buildkernel` on 31 October, and `stable/14` failed in `buildworld` (04.6, 01). Some of those failures are GCC 14 specific, such as bug 297758, which GCC 15 does not have. With GCC 15 as the reference, we believe each branch has a G-good commit within the last 60 days.

**Risk.** If `main` has no G-good commit for months, the nightly grades a stale pin. After 30 days the branch drops from the headline (04.6). A FreeBSD claim on releases only is still a claim, but a weaker one. Fixing G's failures ourselves would mean patching the tree, which 02.1 forbids.

**How to find out.** B0 runs `rb baseline` with GCC 15 on the heads of `main`, `stable/15` and `stable/14`, then bisects backwards over the last 60 days for each failing branch. The result is a list of dates and causes per branch. A reproducible failure with FreeBSD's own CI configuration is reported upstream by a person (question 16).

**Deadline.** B0.

## 2. How large warning parity is

**Belief.** Both systems build nearly everything with `-Werror` (03.6). A warning rucc gives and GCC does not is a build failure. We believe the first pass is **L**, three to six weeks, because rucc already tracks GCC's warning set for Linux, where `-Werror` is now common, and because FreeBSD demotes about 40 warning classes under GCC.

**Risk.** The two systems have about 12 million lines of userland C that no rucc build has seen, with `WARNS=6` in much of FreeBSD and `WARNS?=5` in much of NetBSD. If rucc's warnings differ on a few common patterns, the count is small. If they differ in the analysis behind `-Wmaybe-uninitialized`, `-Wstringop-overflow` or `-Warray-bounds`, which depend on the optimizer, the count depends on optimization decisions rucc cannot copy, and B2 could double. Each persona also needs its own warning set (04.3), and NetBSD 10's GCC 10.5 is far from rucc's native set.

**How to find out.** `rb syntax` and `rb warnings` (05.7, 06.9) over every C unit of B0's G builds, as soon as rucc accepts the targets at B1. That gives the count per warning class before B2 starts, and B2's estimate is redone from it.

**Deadline.** B2's first week.

## 3. OpenBSD

**Belief.** OpenBSD is outside the claim (02.7, 04.2). On every architecture in use it is built by its own clang, 22.1.6 in 8.0 (released 20 October 2026), with options no GCC has: `-fret-protector`, `-fret-clean`, `-ffixup-gadgets`, `-msave-args`. There is no GCC reference to grade against, and the GCC 4.2.1 architectures are legacy hardware.

**Risk.** Two things would change it:
- OpenBSD's kernel or libc might build with a ports GCC on amd64 with the hardening options simply absent, giving a reference of sorts. That would be a configuration OpenBSD does not ship, so its results would say little.
- rucc might implement retguard and its siblings, as OpenBSD's clang defines them. Then the reference is OpenBSD's clang, which is a different oracle, and the claim becomes "as compatible as OpenBSD's own compiler" rather than "as GCC".

Nothing in this plan depends on the answer. It decides whether a later plan exists.

**How to find out.** At B11:
1. Count the units of OpenBSD's base that pass each OpenBSD-local option, from a native build's command lines.
2. Try a kernel and a libc build with the `lang/gcc` port on amd64, and record what fails and why.
3. Read the clang-local(1) definitions of each option against what rucc would need to emit.

The answer is written into 04.2.

**Answer.** OpenBSD stays outside the claim, and no later plan starts now. Its base has no GCC build on any architecture in use. Retguard, the gadget fix-up pass and `-mretpoline` are defaults of OpenBSD's clang, not options on the command line, and the amd64 assembly adds retguard only when the compiler predefines `_RET_PROTECTOR`. A G build with the `lang/gcc/16` port would need a changed kernel Makefile and would grade a configuration that nobody uses, so step 2 was not run. rucc would need a clang persona, and the oracle would be OpenBSD's clang. 04.2 has the evidence, the files that pass each option, and what a builder would need.

**Deadline.** B11.

## 4. Whether the C++ share leaves a claim worth making

**Belief.** The C++ compiler is the reference `g++` in both R and G (10). NetBSD's `c-only` row (`MKCXX=no`) makes rucc's share of objects very large. FreeBSD has no such switch since January 2023, but the `graded` configuration drops LLVM, and the `c-only` row drops GoogleTest and `pmc`. We believe rucc builds more than 95% of FreeBSD's `c-only` objects outside the C++ libraries.

**Risk.** If the census shows a large C++ share in FreeBSD's graded world, for example through `libc++`, `devd`, `dtc`, `ctld`, `zfsd` and 421 C++ files outside LLVM and kyua (10.2), then "FreeBSD built by rucc" overstates what happened, even with the census beside it.

**How to find out.** B0's `rb census` on G's `graded` and `c-only` builds gives the shares by object count and by lines. The headline in 14.8 states the share for every row, and 00 says which rows carry the claim.

**Deadline.** B2, before the first FreeBSD result is reported.

## 5. Whether G builds and boots the boot code on both systems

**Belief.** FreeBSD's GCC builds compile `stand/` with GCC, including `boot2` with `-mrtd`, `--param min-pagesize=1024` and `BOOT2SIZE=7680`, and the result fits and boots. NetBSD's boot blocks are built with GCC by default. So G's boot code exists and is the reference for R's (08.6).

**Risk.** If FreeBSD's GCC build of `boot2` does not fit or does not boot, G has no BIOS boot path. Then R's `boot2` has no oracle, and the BIOS boot of 12.3 cannot be graded on FreeBSD. The UEFI path through `loader.efi` would be all that is graded.

**How to find out.** B0's FreeBSD baseline boots G through SeaBIOS and `gptboot`. If it fails, the failure is bisected and reported upstream (question 16), and 12.3 marks the FreeBSD BIOS boot ungraded until G has one.

**Deadline.** B0 for the measurement. B6 for the decision.

## 6. Whether the NetBSD `TOOLDIR` overlay is stable

**Belief.** `build.sh -u` uses an existing `TOOLDIR` without rebuilding it, so a copy of G's tools with two shims in place is what the R build runs (05.4).

**Risk.**
- `build.sh` may check the tools' timestamps or hashes in some paths and rebuild `nbgcc`, replacing the shim. A partly rucc build would then pass every check by not using rucc.
- Some part of the build may call the compiler by a path other than `TOOL_CC`, such as a `${TOOLDIR}/libexec/gcc` path or `cc1` directly.

**How to find out.** At B1:
1. `rb toolchain-diff` runs before and after a build and must show the same two files differing.
2. `rb census` must account for every object as built through the shim. An object with no shim record is the sign of a bypass.

**Deadline.** B1.

## 7. Hardware for aarch64

**Belief.** The fleet has no arm64 machine with KVM. aarch64 runs under TCG on server1 and server2, so its full test suite runs weekly and it has no performance gate (11.5, 13.5).

**Risk.** Without hardware, B7 grades aarch64 with a tenth of amd64's coverage, and the code generator's aarch64 performance is never measured on a BSD.

**How to find out.**
- Check whether GitHub's `ubuntu-24.04-arm` runners expose `/dev/kvm`, as `rucc-kernel` 16.8 asks. If they do, nightly aarch64 boots move there.
- Price an arm64 server with a FreeBSD and NetBSD supported board, such as an Ampere machine.

**Deadline.** B7 for the test cadence. B8 for whether aarch64 has a performance gate.

## 8. rucc on a NetBSD host

**Belief.** Rust supports NetBSD amd64 as a host, so rucc builds there. B10 can attempt a NetBSD fixed point: the RR NetBSD image builds itself with rucc.

**Risk.** rucc's dependencies, or its own code, may assume Linux or FreeBSD in places, such as how it finds its own path or reads `/proc`. Then the NetBSD fixed point waits for host fixes. That is reported in B10 but not gated (16.11).

**How to find out.** Build rucc and run its test suite on a NetBSD 11.1 VM at B4, when the first NetBSD RR image exists.

**Deadline.** B10.

## 9. Disk and machine time on gpc

**Belief.** gpc needs about 600 GB of scratch for this folder, on top of what `rucc-kernel` uses (14.7): builder images, tree caches, images kept for failures, baselines. A baseline is about a day of gpc per system, version and row (14.6).

**Risk.** If gpc lacks the disk, fewer failure images are kept and fewer pins are cached. If it lacks the time, the nightly drops rows. `rucc-kernel` and `rucc-bsd` share gpc, and both have nightlies.

**How to find out.** B0 measures one baseline and one nightly for each system, the size of `compile.jsonl`, and the image sizes. The schedule is then written for both harnesses together.

**Deadline.** B0.

## 10. DragonFly's `CCVER`

**Belief.** rucc can be installed as a custom `CCVER` through `compilers.conf`, a configuration file on the builder (05.3). With a name like `rucc120`, the build takes the same configuration as `gcc120`.

**Risk.** DragonFly's `bsd.sys.mk` keys its GCC warning sets on `${_WCCVER:Mgcc*}`. A `CCVER` that does not start with `gcc` takes the non-GCC path, which configures the build differently from G. A name starting with `gcc` that runs rucc would get the right configuration, but would be a lie in a name, and the shim rules of 14.4 say the name should not decide the type.

**How to find out.** At B11, run `rb mk-diff` between `CCVER=gcc120` and `CCVER=rucc120` with both pointing at the reference GCC. Any difference in the compiler-dependent variables is the answer. If there is one, the fallback is a `CCVER` named for the GCC version it imitates, with the shim recording which compiler ran, and that choice written down in 05.3.

**Answer.** The name alone does not give the configuration of G, so the build uses the fallback, with three make variables. Run 43220187148 of `dragonfly-r` compared `CCVER=gcc80` and `CCVER=rucc80` on 6.4.2 with `scripts/dragonfly/mkdiff.sh`. With `rucc80`, `bin/sh` lost `-Werror` and five of its `-Wno-` options, and the ext2fs module lost `-Werror`, `-mpreferred-stack-boundary=4`, `-mno-ssse3` and the other `-mno-` options, and `-mindirect-branch=thunk-inline`. rucc is now the `CCVER` `gcc80rucc`, and `gcc120rucc` on master. The name starts with the name of the GCC that rucc imitates, so all the `Mgcc*` and `Mgcc8*` patterns of `share/mk`, `sys/conf`, `sys/platform/pc64/conf/kern.mk` and `lib/libzstd` match. Four tests compare `CCVER` to `gcc80` exactly: `bsd.cpu.mk`, the two `-Werror` tests of `bsd.sys.mk`, and `WERROR` in `kern.pre.mk` and `kmod.mk`. With the name only, run 43226747023 showed that `MACHINE_CPU` and `-Werror` were the only differences. The harness gives these four tests the values of the reference with three variables on the make command line, `CCVER_BSD_CPU_MK=bsd.cpu.gcc80.mk`, `_WCCVER=gcc80` and `WERROR=-Werror`, and make passes them to each make below it. With them, the make variables at lib/libc, lib/libzstd, bin/sh, sbin/devd and sys/vfs/ext2fs are the same as with `gcc80` (run 43226747023), and the R job stops if they are not. The tree is not changed, and `compilers.conf` is the only file on the builder that names rucc. The name and `rucc.txt` of the build record say which compiler ran. 05.3 has the details.

**Deadline.** B11.

## 11. The museum

**Belief.** FreeBSD 2 to 4 and NetBSD 1.x, built with GCC 2, are outside this goal (02.4). `rucc-kernel` K11 builds the machinery for old GNU C (persona 2.x, era containers), and a BSD museum would reuse it.

**Risk.** None for this plan. The question is whether the project wants the BSD museum at all, since nobody has a test suite for those trees and they need a.out on the oldest systems.

**How to find out.** After B12 and after `rucc-kernel` K11, estimate the work per tree from the census of one old tree, such as FreeBSD 4.11 and NetBSD 1.6.

**Deadline.** None.

## 12. Whether GCC 15.2's `-mbranch-protection=standard` sets GCS

**Belief.** clang 21 with `standard` sets the BTI, PAC and GCS bits in `.note.gnu.property`. rucc sets BTI and PAC (probed, 11.2). We believe GCC 15 also added GCS to `standard`, which would make rucc's note differ from G's. This is not yet checked.

**Risk.** The linker ANDs the bits. If G sets GCS and R does not, R's binaries lose GCS silently. If neither does, there is nothing to do.

**How to find out.** On the FreeBSD builder, compile an empty function with `aarch64-gcc15 -mbranch-protection=standard` and read the note with `readelf -n`. The same with NetBSD's in-tree GCC 14.4 and 12.5. Then `rb sections-diff` on every aarch64 object (16.8).

**Deadline.** B7.

## 13. Whether the directive reader stays

**Belief.** The rule of 07.3 makes the reader correct by sending anything it cannot reproduce exactly to the assembler path. Whether the reader is worth keeping is unmeasured.

**Risk.**
- **Keeping it** keeps a second implementation of part of GNU as, which is where bugs A, B and C came from.
- **Removing it** sends every file with an `__RCSID` or `__FBSDID` block, which is most of both systems, down the assembler path. If that path costs a few percent per file, it eats into the compile time gate of B8.

**How to find out.** B1 builds NetBSD's userland with the reader on and with `-frucc-asm-reader=off`, from the shim's records. If the reader saves less than 1% of build time, it goes.

**Answer.** The reader goes on ELF. Run 42644075064 of `reader-cost` measured it over 55896 units of NetBSD's userland. It saved -0.01% of the compile time, so removing it costs no compile time. The file-scope blocks cost 2.52% on the assembler path, and the reader got none of it back. 07.3 records the numbers.

**Deadline.** B1.

## 14. Whether a NetBSD riscv64 baseline grades anything

**Belief.** NetBSD's riscv port is new in 11.0. No ATF run on QEMU is published. Our G baseline is the first, and it will have many `g-unstable` cases (12.5). We believe enough cases pass G three times for the row to mean something.

**Risk.** Under TCG with `-smp 4`, timing-sensitive tests flake. If G's three runs agree on too few cases, the row grades little, and NetBSD riscv64 becomes a "builds and boots" row like DragonFly.

**How to find out.** Run the riscv64 G baseline as soon as B4 has the NetBSD rig, not at B7, so the number is known early. The row's claim is fixed by the share of cases G passes three times.

**Deadline.** B7.

## 15. A GCC import into NetBSD trunk

**Belief.** NetBSD trunk is on GCC 14.4.0 (04.3). A newer GCC will be imported at some point, perhaps before NetBSD 12.0. `rb sets` notices the `BASE-VER` change and opens a persona review (04.7).

**Risk.** An import changes `HAVE_GCC`, the warning flags NetBSD adds or removes for that version, and the warnings on by default. For a few days or weeks, trunk's G may itself fail. If it lands in the middle of B4, the level A result has to be redone on the new persona.

**How to find out.** Follow NetBSD's `tech-toolchain` list and the `external/gpl3/gcc` tree. When the import happens, `rb mk-diff` between the old and new trees lists the configuration changes.

**Deadline.** Continuous. If it lands during B4, B4's exit is taken on the `netbsd-11` branch first and on trunk once the persona review is done.

## 16. Reporting results and bugs upstream

**Belief.** This folder reports G failures upstream only through the projects' own channels, by a person (04.6). It files rucc bugs in `tamnd/rucc`. It does not publish results about FreeBSD or NetBSD in their forums.

**Risk.** Public results claiming "FreeBSD built by rucc" without the census and the C++ line would mislead. Bug reports to FreeBSD or NetBSD about GCC build failures are welcome as a rule, but a flood of them from one harness is not.

**How to find out.** Before level A is announced anywhere, decide with the project owner:
- what is published, and where;
- who files upstream reports, and how many at a time;
- whether to contact NetBSD's toolchain maintainers about the rig, since it builds their tree in a way they may find useful.

**Deadline.** B4.
