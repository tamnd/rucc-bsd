//! The command line, parsed by hand as `rk` and `rpg` do it.
//!
//! A command takes at most one positional word, such as a pin name, and then options. Every option
//! takes a value, given as the next word or after `=`, except the switches. A value may start with
//! a dash. Each command lists the options it accepts, so that a misspelt option fails before
//! anything runs.

use std::collections::BTreeMap;

/// A parsed command line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Args {
    /// The subcommand.
    pub command: String,
    /// The positional word, which is a pin name where a command takes one.
    pub target: Option<String>,
    /// Options with values.
    pub values: BTreeMap<String, String>,
    /// Switches that were given.
    pub switches: Vec<String>,
}

/// What one command accepts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Accepts {
    /// Whether the command takes a positional word.
    pub positional: bool,
    /// The options that take a value.
    pub values: &'static [&'static str],
    /// The options that take no value.
    pub switches: &'static [&'static str],
}

impl Accepts {
    /// A command that takes nothing.
    pub const NOTHING: Self = Self {
        positional: false,
        values: &[],
        switches: &[],
    };
}

/// What each command of `rb` accepts, or `None` for a command `rb` does not have.
#[must_use]
pub fn accepted(command: &str) -> Option<Accepts> {
    Some(match command {
        "help" | "version" => Accepts::NOTHING,
        "abi-diff" => Accepts {
            positional: false,
            values: &["g", "r", "jobs", "limit"],
            switches: &[],
        },
        "bench" => return Some(BENCH),
        "baseline" | "boot" | "cross-modules" => return boot_commands(command),
        "census" => Accepts {
            positional: true,
            values: &["objdir", "skip", "objects", "limit"],
            switches: &[],
        },
        "demands" => Accepts {
            positional: true,
            values: &["pin", "row", "conf", "jobs", "limit"],
            switches: &["write", "flags-only"],
        },
        "fetch" => Accepts {
            positional: true,
            values: &["set"],
            switches: &["tarball"],
        },
        "foreign" => Accepts {
            positional: false,
            values: &["conf", "out", "r", "g", "limit"],
            switches: &[],
        },
        "image" => Accepts {
            positional: true,
            values: &[
                "build", "out", "system", "suites", "packages", "size", "tools", "work",
            ],
            switches: &["dry-run"],
        },
        "personas" => Accepts {
            positional: false,
            values: &["arch"],
            switches: &[],
        },
        "sections-diff" => Accepts {
            positional: false,
            values: &["g", "r", "g-root", "r-root", "allow", "jobs", "limit"],
            switches: &[],
        },
        "symbols-diff" | "vec-audit" => Accepts {
            positional: false,
            values: &["g", "r", "g-root", "r-root", "jobs", "limit"],
            switches: &[],
        },
        "sets" => Accepts {
            positional: false,
            values: &["as-of"],
            switches: &["write"],
        },
        "warnings" => Accepts {
            positional: false,
            values: &["g", "r", "g-root", "r-root", "limit"],
            switches: &[],
        },
        _ => return toolchain_commands(command),
    })
}

/// What the commands that boot images accept.
/// What `rb bench` accepts: its own options and the boot options it passes on.
const BENCH: Accepts = Accepts {
    positional: true,
    values: &[
        "g-image",
        "r-image",
        "g-bench",
        "r-bench",
        "g-from",
        "r-from",
        "runs",
        "dir",
        "work",
        "system",
        "row",
        "firmware",
        "bench-scale",
        "bench-only",
        "boot-timeout",
        "timeout",
        "disk-size",
        "cpus",
        "memory",
        "qemu",
        "ovmf-code",
        "ovmf-vars",
    ],
    switches: &["keep", "dry-run"],
};

fn boot_commands(command: &str) -> Option<Accepts> {
    Some(match command {
        "baseline" => Accepts {
            positional: true,
            values: &[
                "row",
                "conf",
                "image",
                "from",
                "runs",
                "dir",
                "work",
                "system",
                "suite",
                "firmware",
                "tests-only",
                "tests-except",
                "boot-timeout",
                "timeout",
                "reboots",
                "spare-disks",
                "disk-size",
                "cpus",
                "memory",
                "qemu",
                "ovmf-code",
                "ovmf-vars",
                "edk2-code",
                "edk2-vars",
                "u-boot",
            ],
            switches: &["dry-run"],
        },
        "boot" => Accepts {
            positional: false,
            values: &[
                "image",
                "out",
                "suite",
                "row",
                "firmware",
                "tests-only",
                "tests-except",
                "tests-repeat",
                "modules",
                "modules-only",
                "modules-skip",
                "bench",
                "bench-scale",
                "bench-only",
                "pkgs",
                "boot-timeout",
                "timeout",
                "reboots",
                "spare-disks",
                "disk-size",
                "cpus",
                "memory",
                "qemu",
                "ovmf-code",
                "ovmf-vars",
                "edk2-code",
                "edk2-vars",
                "u-boot",
            ],
            switches: &["in-place", "keep", "login", "scripts", "dry-run"],
        },
        "cross-modules" => Accepts {
            positional: false,
            values: &[
                "image",
                "out",
                "attempts",
                "gg",
                "rg",
                "gr",
                "rr",
                "limit",
                "row",
                "firmware",
                "modules",
                "modules-only",
                "modules-skip",
                "boot-timeout",
                "timeout",
                "reboots",
                "disk-size",
                "cpus",
                "memory",
                "qemu",
                "ovmf-code",
                "ovmf-vars",
            ],
            switches: &["keep", "login", "scripts"],
        },
        _ => return None,
    })
}

