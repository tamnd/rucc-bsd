# Systems, versions and references

This document says which trees are built, which GCC grades each one, what persona rucc wears for it, and where the build runs. Facts about releases and compilers are from 01. Here they become decisions.

## 04.1 The four systems as compilers see them

| | FreeBSD | NetBSD | DragonFly | OpenBSD |
|---|---|---|---|---|
| Base compiler today | clang/LLVM 21.1.8 on every branch tip; 19.1.7 in 15.1-RELEASE and 14.4 | GCC 14.4.0 on trunk, 12.5.0 on netbsd-11, 10.5.0 on netbsd-10 | GCC 12.5.0 on master (since 21 October 2026), GCC 8.3 in 6.4.2 | clang 22.1.6 with local patches; GCC 4.2.1 on alpha, hppa, m88k, sh |
| GCC build path | `CROSS_TOOLCHAIN=amd64-gcc14` or `-gcc15`, from the `devel/freebsd-gccN` ports; CI jobs for GCC 14 | the default | the default; `CCVER` selects any compiler | none on clang architectures |
| Linker for a GCC build | GNU ld (BFD) from `devel/binutils` | GNU ld 2.46 on trunk, 2.42 on netbsd-11 | GNU ld 2.34 | lld, or BFD 2.17 on GCC architectures |
| How the build identifies the compiler | parses `cc --version` (`bsd.compiler.mk`) | make variables only: `HAVE_GCC=14` | the `CCVER` name: `gcc120` | the architecture table |
| C++ in base | mandatory, no switch since January 2023 | optional, `MKCXX=no` | GCC and libstdc++ only | mandatory |
| Cross build from Linux | `tools/build/make.py` | `build.sh`, by design | no | no |
| Test suite | kyua and ATF, about 9,100 cases written in C and shell | ATF, about 11,600 cases; a public bare-metal run on 4 October 2026 had 10,870 passed and 12 failed | none structured | `regress/`, native |
| C standard in base | `gnu17` | `gnu11` | `gnu11` | the compiler's default |

The rows that decide this folder:
- **NetBSD is the system whose own compiler is GCC.** It builds from Linux by design and can drop C++ with a supported switch. That makes it the first target (00).
- **FreeBSD's own compiler is clang.** GCC is a maintained second path, with CI jobs, packages and a FreeBSD-local format patch. That path is where rucc enters.

## 04.2 Why OpenBSD and DragonFly wait

**OpenBSD.** On amd64 and arm64 there is no reference GCC build to compare with. The kernel and libc are built with options that exist only in OpenBSD's clang:
- `-fret-protector`, which is retguard;
- `-fret-clean` and `-ffixup-gadgets`;
- `-msave-args`;
- `-mretpoline` on by default.

The build is native only. A rucc OpenBSD would need rucc to implement those options as OpenBSD's clang defines them, and to be graded against OpenBSD's clang rather than any GCC. That is a different project with a different oracle. The GCC 4.2.1 architectures are graded nowhere because nobody runs them under emulation at scale. Open question 3 says what would change this.

**DragonFly.** It is the cheapest system after NetBSD, with GCC 12.5, GNU ld, x86-64 only and a native plug-in for a custom compiler. It waits only because it has no test suite to grade against, so its claim is weaker (02.7), and because it needs no rucc work that FreeBSD and NetBSD do not already force. B11 adds it at about a week of harness work.

## 04.3 References and personas

Every pin has a reference toolchain and a persona, written in `personas.toml` and `toolchains.toml` and printed by `rb personas`. The persona is rucc's `-fgnuc-version=` flag. It must make rucc answer every identity question the way the reference does, apart from naming itself (05.3).

| System | Pin | Reference GCC | Reference binutils | rucc persona |
|---|---|---|---|---|
| FreeBSD | `main`, `stable/15`, `releng/15.x` | `amd64-gcc15` package, GCC 15.2.0 with the port's patches | `binutils` package, BFD ld | 15.2.0 |
| FreeBSD | `stable/14`, `releng/14.x` | `amd64-gcc15`, falling back to `amd64-gcc14` (14.2.0) where 15 fails and 14 does not | same | the reference's version |
| NetBSD | `trunk` | the in-tree GCC 14.4.0, as `build.sh tools` builds it | in-tree binutils 2.46 | 14.4.0 |
| NetBSD | `netbsd-11`, 11.x | in-tree GCC 12.5.0 | in-tree 2.42 | 12.5.0 |
| NetBSD | `netbsd-10`, 10.x | in-tree GCC 10.5.0 | in-tree | 10.5.0 |
| DragonFly (B11) | master | in-tree GCC 12.5.0 | in-tree 2.34 | 12.5.0 |

Why these and not others:
- **FreeBSD and GCC 15.** FreeBSD's CI uses GCC 14, and `make universe` defaults to `gcc15`. GCC 14 rejects a C++ flexible array member in `usr.sbin/pmc/headers.hh` that GCC 15 and clang 21 accept (FreeBSD bug 297758, August 2026). That is a `g++` problem and does not involve rucc, but `g++` runs in both R and G. A reference that cannot build the C++ part makes no G at all. GCC 15 is also the version nearest to rucc's own dialect.
- **Why not GCC 16 for FreeBSD.** There is no `freebsd-gcc16` port as of 8 November 2026. Building GCC 16 ourselves with the port's patches would make a reference nobody else uses. When the port appears, it becomes a second reference, `G16`, graded weekly, and the persona for that row becomes 16.
- **NetBSD's own GCC.** Using `build.sh`'s in-tree GCC means G is exactly what NetBSD ships. Its version is set by the tree, not by us.

