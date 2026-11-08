# Boot and tests

Compiling is half the claim. The other half is that the system runs. This document says how a build becomes a running system, what is run on it, how results are compared with the reference, and how a failure is traced to rucc or cleared.

## 12.1 The oracle

The oracle is the reference build of the same tree, run the same way on the same machine. It is not a list of expected results written by us or published by the project.

- **G is run three times** per baseline (`rb baseline`). A test case counts only if it passed all three times. One that fails, skips or times out in any G run is recorded as `g-unstable` and never graded.
- **The project's published runs are cross-checks, not the oracle.** NetBSD publishes ATF results, for example 10,870 passed and 12 failed on bare metal on 4 October 2026 (01), and FreeBSD publishes its CI results. A G baseline far from them means our rig differs from theirs, which is a rig bug to fix before grading.
- **Kernel logs are part of the oracle.** A message in R's console that G's three runs never printed is a finding even if every test passed (02.3, part 5).

## 12.2 What runs

| Suite | Contents | Where it comes from |
|---|---|---|
| boot | the kernel reaches multi-user, `rb-rc` reports in, a clean shutdown completes | 12.4 |
| smoke | the list of 02.3 part 3: shells, network on `vtnet0` or `vioif0`, `fsck` and `mount` of a second partition, `pkg` or `pkg_add` of one package from a local repository, one module loaded and unloaded, `/usr/tests` present | `rucc-bsd/smoke/`, the same script for R and G |
| tests | FreeBSD: `kyua test -k /usr/tests/Kyuafile`. NetBSD: `atf-run` and `atf-report` over `/usr/tests`, as `anita` runs them | the system's own suites |
| dtrace | `dtrace -l`, one `fbt` and one `syscall` probe, and on FreeBSD the DTrace test suite | 09.6 |
| cross-modules | 09.3 | |
| foreign | 12.8 | |

**`RB-TEST`.** The kernel that runs the tests may need options GENERIC does not have:
- a serial console at a fixed speed;
- test modules built in;
- `ddb` with `KDB_UNATTENDED` so a panic dumps a backtrace and reboots instead of waiting for a person.

`RB-TEST` is a kernel configuration file in `rucc-bsd`:
- **FreeBSD:** `include GENERIC` plus options;
- **NetBSD:** `include "arch/amd64/conf/GENERIC"` plus options.

It may only add options, never remove them. G uses the same file. Every option carries a comment saying which test or which part of the rig needs it, as `src.conf` knobs do (02.1).

**Test settings.**
- **The machine shape follows FreeBSD's CI:** 2 vCPUs and 8 GB, for both systems.
- **Parallelism:** `kyua`'s `parallelism` is 2, written in the image's `kyua.conf` for R and G alike.
- **Timeouts:** each test's own timeout, unchanged.
- **A whole-run limit** of 4 hours on amd64 under KVM, after which the run is killed and every unfinished case counts as `timeout` for that side.

## 12.3 Images and boot: `rb image`, `rb boot`

**Making the image.** `rb image` turns a staging tree from `installworld`/`installkernel`/`distribution` (FreeBSD) or from the release sets (NetBSD) into a raw disk image:
- **FreeBSD:** `makefs -t ffs` for the root and `mkimg -s gpt` with `efi`, `freebsd-boot` (from `gptboot`), `freebsd-ufs` and `freebsd-swap` partitions, the same tools as `release/Makefile.vm` and `tools/boot/rootgen.sh` use.
  - On `main`, the `vm-image` target (`release/Makefile.vm:188`) depends on a pkgbase repository, which would add `make packages` to every nightly. `rb image` uses the tools directly, and the image is equivalent.
  - A weekly job builds the official `vm-image` from G as a check that the two are equivalent.
- **NetBSD:** `build.sh live-image` (`build.sh:1148`), which is the project's own path.