/// What the commands that set up compilers accept.
fn toolchain_commands(command: &str) -> Option<Accepts> {
    Some(match command {
        "shim" => Accepts {
            positional: true,
            values: &[
                "row",
                "dir",
                "cc",
                "rucc",
                "log",
                "tools",
                "destdir",
                "shim",
                "bringup",
                "bringup-units",
            ],
            switches: &[],
        },
        "toolchain" => Accepts {
            positional: true,
            values: &[
                "row",
                "dir",
                "cc",
                "rucc",
                "log",
                "tools",
                "destdir",
                "shim",
                "bringup",
                "bringup-units",
            ],
            switches: &["ungraded", "fallback"],
        },
        "build" => Accepts {
            positional: true,
            values: &[
                "row",
                "cc",
                "root",
                "src",
                "targets",
                "jobs",
                "conf",
                "kernconf",
                "rucc",
                "log",
                "shim",
                "noise",
                "bringup",
                "bringup-units",
            ],
            switches: &["no-shim", "ungraded", "dry-run"],
        },
        "flags-diff" => Accepts {
            positional: true,
            values: &["row", "g", "r", "g-root", "r-root", "divergences", "limit"],
            switches: &["paired-only"],
        },
        "mk-diff" => Accepts {
            positional: true,
            values: &[
                "row",
                "dir",
                "g",
                "r",
                "src",
                "conf",
                "kernel",
                "work",
                "divergences",
            ],
            switches: &[],
        },
        "probes" => Accepts {
            positional: true,
            values: &[
                "row",
                "dir",
                "g",
                "r",
                "from",
                "work",
                "details",
                "divergences",
            ],
            switches: &["all"],
        },
        "syntax" => Accepts {
            positional: true,
            values: &["row", "log", "rucc", "sysroot", "jobs", "limit", "out"],
            switches: &[],
        },
        "toolchain-diff" => Accepts {
            positional: true,
            values: &["row", "dir", "g", "r"],
            switches: &["ungraded"],
        },
        _ => return graded_commands(command),
    })
}

/// What the commands that grade R against G accept.
fn graded_commands(command: &str) -> Option<Accepts> {
    Some(match command {
        "ctf-diff" => Accepts {
            positional: false,
            values: &[
                "g",
                "r",
                "g-log",
                "r-log",
                "out",
                "limit",
                "allow-types",
                "allow-symbols",
            ],
            switches: &[],
        },
        "frames" => Accepts {
            positional: false,
            values: &["g", "r", "g-root", "r-root", "limit", "top"],
            switches: &[],
        },
        "kmod-relocs" => Accepts {
            positional: true,
            values: &["limit"],
            switches: &[],
        },
        "mixed" => Accepts {
            positional: false,
            values: &[
                "g",
                "r",
                "g-root",
                "r-root",
                "into",
                "link",
                "test",
                "ctf-g",
                "ctf-r",
                "allow-types",
            ],
            switches: &["dry-run"],
        },
        "speed" => Accepts {
            positional: false,
            values: &[
                "g", "r", "g-root", "r-root", "g-text", "r-text", "build", "units", "limit",
                "bench",
            ],
            switches: &[],
        },
        "test" => Accepts {
            positional: false,
            values: &[
                "gg",
                "rg",
                "gr",
                "rr",
                "gg-image",
                "rg-image",
                "gr-image",
                "rr-image",
                "baseline",
                "out",
                "work",
                "limit",
                "suite",
                "row",
                "firmware",
                "tests-only",
                "tests-except",
                "boot-timeout",
                "timeout",
                "reboots",
                "spare-disks",
                "disk-size",
                "cpus",
                "memory",
                "qemu",
                "ovmf-code",
                "ovmf-vars",
            ],
            switches: &["keep", "dry-run"],
        },
        _ => return None,
    })
}

/// The usage text.
pub const USAGE: &str = "\
rb: build pinned FreeBSD and NetBSD trees with rucc and with a reference GCC

