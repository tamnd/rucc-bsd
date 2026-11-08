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
        _ => return None,
    })
}

/// The usage text.
pub const USAGE: &str = "\
rb: build pinned FreeBSD and NetBSD trees with rucc and with a reference GCC

usage:
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
  rb personas [--arch ARCH]
      Print the persona, the reference toolchain and rucc's --target for every pin, from
      pins.toml, personas.toml and toolchains.toml. ARCH is rucc's architecture, x86_64 by
      default. Exits 1 when a persona needs review, such as a NetBSD BASE-VER that moved.
  rb sets [--as-of TIME] [--write]
      Read what FreeBSD and NetBSD publish, expand sets.toml into pins and compare them with
      pins.toml. --write rewrites pins.toml. --as-of takes the newest commit at or before TIME,
      such as 2026-11-08T06:00:00Z. Exits 1 when pins.toml is out of date and --write is not given.
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
