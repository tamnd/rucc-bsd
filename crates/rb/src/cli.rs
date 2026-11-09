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
            switches: &["in-place", "keep", "dry-run"],
        },
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
        "sets" => Accepts {
            positional: false,
            values: &["as-of"],
            switches: &["write"],
        },
        _ => return toolchain_commands(command),
    })
}

/// What the commands that set up compilers accept.
fn toolchain_commands(command: &str) -> Option<Accepts> {
    Some(match command {
        "shim" => Accepts {
            positional: true,
            values: &[
                "row", "dir", "cc", "rucc", "log", "tools", "destdir", "shim",
            ],
            switches: &[],
        },
        "toolchain" => Accepts {
            positional: true,
            values: &[
                "row", "dir", "cc", "rucc", "log", "tools", "destdir", "shim",
            ],
            switches: &["ungraded"],
        },
        "probes" => Accepts {
            positional: true,
            values: &["row", "dir", "g", "r", "from", "work", "details"],
            switches: &["all"],
        },
        "toolchain-diff" => Accepts {
            positional: true,
            values: &["row", "dir", "g", "r"],
            switches: &["ungraded"],
        },
        _ => return None,
    })
}

/// The usage text.
pub const USAGE: &str = "\
rb: build pinned FreeBSD and NetBSD trees with rucc and with a reference GCC