usage:
  rb abi-diff --g DIR --r DIR [--jobs N] [--limit N]
      Compare the libraries of two worlds (plan 09.4). DIR holds the libraries of one side, such
      as the tree of scripts/freebsd/libs.sh, and files are paired by their path under it. For
      each shared object: each dynamic symbol with its version (name@@V, name@V for a hidden
      version, or name), and its binding, type, visibility and size; the version definitions and
      needs; DT_SONAME, DT_NEEDED, DT_FLAGS and DT_FLAGS_1; the program headers with their flags;
      and the number of IRELATIVE relocations. For each archive, such as libc.a, the ifunc
      symbols of each member. A link must point to the same place, and another file must have
      the same bytes. A file on one side only counts. The size of a function and a symbol that
      is used but not defined may differ: they are listed but not counted. Each list stops after
      N lines, 20 by default. Exits 1 when a difference counts or no file is paired.
  rb baseline PIN --conf C (--image IMG | --from DIR,DIR,DIR) [--row ROW] [--runs N] [--dir DIR]
          [--work DIR] [--system freebsd|netbsd] [--suite boot,smoke,tests] [the options of rb boot]
      Boot G's image N times, 3 by default, each on a fresh overlay, and write the baseline to
      DIR/<system>/<row>/<version>/<conf>. DIR is baselines/ by default. The baseline has each
      case's result in each run, the cases that passed in every run, the counts per run, the
      kernel messages and the image's SHA-256. The boots go to work/baseline/... and run every
      suite by default. With --from, the runs are the output directories of earlier rb boot
      commands, and nothing boots. A run in shards, one boot per shard, joins the directories
      of its shards with '+', as in --from smoke+net+lib. Exits 1 when a run did not pass.
  rb bench PIN --g-image IMG --r-image IMG --g-bench FILE --r-bench FILE [--runs N] [--row ROW]
          [--dir DIR] [--work DIR] [--system freebsd|netbsd] [--bench-scale S]
          [--bench-only NAME,...] [the options of rb boot]
  rb bench PIN --g-from DIR,... --r-from DIR,... [--g-image IMG] [--r-image IMG] [--dir DIR]
      Run the benchmark set of plan 13.2 on G and R (13.3). The first form boots R, then G, N
      times each, 10 by default, each on a fresh overlay with the bench suite and the rb-bench
      that its own compiler built, FILE of --g-bench or --r-bench. The boots go to
      work/bench/<system>/<row>/<version>/r-N and g-N. The second form reads the output
      directories of earlier boots, and nothing boots. For each benchmark, rb takes the median per
      side and R's speed against G: G's time over R's, or R's rate over G's, so under 1 is
      slower. A benchmark is gated with a value in every run of both sides, 3 or more runs, and a
      CV of at most 1% on G. The report gives the geometric mean of the gated speeds and the
      benchmarks behind by more than 2%. DIR/<system>/<row>/<version>/bench.tsv gets the summary
      and runs.tsv every value; DIR is results/bench by default. --bench-scale multiplies the
      work, and --bench-only runs only the benchmarks named. Exits 1 when a run wrote no results.
  rb boot --image IMG --out DIR [--suite boot,smoke,tests,modules,bench,foreign] [--row ROW]
          [--firmware ovmf|seabios|edk2|u-boot]
          [--tests-only DIR,...] [--tests-except DIR,...] [--tests-repeat N]
          [--modules DIR] [--modules-only NAME,...] [--modules-skip NAME,...]
          [--bench FILE] [--bench-scale S] [--bench-only NAME,...] [--pkgs DIR]
          [--boot-timeout SECS] [--timeout SECS]
          [--reboots N] [--spare-disks N] [--disk-size SIZE] [--cpus N] [--memory SIZE]
          [--qemu PATH] [--ovmf-code FILE --ovmf-vars FILE] [--edk2-code FILE --edk2-vars FILE]
          [--u-boot FILE] [--in-place] [--keep] [--login] [--scripts] [--dry-run]
      Boot an image once under QEMU, on a qcow2 overlay so the image does not change. The second
      disk holds an rb.conf with the suites, boot by default, and rb-rc writes its results back
      to it. The console goes to DIR/console.log. rb waits for RB-READY or a login prompt for
      SECS, the row's boot_limit_seconds, else 600 under KVM and 1800 under TCG, then for RB-DONE
      until the row's run limit. With --login, for an image with no rb-rc such as a release
      image, rb logs in as root at the login prompt and starts rb-rc from the results disk,
      which then also holds rb-rc's scripts, and it waits for RB-READY. With --scripts, the
      results disk holds the suite scripts of this rb, and rb-rc runs them and not the image's.
      On NetBSD a directory of --tests-only can be under the top, such as net/carp, and
      --tests-repeat N runs the directories of --tests-only N times. The modules suite, on
      NetBSD, loads and unloads each kernel module of the image, or of DIR of --modules, a tree
      of NAME/NAME.kmod that goes on the results disk; --modules-only and --modules-skip choose
      the modules. The bench suite runs the benchmark set once, with the rb-bench of --bench,
      which goes on the results disk, else the image's. The foreign suite installs the packages
      of DIR of --pkgs, which rb foreign fetched and which go on the results disk, and runs the
      check of each. On a panic it sends bt to ddb and stops QEMU. The results go to
      DIR/results and the outcome to DIR/boot.json. ROW is amd64 by default, and the firmware is
      the row's test firmware. edk2 is EDK2 for aarch64 in flash. u-boot is QEMU's OpenSBI, then
      the S-mode U-Boot of riscv64, which loads the EFI loader from the disk.
      Exits 1 when the boot did not pass.
  rb build PIN --cc rucc|ref --root DIR [--row ROW] [--targets T,...] [--src DIR] [--jobs N]
          [--conf CONF] [--kernconf NAME] [--rucc PATH] [--log FILE] [--shim PATH] [--noise N]
          [--bringup as,cxx-c [--bringup-units UNIT,...]] [--no-shim] [--ungraded] [--dry-run]
      Build the pin for the row with R's or G's compiler, as the workflows do. The tree is DIR/src
      or --src, and the build writes DIR/obj, DIR/destdir, DIR/release and DIR/logs. First rb makes
      DIR/toolchain-<cc> as rb toolchain does and runs its checks; the compile records go to
      DIR/logs/compile-<cc>.jsonl or FILE. For NetBSD each target is one build.sh run with the
      TOOLDIR of the shim, -U and -u; the TOOLDIR of build.sh tools is DIR/tools, and the target
      tools builds it. The targets are distribution, kernel=GENERIC, releasekernel=GENERIC and
      sets by default. kernel and releasekernel with no =CONF get the --kernconf NAME, GENERIC by
      default. For FreeBSD the targets are world and kernel by default, run by make with
      CROSS_TOOLCHAIN set to the .mk file of rb toolchain and SRCCONF=conf/freebsd/CONF.conf,
      graded by default; another target, such as installworld, gets DESTDIR=DIR/destdir.
      --bringup, else RB_BRINGUP, turns on the bring-up classes of R: as hands .S files, and C
      units rucc compiles but cannot assemble, to the reference; cxx-c hands the units of
      --bringup-units or RB_BRINGUP_UNITS, each a path suffix and its issue such as
      lib/libc/gen/x.c=tamnd/rucc#1, to the reference GCC. A path that ends in / names each
      unit under that directory, such as stand/=tamnd/rucc#4097. The record of the call names both. Either needs --ungraded, and the report says the run is ungraded in its
      first line. --no-shim builds G with the reference alone, as the B0 workflows do. Each target's output
      goes to DIR/logs/<target>.log and a line to DIR/logs/phases.tsv. --dry-run prints the
      commands. Exits 1 when a check or a target fails.
  rb census LOG [--objdir DIR] [--skip PATH,...] [--objects FILE] [--limit N]
      Say which compiler made each object of one build, from the compile.jsonl that rb-cc,
      rb-c++, rb-cpp and rb-as wrote. With --objdir, also walk the object tree, and count every
      object no call wrote as other; --skip leaves out paths under DIR, such as the host tools.
      --objects writes every object, its origin and its slot as tab-separated values. Lists in
      the report stop after N lines, 50 by default. When a call was delegated under bring-up, the
      first line says the build is ungraded, and the report lists each delegated unit with what
      rucc said about it.
  rb cross-modules --image IMG --out DIR [--modules DIR] [--attempts N] [--modules-skip NAME,...]
          [the options of rb boot]
  rb cross-modules --gg DIR [--rg DIR] [--gr DIR] [--rr DIR] [--limit N]
      Load the kernel modules of one build into the kernel of another (plan 09.3). The first
      form runs one cell: it boots IMG with the modules suite, with the modules of --modules or
      else the image's, to DIR/boot-N. When a boot stops in a panic or a timeout while a module
      loads, the module gets that result, and the next boot skips it, up to N boots, 8 by
      default. DIR/modules.tsv gets each module's result. Exits 1 unless every module passed.
      The second form compares the cells, each the DIR of the first form. In a cell name the
      first letter is the kernel and the second the modules. A module that passes in GG and not
      in another cell is a finding. Exits 1 when there is a finding.
  rb ctf-diff --g FILE|DIR --r FILE|DIR [--g-log LOG --r-log LOG] [--out DIR] [--limit N]
          [--allow-types T,T] [--allow-symbols S,S]
      Compare the CTF of G's and R's kernels and modules. Each side is an ELF file with a
      .SUNW_ctf section, or a directory, where the files with one are paired by their path under
      it. Each named type must have the same members, member types and bit offsets, or the same
      values, and each data and function symbol must have the same type, or a type where G has
      none. An array with no length in G may have its length in R. The symbol types are read in
      symbol table order, as libctf reads them. All types, anonymous ones too, are compared by
      kind and by how a use of them is written, and are reported, but they do not count. LOG is a
      build log as text: the warnings and errors of ctfconvert and ctfmerge only R's log has are
      the losses. --allow-types and --allow-symbols name the named types and symbols whose
      differences are reported as allowed and do not count. --out writes the named types of each
      side and the differences. Each list stops after N lines, 40 by default. Exits 1 when a
      difference counts, a file has CTF on one side only, or R has a loss.
  rb demands LOG [--pin PIN --row ROW --conf CONF [--write]] [--flags-only] [--jobs N] [--limit N]
      Count the compiler features one build uses, from its compile.jsonl. Every C and assembly
      unit of the C compiler slot is run again with -E in place of -c, in its own directory, and
      its text is scanned for attributes, builtins, keywords, inline and file-scope asm, assembler
      directives, mnemonics and pragmas; its -f, -m, -W, --param and -std= flags are counted too.
      Run it where the build ran. --flags-only reads the command lines and runs nothing. --write
      puts the result in demands.toml under the pin, row and configuration. N units run at once,
      one per CPU by default, and each list of the report stops after 30 lines by default.
  rb flags-diff PIN --g LOG --r LOG [--row ROW] [--g-root DIR --r-root DIR] [--divergences FILE]
          [--limit N] [--paired-only]
      Compare the command line of every unit in two builds, from their compile.jsonl. Units are
      paired by the object they made. The compiler's path, the persona flag -fgnuc-version= and
      the target flag are left out, and each build's DIR is replaced by <root>. Units whose
      remaining flags differ in the same way are one group, and each group lists its units. A
      flag that an entry of mk-divergences.toml, or FILE, names is left out with its reason.
      Units only one build made are listed, and they count unless --paired-only is given. Each
      list stops after N lines, 20 by default. Exits 1 when a difference counts.
  rb fetch PIN [--tarball]
  rb fetch --set current|releases [--tarball]
      Fetch a pinned tree into RB_CACHE/trees and verify it. A pin with a commit is fetched one
      commit deep with git, and HEAD must be the pinned commit. A release with --tarball, or a pin
      with no commit, is downloaded and checked against the digest the project publishes. The
      tree's newvers.sh or param.h must give the pin's version. Prints where the tree is.
  rb foreign --conf FILE --out DIR
  rb foreign --r DIR [--g DIR] [--limit N]
      Run the project's own packages on a rucc world (plan 12.8). The first form reads FILE, such
      as conf/netbsd/foreign.toml, which names a package repository and the packages to check.
      It fetches the repository's pkg_summary.gz and downloads each package and what it needs to
      DIR/All, and writes DIR/checks for the foreign suite of rb boot --pkgs DIR. A file whose
      size is not the one of the summary is an error. The second form reads the foreign results
      of an rb boot of R, DIR/results/foreign/foreign.tsv, and of G with --g. A package that did
      not pass on R counts when it passed on G, or when there are no G results. Each list stops
      after N lines, 20 by default. Exits 1 when a package counts or none ran.
  rb frames --g LOG --r LOG [--g-root DIR --r-root DIR] [--limit N] [--top N]
      Compare the stack frame of each function in R and G, from the .su files that -fstack-usage
      writes beside each object, X.su for X.o. LOG is the compile.jsonl of each build, objects are
      paired as for rb sections-diff, and functions by name. A finding is a frame of R that is
      larger than G's by more than 25% and 128 bytes, over the unit's -Wstack-usage= limit when
      G's is not, or dynamic when G's is static, and a unit where G wrote a .su file and R did not.
      Functions in one file only are listed and not counted. The N largest differences, 100 by
      default, are listed for information. Each other list stops after N lines of --limit, 20 by
      default. Exits 1 when there is a finding or no function was paired.
  rb image DIR|--build DIR --out IMG [--system freebsd|netbsd] [--suites boot,smoke,tests] [--packages DIR]
           [--size SIZE] [--tools DIR] [--work DIR] [--dry-run]
      Make a raw disk image from a build, with rb-rc and the smoke suite in it. For FreeBSD, DIR
      is the staging tree of installworld, installkernel and distribution; rb writes its files
      into it and runs makefs and mkimg, from --tools or PATH. SIZE is the root file system, 10g
      by default. For NetBSD, DIR is the TOOLDIR of build.sh or its nbmake wrapper; rb runs the
      live-image target with its files added, and grows the image to SIZE. The suites go in
      /etc/rb.conf, boot only by default. --packages copies a local package repository to
      /var/db/rb-pkg. The logs go to --work, IMG.work by default. --dry-run prints the commands.
  rb kmod-relocs DIR [--limit N]
      Check that each kernel module under DIR, every *.ko file, has only the relocations the
      FreeBSD amd64 kernel linker applies: in a relocation section whose target section is
      SHF_ALLOC, the types NONE, 64, PC32, PLT32, 32S, GLOB_DAT, JUMP_SLOT, RELATIVE and
      IRELATIVE. Any other type there makes kldload refuse the module. A file that is not an
      x86-64 ET_REL object is listed and not counted. Each list stops after N lines, 40 by
      default. Exits 1 when a module has another type, a module cannot be read, or there is no
      module.
  rb mixed --g LOG --r LOG [--g-root DIR --r-root DIR] --into DIR --ctf-g K --ctf-r K
          [--allow-types T,T] (--link CMD --test CMD | --dry-run)
      Find the object that makes R fail, by bisection over builds that mix R's and G's objects. The
      objects are the units both compile logs have, sorted by path. Step k copies R's objects of the
      first k units and G's of the rest to the object's path with DIR of --into in place of the
      build's root, and runs CMD of --link and CMD of --test with sh -c. The test exits 0 when the
      result behaves as G does, 125 when it cannot tell, and with another status when it fails as R
      does. A mix that does not link counts as 125. RB_MIXED_R and RB_MIXED_UNITS give k and the
      count. All of G's objects must be good and all of R's bad. rb mixed does not run when the CTF
      of the two kernels K, read as rb ctf-diff reads them, has a named type that differs and that
      --allow-types does not name. --dry-run pairs the objects and checks the CTF only. Exits 0 when
      there is nothing to find, because all of R's objects are good. Exits 1 when an object is
      found, when all of G's objects are not good, or when the test cannot tell.
  rb mk-diff PIN [--row ROW] (--dir DIR | --g G [--r R]) [--src DIR] [--conf CONF]
          [--kernel DIR|G,R] [--work DIR] [--divergences FILE]
      Read the make variables that depend on the compiler, in G's and R's configuration, and
      compare them. G and R are toolchain descriptions: for FreeBSD the CROSS_TOOLCHAIN files, or
      DIR/ref.mk and DIR/rucc.mk; for NetBSD the TOOLDIRs, whose bin/nbmake-MACHINE build.sh has
      written. make runs in the tree --src at the top, in lib/libc and bin/sh, in stand/libsa on
      FreeBSD and in the kernel build directory --kernel. FreeBSD adds make showconfig, NetBSD
      adds every MK*, HAVE_* and CC_WNO_* of share/mk, and --kernel adds its opt_*.h files.
      FreeBSD reads src.conf from conf/freebsd/CONF.conf, graded by default. Each side's values
      go to WORK/mk-G.txt and WORK/mk-R.txt, and WORK is mk-diff by default. G or R may be such a
      file from an earlier run. With --g alone, rb writes G's file and stops. A difference that
      mk-divergences.toml, or FILE, allows is listed with its reason and does not count. Exits 1
      when a difference counts.
  rb personas [--arch ARCH]
      Print the persona, the reference toolchain and rucc's --target for every pin, from
      pins.toml, personas.toml and toolchains.toml. ARCH is rucc's architecture, x86_64 by
      default. Exits 1 when a persona needs review, such as a NetBSD BASE-VER that moved.
  rb probes PIN [--row ROW] (--dir DIR | --g G --r R) [--from LOG] [--work DIR] [--all]
          [--details N] [--divergences FILE]
      Ask G's and R's C compilers the questions a build asks, without building, and say for each
      whether they answer the same. DIR is a FreeBSD directory of rb toolchain. G and R are
      compilers, or directories of rb toolchain or rb shim whose rb-shim.toml names a cc slot,
      such as NetBSD TOOLDIRs. The probes are --version as bsd.compiler.mk reads it, the gcc
      version of -v, echo __FreeBSD_cc_version | cc -E -, the -print- questions, the macros of
      -dM -E, the __has_feature and __has_extension queries of the systems' headers, the kernel
      and boot code flags rucc may refuse, and -B and --sysroot as buildworld gives them, on a
      small tree in the work directory. --from adds every C compiler probe of a compile.jsonl,
      once each. The probes run in DIR of --work, and the shims' records of them go to
      probes-G.jsonl and probes-R.jsonl there. --all prints the probes that are the same too. A
      macro that mk-divergences.toml, or FILE, allows is listed with its reason and does not
      count. Exits 1 when a probe differs.
  rb sections-diff --g LOG --r LOG [--g-root DIR --r-root DIR] [--allow FILE] [--jobs N]
          [--limit N]
      Compare each object of two builds. LOG is the compile.jsonl of each build, and objects are
      paired by path, with the roots replaced as for rb flags-diff. Section names must be the
      same. Each section both objects have must have the same flags, type, entry size and group,
      and each data section the same size. Each linker set, set_* or link_set_*, must hold the
      same entries, with the .N of a static local's name left out. Code, read-only data,
      relocation, debug, .SUNW_ctf and .comment sections may differ: they are listed but not
      counted. FILE lists statics by source and name, each with a reason, such as
      conf/netbsd/sections-allow.toml. A data section that differs is allowed when an allowed
      static of the unit is in it, and its other object symbols are the same in both builds.
      Each difference is one line with the unit's source, grouped by kind. Each list stops after
      N lines, 20 by default. Exits 1 when a difference counts or no object is paired.
  rb sets [--as-of TIME] [--write]
      Read what FreeBSD and NetBSD publish, expand sets.toml into pins and compare them with
      pins.toml. --write rewrites pins.toml. --as-of takes the newest commit at or before TIME on
      each branch's first-parent line, such as 2026-11-08T06:00:00Z. A branch pin is good at the
      commit of a complete baseline under baselines/ made by TIME. Under last-g-good it stays at
      that commit until G is good at the head. Exits 1 when pins.toml is out of date and --write is
      not given.
  rb shim PIN --dir DIR [--row ROW] [--cc rucc|ref] [--rucc PATH] [--log FILE] [--tools DIR]
          [--destdir DIR] [--shim PATH] [--bringup as,cxx-c [--bringup-units UNIT,...]]
      Put rb-cc under the names a build calls its compilers by in DIR, with the rb-shim.toml that
      says what each name runs. rucc gets the --target and -fgnuc-version= of the pin's persona.
      rucc is PATH, else RB_RUCC, else the [rucc] command of toolchains.toml on PATH; rb prints
      its SHA-256, its --version and whether it is the pinned rucc. For FreeBSD the names are
      <triple>-rb-cc-rucc, -rb-cc-ref, -rb-c++, -rb-cpp and -rb-as, and the reference is the
      package of toolchains.toml. For NetBSD the names are the C compiler and the preprocessor of
      TOOLDIR, the reference is in the TOOLDIR of build.sh tools given by --tools, --cc gives the
      side, and rucc gets --destdir as its sysroot. Each call appends a record to FILE. The shim
      binary is PATH, else RB_SHIM, else rb-cc next to rb. --bringup and --bringup-units are
      those of rb build, and go in rb-shim.toml for R.
  rb symbols-diff --g LOG --r LOG [--g-root DIR --r-root DIR] [--jobs N] [--limit N]
      Compare the symbols of each object of two builds, paired as for rb sections-diff. Each
      symbol that is not local must have the same name, binding, type and visibility in both,
      and a data symbol the same size. Function sizes and the names a compiler makes, such as
      .L*, *.cold, *.isra.* and *.constprop.*, may differ: they are listed but not counted. Exits
      1 when a difference counts or no object is paired.
  rb syntax PIN --log LOG [--row ROW] [--rucc PATH] [--sysroot DIR] [--jobs N] [--limit N]
          [--out FILE]
      Check every C unit of a G build with rucc's front end. LOG is the compile.jsonl of the
      build. Each C unit runs through rucc with -fsyntax-only and each .S unit with -E, with the
      unit's own flags and the persona's arguments, in the directory the build ran it in. Nothing
      is written. --sysroot is added to each unit that names none, such as the DESTDIR of a NetBSD
      build. rucc is found as for rb shim. The report lists the units that failed with rucc's
      first error, the flags rucc refused, and the units with a warning G did not give, compared
      as rb warnings does. --out writes one line per unit as tab-separated values. Exits 1 when a
      unit failed or has such a warning.
  rb speed [--g LOG --r LOG [--g-root DIR --r-root DIR]] [--g-text FILE,... --r-text FILE,...]
          [--bench FILE,...] [--build NAME] [--units FILE] [--limit N]
      The compile time and size gates of 02.5 (plan 13.4). LOG is the compile.jsonl of each
      build, and units are paired by object as for rb flags-diff. Only C units and .S files of the
      C compiler slot count; probes, links, C++ units and failed calls do not. R's summed user and
      system CPU time must be at most 0.5 of G's, and no call of R's C compiler may use more than
      1.5 GiB. The report gives the percentiles of the per unit ratio and the N units with the
      worst one, 50 by default, over units where G took at least 10 ms, the wall time of each
      build, and R's probes slower than 20 ms. A unit in one build only fails the gates. Each
      FILE of --g-text is compared with the FILE of --r-text in the same place, such as the
      kernel and libc.so: the size of R's executable sections must be within 5% of G's. --build
      names the build in the report, and --units writes each paired unit as tab-separated
      values. Each FILE of --bench is the bench.tsv of an rb bench: the geometric mean of R's
      speed over the gated benchmarks must be at least 0.98, and no gated benchmark may be below
      0.95. The report lists every benchmark more than 2% behind. Exits 1 when a gate fails.
  rb test (--gg RUNS | --gg-image IMG | --baseline DIR) [--rg RUNS | --rg-image IMG]
          [--gr RUNS | --gr-image IMG] [--rr RUNS | --rr-image IMG] [--out DIR] [--work DIR]
          [--limit N] [the options of rb boot] [--dry-run]
      The graded run with the 2x2: GG, RG (R's kernel on G's world), GR (G's kernel on R's world)
      and RR. RUNS are output directories of rb boot, as rb baseline --from takes them: runs
      separated by ',' and the shards of one run by '+'. An image is booted into DIR/<cell>,
      work/test by default. A case is graded when every GG run passed it, and with --baseline
      when its verdict in DIR/cases.tsv is stable-pass. The claim is RR, or RG or GR when it is
      the only cell given. It runs every suite, boot,smoke,tests by default; an RG or GR image
      next to an RR claim is booted only for the suites and top-level test directories of the
      cases RR failed. Each graded case the claim does not pass gets a verdict from RG and GR:
      kernel, world, both or interaction, as plan 12.6 gives them, and only kernel with an RG
      claim and only world with a GR claim. --out writes attribution.tsv and test.md. The case
      list stops after N lines, 50 by default. Exits 1 when the claim does not pass a graded
      case, or nothing is graded.
  rb toolchain PIN --dir DIR [--row ROW] [--cc rucc|ref] [--rucc PATH] [--log FILE] [--tools DIR]
          [--destdir DIR] [--shim PATH] [--bringup as,cxx-c [--bringup-units UNIT,...]]
          [--ungraded] [--fallback]
      Make the toolchain a build of the pin reads, then run the checks of rb toolchain-diff on it.
      With --fallback, the persona's fallback of personas.toml builds G and rucc claims its
      version, as for FreeBSD stable/14 where amd64-gcc15 fails and amd64-gcc14 does not.
      For FreeBSD, DIR gets the shims in DIR/bin and the CROSS_TOOLCHAIN files DIR/ref.mk for G
      and DIR/rucc.mk for R. For NetBSD, DIR becomes a copy of the TOOLDIR of build.sh tools in
      --tools, with the shim as its C compiler and preprocessor for the side --cc gives. Give DIR
      to build.sh -T. The other options are those of rb shim. Exits 1 when a check fails.
  rb toolchain-diff PIN [--row ROW] (--dir DIR | --g G --r R) [--ungraded]
      Check that R's toolchain differs from G's in the C compiler only. For FreeBSD, G and R are
      the CROSS_TOOLCHAIN files, or DIR/ref.mk and DIR/rucc.mk: they must differ in XCC only, and
      the shims they name must run the reference of toolchains.toml and give rucc the persona's
      arguments. For NetBSD, G and R are TOOLDIRs, and one of them is enough: each file must be
      the same as in the TOOLDIR of build.sh tools, except the two shim names and rb-shim.toml.
      rucc must be the pinned rucc, and no bring-up class may be on, or --ungraded must be
      given. For a release pin, the SHA-256 of rucc must be in [rucc.binary_sha256]. For a commit
      pin, the file rucc.source beside rucc must name the pinned repo and commit and the SHA-256
      of rucc. Exits 1 when a check fails.
  rb vec-audit --g LOG --r LOG [--g-root DIR --r-root DIR] [--jobs N] [--limit N]
      Decode each object of R whose compile command leaves out the FPU, such as with -mno-sse or
      -msoft-float, and list each vector or x87 instruction in it: one that names an xmm, ymm,
      zmm, mm, st, mask or tile register, or is an x87 instruction. LOG is the compile.jsonl of
      each build, and objects are paired as for rb sections-diff. A mnemonic is allowed when G's
      object of the same unit has it too, as in inline asm that uses the FPU by design; else it is
      a finding, with the functions it is in. Objects not for x86-64 are skipped. Each list stops
      after N lines, 20 by default. Exits 1 when there is a finding, an object cannot be read or no
      object was decoded.
  rb version
  rb warnings --g LOG --r LOG [--g-root DIR --r-root DIR] [--limit N]
      Compare the warnings of each unit in two builds. LOG is the compile.jsonl of each build, and
      units are paired by object, with the roots replaced as for rb flags-diff. A diagnostic is
      its file, line and -W option. R's diagnostics must be a subset of G's: each one only R gives
      is counted, and each one only G gives is listed but not counted. Only the first KiB of a
      unit's standard error is kept, so the report says where it was cut. Exits 1 when R gives a
      diagnostic G does not.