Both images include:
- the boot code the build made (so in R, rucc's boot code, 08.6);
- the `rb-rc` script;
- the smoke suite;
- a local package repository with the handful of packages the smoke suite needs. These are built once per pin by G and are the same for R and G.

**Booting.**
- **amd64 boots twice per run:** once with SeaBIOS through the BIOS boot blocks, once with OVMF through the EFI loader. The tests run on the UEFI boot. The BIOS boot runs only the boot suite, because its point is the boot code.
- aarch64 uses QEMU `virt` with EDK2, as FreeBSD's `tools/boot/ci-qemu-test.sh:92` does.
- riscv64 uses QEMU `virt` with OpenSBI and U-Boot or EDK2, whichever the system's own documentation names (11.5).

`rb boot --image IMG --suite S`:
1. starts QEMU with KVM where possible, the serial console on a log file, and a second, empty virtio disk;
2. waits for `rb-rc` markers on the console with per-stage timeouts;
3. kills the VM if a stage times out;
4. reads results back from the second disk.

## 12.4 `rb-rc`

A small `/etc/rc.d/rb` script (both systems use `rc.d`), enabled in `rc.conf`, run last at boot:
1. prints `RB-READY <uname -a>` on the console;
2. checks `dmesg` and writes it to the results disk;
3. runs the suites named in `/etc/rb.conf`, which `rb image` writes, and saves the raw outputs (`kyua report-junit`, or `atf-report`'s XML output) to the results disk as a `tar` archive with no file system, so a broken file system driver in R cannot hide results;
4. prints `RB-DONE <counts>` and powers off.

**No network** is used to report, because the network stack is under test. The console markers carry only what is needed to know the run finished. The results disk carries the rest.

**A panic** prints a backtrace through `ddb` to the console. `rb boot` saves it, and the run is marked `panic` with the test that was running, which `kyua` and `atf-run` log before starting each case.

## 12.5 Baselines

`rb baseline V --row R --conf C` builds G, images it, and runs every suite three times on fresh images. It writes to `baselines/`:
- **per test case:** the three results and their durations;
- **per suite:** counts;
- **the kernel log lines** of all three runs, as the allowed set for 12.1;
- **the G image hashes,** so a later rebuild of G can be checked to be byte-identical. Both systems' builds are reproducible with `MKREPRO` and FreeBSD's `WITH_REPRODUCIBLE_BUILD`. Where a rebuild is not identical, the baseline is redone.

**How baselines move.**
- Baselines are redone when the pin moves (04.6), the reference toolchain moves, or the rig changes.
- They are kept forever, so a result can always be traced to the oracle it was graded against.

**Known gaps.** NetBSD's riscv port has no published ATF run on QEMU. Our baseline is the first, and it will contain many `g-unstable` cases. Those cases are reported upstream through the project's own channels only by a person (04.6).

## 12.6 The 2x2: `rb test`

Every graded run produces four systems from two builds:

| | G world | R world |
|---|---|---|
| **G kernel** | GG, the baseline | GR: rucc's userland on the reference kernel |
| **R kernel** | RG: rucc's kernel under the reference userland | RR: the claim |

The kernel and its modules go together. The world is everything else, including the boot loader. The boot code is graded by the BIOS and UEFI boots of RR and by `bsd-asm` (07.4).

**Attribution of a case that GG passes and RR fails:**

| RG | GR | Verdict |
|---|---|---|
| fail | pass | kernel |
| pass | fail | world |
| fail | fail | both, or something in the kernel's interface to the world; filed once, investigated from the kernel side first |
| pass | pass | interaction: only the combination fails. Investigated with the `ktrace` or `ktruss` outputs of RR and GG |

**Cost.** The 2x2 costs two more boots and test runs per pin. They run only for the cases RR failed, not the whole suite, which is cheap.

**Swapping kernels** is a file copy, because R and G come from the same commit and configuration: `/boot/kernel` with its modules on FreeBSD, and `/netbsd` with `/stand/<arch>/<version>/modules` on NetBSD. The version strings differ only in the build path and the compiler, which nothing checks.

## 12.7 `rb mixed`: finding the object

Once the 2x2 says "world" or "kernel", `rb mixed --unit U` narrows it to an object:
1. Take the linked unit U that the failing test exercises: a library, a program or the kernel. The test's process trace says which.
2. R and G compiled U's objects with identical command lines (`rb flags-diff`, 05.6). So any subset of R's objects can be replaced with G's, and the result relinked with the same link command.
3. Bisect over the object list. Relink U with half R and half G objects, install it in an RR image, and rerun the one failing case. Repeat until one object remains.
4. Report the object, its command line and its preprocessed source, as `rb repro` prints them.

**Below one object** is the rucc developers' usual work: reduce the preprocessed unit with `cvise` against the failing behaviour, then add the reduced test to `rucc-corpus`. That reduced test is the end product of a failure, and the first thing checked when it is fixed.

**For the kernel**, step 3 relinks the kernel with mixed objects. FreeBSD's `kern.post.mk` link and NetBSD's `Makefile.kern.inc` link take an object list, so this needs no patch. It needs only a link step run outside `make`, from the command `rb-cc` recorded.

**When mixing is impossible.** A struct layout difference makes every mix of R and G objects using that struct wrong, so bisection points at whichever object the layout first meets. `rb mixed` therefore refuses to run until `bsd-abi` and `rb abi-diff` are clean for the pin.

## 12.8 `rb foreign`: the project's own binaries

A rucc system must run what users run, and what users run was built by the project, not by us:
- **FreeBSD's official release `base.txz`** and packages from `pkg.freebsd.org` for that release. Both are built by **clang**.
- **NetBSD's official release sets** and `pkgsrc` binary packages for that release, built by GCC.

`rb foreign` checks three things:
1. **Official userland on rucc's kernel.** The official world runs on R's kernel. This is a third cell beside the 2x2: the kernel's system call ABI against a world nobody here built. The official release's own kernel is not used, because it is not from the pinned commit. The pin must be the release tag for this to mean anything, so `rb foreign` runs on release pins only.
2. **Official packages on rucc's world.** A fixed list of about 30 packages is installed from the project's repository into RR and run with a scripted check each: `bash`, `perl`, `python`, `git`, `curl`, `sqlite3`, `tmux`, `vim`, `nginx`, `postgresql`, a Rust program, a Go program and a C++ program among them. Their libraries link against rucc's `libc`, `libm`, `libthr` or `libpthread`, `libutil` and `rtld`. This is the strongest ABI test available: binaries from another compiler (clang, for FreeBSD), built years of releases apart, against rucc's shared libraries.
3. **Compatibility libraries.** FreeBSD's `compat` packages (`misc/compat14x` on 15.x) run an older release's binaries on RR. This exercises the `__sym_compat` symbols (08.5).

**Grading.** A foreign failure counts as a failure of the pin if GG passes the same check. Packages update, so the package set is pinned by the repository's catalogue date and kept per release.