usage:
  rb baseline PIN --conf C (--image IMG | --from DIR,DIR,DIR) [--row ROW] [--runs N] [--dir DIR]
          [--work DIR] [--system freebsd|netbsd] [--suite boot,smoke,tests] [the options of rb boot]
      Boot G's image N times, 3 by default, each on a fresh overlay, and write the baseline to
      DIR/<system>/<row>/<version>/<conf>. DIR is baselines/ by default. The baseline has each
      case's result in each run, the cases that passed in every run, the counts per run, the
      kernel messages and the image's SHA-256. The boots go to work/baseline/... and run every
      suite by default. With --from, the runs are the output directories of earlier rb boot
      commands, and nothing boots. A run in shards, one boot per shard, joins the directories
      of its shards with '+', as in --from smoke+net+lib. Exits 1 when a run did not pass.
  rb boot --image IMG --out DIR [--suite boot,smoke,tests] [--row ROW] [--firmware ovmf|seabios]
          [--tests-only DIR,...] [--tests-except DIR,...] [--boot-timeout SECS] [--timeout SECS]
          [--reboots N] [--spare-disks N] [--disk-size SIZE] [--cpus N] [--memory SIZE]
          [--qemu PATH] [--ovmf-code FILE --ovmf-vars FILE] [--in-place] [--keep] [--dry-run]
      Boot an image once under QEMU, on a qcow2 overlay so the image does not change. The second
      disk holds an rb.conf with the suites, boot by default, and rb-rc writes its results back
      to it. The console goes to DIR/console.log. rb waits for RB-READY or a login prompt for
      SECS, 600 by default under KVM, then for RB-DONE until the row's run limit. On a panic it
      sends bt to ddb and stops QEMU. The results go to DIR/results and the outcome to
      DIR/boot.json. ROW is amd64 by default, and the firmware is the row's test firmware.
      Exits 1 when the boot did not pass.
  rb census LOG [--objdir DIR] [--skip PATH,...] [--objects FILE] [--limit N]
      Say which compiler made each object of one build, from the compile.jsonl that rb-cc,
      rb-c++, rb-cpp and rb-as wrote. With --objdir, also walk the object tree, and count every
      object no call wrote as other; --skip leaves out paths under DIR, such as the host tools.
      --objects writes every object, its origin and its slot as tab-separated values. Lists in
      the report stop after N lines, 50 by default.
  rb demands LOG [--pin PIN --row ROW --conf CONF [--write]] [--flags-only] [--jobs N] [--limit N]
      Count the compiler features one build uses, from its compile.jsonl. Every C and assembly
      unit of the C compiler slot is run again with -E in place of -c, in its own directory, and
      its text is scanned for attributes, builtins, keywords, inline and file-scope asm, assembler
      directives, mnemonics and pragmas; its -f, -m, -W, --param and -std= flags are counted too.
      Run it where the build ran. --flags-only reads the command lines and runs nothing. --write
      puts the result in demands.toml under the pin, row and configuration. N units run at once,
      one per CPU by default, and each list of the report stops after 30 lines by default.
  rb fetch PIN [--tarball]
  rb fetch --set current|releases [--tarball]
      Fetch a pinned tree into RB_CACHE/trees and verify it. A pin with a commit is fetched one
      commit deep with git, and HEAD must be the pinned commit. A release with --tarball, or a pin
      with no commit, is downloaded and checked against the digest the project publishes. The
      tree's newvers.sh or param.h must give the pin's version. Prints where the tree is.
  rb image DIR|--build DIR --out IMG [--system freebsd|netbsd] [--suites boot,smoke,tests] [--packages DIR]
           [--size SIZE] [--tools DIR] [--work DIR] [--dry-run]
      Make a raw disk image from a build, with rb-rc and the smoke suite in it. For FreeBSD, DIR
      is the staging tree of installworld, installkernel and distribution; rb writes its files
      into it and runs makefs and mkimg, from --tools or PATH. SIZE is the root file system, 10g
      by default. For NetBSD, DIR is the TOOLDIR of build.sh or its nbmake wrapper; rb runs the
      live-image target with its files added, and grows the image to SIZE. The suites go in
      /etc/rb.conf, boot only by default. --packages copies a local package repository to
      /var/db/rb-pkg. The logs go to --work, IMG.work by default. --dry-run prints the commands.
  rb personas [--arch ARCH]
      Print the persona, the reference toolchain and rucc's --target for every pin, from
      pins.toml, personas.toml and toolchains.toml. ARCH is rucc's architecture, x86_64 by
      default. Exits 1 when a persona needs review, such as a NetBSD BASE-VER that moved.
  rb probes PIN [--row ROW] (--dir DIR | --g G --r R) [--from LOG] [--work DIR] [--all]
          [--details N]
      Ask G's and R's C compilers the questions a build asks, without building, and say for each
      whether they answer the same. DIR is a FreeBSD directory of rb toolchain. G and R are
      compilers, or directories of rb toolchain or rb shim whose rb-shim.toml names a cc slot,
      such as NetBSD TOOLDIRs. The probes are --version as bsd.compiler.mk reads it, the gcc
      version of -v, echo __FreeBSD_cc_version | cc -E -, the -print- questions, the macros of
      -dM -E, the __has_feature and __has_extension queries of the systems' headers, and the
      kernel and boot code flags rucc may refuse. --from adds every C compiler probe of a
      compile.jsonl, once each. The probes run in DIR of --work, and the shims' records of them go
      to probes-G.jsonl and probes-R.jsonl there. --all prints the probes that are the same too.
      Exits 1 when a probe differs.
  rb sets [--as-of TIME] [--write]
      Read what FreeBSD and NetBSD publish, expand sets.toml into pins and compare them with
      pins.toml. --write rewrites pins.toml. --as-of takes the newest commit at or before TIME,
      such as 2026-11-08T06:00:00Z. Exits 1 when pins.toml is out of date and --write is not given.
  rb shim PIN --dir DIR [--row ROW] [--cc rucc|ref] [--rucc PATH] [--log FILE] [--tools DIR]
          [--destdir DIR] [--shim PATH]
      Put rb-cc under the names a build calls its compilers by in DIR, with the rb-shim.toml that
      says what each name runs. rucc gets the --target and -fgnuc-version= of the pin's persona.
      rucc is PATH, else RB_RUCC, else the [rucc] command of toolchains.toml on PATH; rb prints
      its SHA-256, its --version and whether it is the pinned release. For FreeBSD the names are
      <triple>-rb-cc-rucc, -rb-cc-ref, -rb-c++, -rb-cpp and -rb-as, and the reference is the
      package of toolchains.toml. For NetBSD the names are the C compiler and the preprocessor of
      TOOLDIR, the reference is in the TOOLDIR of build.sh tools given by --tools, --cc gives the
      side, and rucc gets --destdir as its sysroot. Each call appends a record to FILE. The shim
      binary is PATH, else RB_SHIM, else rb-cc next to rb.
  rb toolchain PIN --dir DIR [--row ROW] [--cc rucc|ref] [--rucc PATH] [--log FILE] [--tools DIR]
          [--destdir DIR] [--shim PATH] [--ungraded]
      Make the toolchain a build of the pin reads, then run the checks of rb toolchain-diff on it.
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
      rucc must be the release of [rucc.binary_sha256], or --ungraded must be given. Exits 1
      when a check fails.
  rb version

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