The repository is found by walking up to sets.toml, or from RB_ROOT. Downloads and fetched
trees go to RB_CACHE, or ~/.cache/rb.
";

impl Args {
    /// Parse the words after the program name, with the commands `table` knows.
    pub fn parse(words: &[String], table: fn(&str) -> Option<Accepts>) -> Result<Self, String> {
        let mut words = words.iter();
        let command = match words.next() {
            None => "help".to_string(),
            Some(w) if w == "-h" || w == "--help" => "help".to_string(),
            Some(w) if w == "--version" => "version".to_string(),
            Some(w) => w.clone(),
        };
        let accepts =
            table(&command).ok_or_else(|| format!("unknown command {command}; see rb help"))?;
        let mut target = None;
        let mut values = BTreeMap::new();
        let mut switches = Vec::new();
        while let Some(word) = words.next() {
            let Some(option) = word.strip_prefix("--") else {
                if accepts.positional && target.is_none() {
                    target = Some(word.clone());
                    continue;
                }
                return Err(format!("unexpected argument {word}"));
            };
            let (name, inline) = match option.split_once('=') {
                Some((n, v)) => (n, Some(v.to_string())),
                None => (option, None),
            };
            if name == "help" {
                switches.push("help".to_string());
                continue;
            }
            if accepts.switches.contains(&name) {
                if inline.is_some() {
                    return Err(format!("--{name} takes no value"));
                }
                switches.push(name.to_string());
                continue;
            }
            if !accepts.values.contains(&name) {
                return Err(format!("rb {command} has no option --{name}"));
            }
            let value = match inline {
                Some(v) => v,
                None => words
                    .next()
                    .cloned()
                    .ok_or_else(|| format!("--{name} needs a value"))?,
            };
            values.insert(name.to_string(), value);
        }
        Ok(Self {
            command,
            target,
            values,
            switches,
        })
    }