**What a persona does not change.** rucc's language is GCC 16's under every persona, because each tree passes its own `-std` (`gnu17` for FreeBSD, `gnu11` for NetBSD). A persona changes:
- `__GNUC__`, `__GNUC_MINOR__` and `__GNUC_PATCHLEVEL__`;
- `-dumpversion` and `-dumpfullversion`;
- the banner;
- one more thing new to this folder: **which warnings are on by default and in `-Wall` and `-Wextra`**. A warning GCC added in 15 and enabled in `-Wall` must not appear under a persona of 14.4 (06.9). Under `-Werror`, a newer warning set is a build failure that G would not have.

## 04.4 Where builds run

| System | Build host | Reference toolchain comes from | rucc comes from |
|---|---|---|---|
| FreeBSD | a FreeBSD 15.1-RELEASE builder VM on gpc, the same release as FreeBSD's own CI build jail | `pkg install amd64-gcc15 binutils` at pinned package hashes | built on the builder from the commit under test (03.11) |
| NetBSD | a Debian 13 container on gpc or server3 | `build.sh tools` from the tree itself | the Linux release build of the commit under test |

**Why FreeBSD builds natively.**
- **The reference is a FreeBSD package.** `devel/freebsd-gcc15` installs a cross GCC that runs on FreeBSD. On Linux we would have to build GCC 15.2 with the port's patches applied ourselves, which is a reference nobody else uses.
- **FreeBSD's own CI does the same.** It builds in a 15.1 jail.

`tools/build/make.py` from Linux is still run, recorded and reported as an informational row. It is FreeBSD's documented way to cross-build from Linux, and a difference between the two build hosts is worth knowing. It is never graded.

**Why NetBSD builds on Linux.** `build.sh` was designed for exactly this, and it builds G's compiler from the tree. The Linux host keeps NetBSD builds off the FreeBSD builder VMs, which are the scarcer resource.

**What the host affects.** Host tools, such as `nbmake`, `nbmakefs`, FreeBSD's bootstrap `bmake`, and `config(8)` on the builder, are built by the host's own compiler in both R and G. They are not graded until B12, when they are built by rucc too (16.13).

## 04.5 Sets and pins

`sets.toml` states rules, and `rb sets --write` expands them into `pins.toml` from what the projects publish:

```toml
[freebsd.current]
branches = ["main", "stable/15", "stable/14"]
releng_in_progress = true           # releng/X.Y while BETA or RC
nightly = true

[freebsd.releases]
supported_only = true               # from the security page's table
weekly = true

[netbsd.current]
branches = ["trunk", "netbsd-11", "netbsd-10"]
nightly = true

[netbsd.releases]
supported_only = true
weekly = true

[pin_policy]
head = "last-g-good"                # see 04.6
```

The pins as of this writing:

| Pin | Source | Date |
|---|---|---|
| FreeBSD `main` | `665afd2a9` | 7 November 2026 |
| FreeBSD 15.1-RELEASE | `releng/15.1` tag | 16 June 2026 |
| FreeBSD 14.5-RELEASE | `releng/14.5` tag | 8 September 2026 |
| FreeBSD `releng/15.2` | BETA1 | 6 November 2026, RC1 due 27 November, release due 8 December |
| NetBSD `trunk` (11.99.10) | `36eddebcc` on the git mirror | 4 November 2026 |
| NetBSD 11.1 | release | 2 November 2026 |
| NetBSD 10.2 | release | 15 September 2026 |

`rb sets` runs daily. A new tag becomes a pin the same day. A branch the project drops from its support table becomes an unpinned history entry with its last results.

## 04.6 A head with no reference

A pin is graded only when G exists for it. For a release, G is built once and almost never moves. For a branch head, G can break:
- all three FreeBSD GCC CI jobs were reported failing in October 2026;
- `main` failed in `buildkernel` on 31 October;
- `stable/14` failed in `buildworld`.

Rucc cannot be graded on a head the reference cannot build. So:
1. `head = "last-g-good"` pins each branch to the newest commit at which G built and booted in the last baseline, not the newest commit.
2. The nightly tries the real head with G first. If G succeeds, the pin moves. If G fails, the pin stays, and the report says how many days the branch has been behind its head.
3. A G failure is reported upstream through the project's own channels when it is reproducible with the project's own CI configuration. For FreeBSD that is the `gcc14` jobs, and the `gcc15` equivalent is something we run. That is the only outward-facing step the harness takes, and it is done by a person, not by `rb`.
4. A branch more than 30 days behind is dropped from the headline and listed as "no reference".

B0's first measurement is whether the FreeBSD heads have a G-good commit at all within the last 60 days (open question 1).

## 04.7 The calendar ahead

| Date | Event | Effect |
|---|---|---|
| 8 December 2026 | FreeBSD 15.2-RELEASE | joins the releases set. It is expected to ship clang 21; G is unaffected |
| 31 December 2026 | FreeBSD 14.4 end of life | leaves the releases set |
| March 2027 | FreeBSD 14.6 | joins |
| about May 2027 | OpenBSD 8.1 | none, OpenBSD is outside the claim |
| June 2027 | FreeBSD 15.3 | joins |
| unannounced | NetBSD 12.0 | joins the day it is tagged. If its in-tree GCC is 16, the persona becomes 16 and rucc's native dialect is the reference |
| December 2027 | FreeBSD 16.0 | joins. `stable/16` becomes a current branch |

A GCC import into NetBSD trunk changes `HAVE_GCC`, and with it a page of warning flags. `rb sets` notices the `BASE-VER` change and opens a persona review before the nightly runs with the new tree.
