# rucc-bsd

A harness that builds pinned, unmodified FreeBSD and NetBSD trees with [rucc](https://github.com/tamnd/rucc) and with a reference GCC. It boots both builds under QEMU, runs the system's own ATF tests against them, and records the results in committed files.

The goal is to build every supported FreeBSD and NetBSD release with rucc. Each build must boot, pass every test that the GCC build passes, and run as fast as the GCC build. The plan is in [docs/plan](docs/plan). It has thirteen milestones, B0 to B12. Each milestone has a tracking issue and a GitHub milestone with the same name. We tick the checklist in the issue when a pull request lands.

## The rules

**No compiler code.** This repository does not compile C. Every compile goes to a real compiler, rucc or the reference GCC, through the `rb-cc` shim. The shim records the call and does not change it. A fix to rucc goes to [tamnd/rucc](https://github.com/tamnd/rucc).

**No source patches and no copied source files.** We fetch each tree by commit hash or as a signed release distribution. The harness does not write into a tree. It writes only to the object directory of the build.

**No reductions.** A reduced test case goes to [rucc-corpus](https://github.com/tamnd/rucc-corpus), or to [rucc-compat](https://github.com/tamnd/rucc-compat) when it compares two compilers. This repository links to it by issue number.

**No hidden configuration.** Every input that changes a graded build is in a committed file: pins, personas, rows, `src.conf` and `mk.conf` knobs, and the list of explained differences.

## R and G

R is a build of a tree by rucc. G is a build of the same tree, with the same configuration, by the reference GCC. For NetBSD, the reference is the GCC that `build.sh tools` builds from the tree. For FreeBSD, the reference is the `amd64-gcc15` package. A test case counts only if G passes it in all three of its baseline runs.

## Milestones

| | Scope |
|---|---|
| B0 | the instrument: `rucc-harness`, pins, G baselines, census and demands |
| B1 | rucc speaks BSD: targets, file-scope `asm`, identity, feature-test macros, driver flags |
| B2 | configured the same: probes, `mk-diff`, `flags-diff`, `syntax`, warning parity |
| B3 | the NetBSD amd64 kernel boots and passes ATF under the G world |
| B4 | the NetBSD amd64 world (level A) |
| B5 | the FreeBSD amd64 kernel |
| B6 | the FreeBSD amd64 world (level B) |
| B7 | aarch64 and riscv64 on both systems |
| B8 | the performance gates |
| B9 | every version and branch |
| B10 | rucc as the system compiler |
| B11 | DragonFly and the OpenBSD decision |
| B12 | host tools, i386 and steady state |

## License

Apache License 2.0. See [LICENSE](LICENSE).