    /// The value of an option, if given.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&str> {
        self.values.get(name).map(String::as_str)
    }

    /// Whether a switch was given.
    #[must_use]
    pub fn has(&self, name: &str) -> bool {
        self.switches.iter().any(|s| s == name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table(command: &str) -> Option<Accepts> {
        match command {
            "fetch" => Some(Accepts {
                positional: true,
                values: &["set"],
                switches: &["all"],
            }),
            "sets" => Some(Accepts {
                positional: false,
                values: &["as-of"],
                switches: &["write"],
            }),
            _ => accepted(command),
        }
    }

    fn parse(words: &[&str]) -> Result<Args, String> {
        let words: Vec<String> = words.iter().map(|s| (*s).to_string()).collect();
        Args::parse(&words, table)
    }

    #[test]
    fn a_pin_then_options() {
        let args = parse(&["fetch", "netbsd-trunk", "--all"]).unwrap();
        assert_eq!(args.target.as_deref(), Some("netbsd-trunk"));
        assert!(args.has("all"));
        assert!(!args.has("write"));
    }

    #[test]
    fn values_come_after_a_space_or_an_equals_sign() {
        let args = parse(&["fetch", "--set", "current"]).unwrap();
        assert_eq!(args.get("set"), Some("current"));
        let args = parse(&["sets", "--as-of=2026-11-08T06:00:00Z", "--write"]).unwrap();
        assert_eq!(args.get("as-of"), Some("2026-11-08T06:00:00Z"));
        assert!(args.has("write"));
    }

    #[test]
    fn a_value_may_start_with_a_dash() {
        let args = parse(&["fetch", "--set", "-x"]).unwrap();
        assert_eq!(args.get("set"), Some("-x"));
    }

    #[test]
    fn mistakes_are_named() {
        let unknown = parse(&["frobnicate"]).unwrap_err();
        assert!(unknown.contains("unknown command"), "{unknown}");
        let extra = parse(&["sets", "netbsd-trunk"]).unwrap_err();
        assert!(extra.contains("unexpected"), "{extra}");
        let second = parse(&["fetch", "a", "b"]).unwrap_err();
        assert!(second.contains("unexpected argument b"), "{second}");
        let option = parse(&["fetch", "--row", "amd64"]).unwrap_err();
        assert!(option.contains("--row"), "{option}");
        let missing = parse(&["fetch", "--set"]).unwrap_err();
        assert!(missing.contains("needs a value"), "{missing}");
        let switch = parse(&["sets", "--write=yes"]).unwrap_err();
        assert!(switch.contains("takes no value"), "{switch}");
    }

    #[test]
    fn nothing_at_all_is_help_and_the_dash_forms_work() {
        assert_eq!(parse(&[]).unwrap().command, "help");
        assert_eq!(parse(&["-h"]).unwrap().command, "help");
        assert_eq!(parse(&["--version"]).unwrap().command, "version");
        assert!(parse(&["version", "--help"]).unwrap().has("help"));
    }

    #[test]
    fn every_command_in_the_usage_is_accepted() {
        for line in USAGE.lines() {
            let Some(rest) = line.trim_start().strip_prefix("rb ") else {
                continue;
            };
            let command = rest.split_whitespace().next().unwrap_or_default();
            assert!(accepted(command).is_some(), "rb {command} is in the usage");
        }
    }
}
