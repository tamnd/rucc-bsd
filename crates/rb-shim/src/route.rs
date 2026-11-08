//! Which compiler a call goes to, and what the census calls the object it makes.
//!
//! The shim is one binary with many names. The name says the slot: the C compiler, the C++
//! compiler, the preprocessor or the assembler (14.4). For the C compiler slot the name or the
//! configuration also says the side. Then the slot, the side and the call's inputs decide the
//! compiler (10.1):
//!
//! | Slot | R | G | Origin of the object |
//! |---|---|---|---|
//! | `cc`, C or assembly input | rucc, with the target and the persona | the reference GCC | `rucc-c` or `rucc-asm` in R, `ref` in G |
//! | `cc`, C++ input | the reference `g++` | the reference `g++` | `cxx-on-cc` |
//! | `cc`, a bring-up unit | the reference GCC | | `ref-c` |
//! | `c++` | the reference `g++` | the reference `g++` | `ref-cxx` |
//! | `cpp` | rucc in `-E` mode where configured, else the reference `cpp` | the reference `cpp` | none |
//! | `as` | the reference `as` | the reference `as` | `other` |

use crate::config::{ShimConfig, Side};
use harness_shim::args::{self, Invocation, Mode};
use std::path::{Path, PathBuf};

/// The four compiler slots of a BSD build.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Slot {
    /// The C compiler: FreeBSD's `XCC`, NetBSD's `TOOL_CC`.
    Cc,
    /// The C++ compiler: `XCXX`, `TOOL_CXX`.
    Cxx,
    /// The standalone preprocessor: `XCPP`, `TOOL_CPP`.
    Cpp,
    /// A direct assembler call.
    As,
}

impl Slot {
    /// The slot's name, as it goes on the record.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Cc => "cc",
            Self::Cxx => "c++",
            Self::Cpp => "cpp",
            Self::As => "as",
        }
    }

    /// Read a slot's name.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "cc" => Some(Self::Cc),
            "c++" => Some(Self::Cxx),
            "cpp" => Some(Self::Cpp),
            "as" => Some(Self::As),
            _ => None,
        }
    }
}

/// What a name says: the slot, and the side when the name fixes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Called {
    /// The slot.
    pub slot: Slot,
    /// The side, for the `-rucc` and `-ref` names of 05.4.
    pub side: Option<Side>,
}

/// Read the name the shim was called by.
///
/// The names in the configuration come first, then the `rb-` names, matched as suffixes so that
/// the triple prefixed names of 05.4, such as `x86_64-unknown-freebsd15.1-rb-cc-rucc`, work
/// without being listed.
#[must_use]
pub fn resolve(name: &str, config: &ShimConfig) -> Option<Called> {
    if let Some(slot) = config.names.get(name) {
        return Slot::parse(slot).map(|slot| Called { slot, side: None });
    }
    let suffixes = [
        ("rb-cc-rucc", Slot::Cc, Some(Side::R)),
        ("rb-cc-ref", Slot::Cc, Some(Side::G)),
        ("rb-cc", Slot::Cc, None),
        ("rb-c++", Slot::Cxx, None),
        ("rb-cpp", Slot::Cpp, None),
        ("rb-as", Slot::As, None),
    ];
    suffixes
        .iter()
        .find(|(suffix, _, _)| name.ends_with(suffix))
        .map(|&(_, slot, side)| Called { slot, side })
}

/// What a call compiles, from its inputs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// C.
    C,
    /// Assembly, with or without the preprocessor.
    Asm,
    /// C++.
    Cxx,
    /// No source: a link of objects, or a question.
    None,
}

/// What the call compiles.
#[must_use]
pub fn kind(invocation: &Invocation) -> Kind {
    if invocation.compiles_cxx() {
        Kind::Cxx
    } else if invocation.assembles() {
        Kind::Asm
    } else if invocation.compiles_c() {
        Kind::C
    } else {
        Kind::None
    }
}

/// A decided call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decision {
    /// The compiler to run.
    pub compiler: PathBuf,
    /// Its arguments.
    pub args: Vec<String>,
    /// The arguments the shim added, for the record.
    pub added: Vec<String>,
    /// The command line, read, for the record.
    pub invocation: Invocation,
    /// The origin of the object the call makes, when it makes one.
    pub origin: Option<&'static str>,
    /// The bring-up class, when the call is delegated.
    pub delegated: Option<String>,
    /// Whether rucc runs, which is when a trace can be asked for.
    pub rucc: bool,
}

/// Decide a call. `rest` is the command line without the program name.
///
/// # Errors
///
/// When the slot needs a side and none is known, or when the compiler it needs is not configured.
pub fn decide(
    called: Called,
    config: &ShimConfig,
    rest: &[String],
    cwd: &Path,
    exists: &dyn Fn(&Path) -> bool,
) -> Result<Decision, String> {
    let side = called.side.or(config.side);
    match called.slot {
        Slot::Cc => decide_cc(side, config, rest, cwd, exists),
        Slot::Cxx => {
            let invocation = args::read(rest, cwd, exists);
            let origin = makes_object(&invocation).then_some("ref-cxx");
            Ok(Decision {
                compiler: need(&config.ref_cxx, "ref-cxx", "RB_REF_CXX")?,
                args: rest.to_vec(),
                added: Vec::new(),
                invocation,
                origin,
                delegated: None,
                rucc: false,
            })
        }
        Slot::Cpp => decide_cpp(side, config, rest, cwd, exists),
        Slot::As => {
            let invocation = args::read(rest, cwd, exists);
            let origin = (!invocation.inputs.is_empty()).then_some("other");
            Ok(Decision {
                compiler: need(&config.ref_as, "ref-as", "RB_REF_AS")?,
                args: rest.to_vec(),
                added: Vec::new(),
                invocation,
                origin,
                delegated: None,
                rucc: false,
            })
        }
    }
}

fn decide_cc(
    side: Option<Side>,
    config: &ShimConfig,
    rest: &[String],
    cwd: &Path,
    exists: &dyn Fn(&Path) -> bool,
) -> Result<Decision, String> {
    let invocation = args::read(rest, cwd, exists);
    let kind = kind(&invocation);
    let object = makes_object(&invocation);
    let reference = |compiler: &PathBuf, key, var, origin, delegated: Option<&str>| {
        Ok(Decision {
            compiler: need(compiler, key, var)?,
            args: rest.to_vec(),
            added: Vec::new(),
            invocation: invocation.clone(),
            origin: object.then_some(origin),
            delegated: delegated.map(String::from),
            rucc: false,
        })
    };
    if kind == Kind::Cxx {
        return reference(&config.ref_cxx, "ref-cxx", "RB_REF_CXX", "cxx-on-cc", None);
    }
    let side = side.ok_or("the C compiler slot needs a side: name it -rucc or -ref, or set side in rb-shim.toml or RB_SIDE")?;
    if side == Side::G {
        return reference(&config.ref_cc, "ref-cc", "RB_REF_CC", "ref", None);
    }
    if kind == Kind::Asm && config.delegates("as") {
        return reference(&config.ref_cc, "ref-cc", "RB_REF_CC", "ref-c", Some("as"));
    }
    if kind == Kind::C && config.delegates("cxx-c") && named_unit(&invocation, config) {
        return reference(
            &config.ref_cc,
            "ref-cc",
            "RB_REF_CC",
            "ref-c",
            Some("cxx-c"),
        );
    }
    let added = rucc_added(config, rest);
    let origin = match kind {
        Kind::C => object.then_some("rucc-c"),
        Kind::Asm => object.then_some("rucc-asm"),
        Kind::Cxx | Kind::None => None,
    };
    Ok(Decision {
        compiler: need(&config.rucc, "rucc", "RB_RUCC")?,
        args: added.iter().chain(rest).cloned().collect(),
        added,
        invocation,
        origin,
        delegated: None,
        rucc: true,
    })
}

fn decide_cpp(
    side: Option<Side>,
    config: &ShimConfig,
    rest: &[String],
    cwd: &Path,
    exists: &dyn Fn(&Path) -> bool,
) -> Result<Decision, String> {
    let operands = cpp_operands(rest);
    let mut normal = vec!["-E".to_string()];
    normal.extend(operands.iter().cloned());
    let invocation = args::read(&normal, cwd, exists);
    let rucc = config.rucc_cpp
        && side.ok_or("the preprocessor slot runs rucc, so it needs a side: set side in rb-shim.toml or RB_SIDE")?
            == Side::R;
    if !rucc {
        return Ok(Decision {
            compiler: need(&config.ref_cpp, "ref-cpp", "RB_REF_CPP")?,
            args: rest.to_vec(),
            added: Vec::new(),
            invocation,
            origin: None,
            delegated: None,
            rucc: false,
        });
    }
    let mut added = rucc_added(config, rest);
    added.push("-E".to_string());
    let mut passed = added.clone();
    passed.extend(operands);
    if invocation.inputs.is_empty() && !args::reads_stdin(&passed) {
        // cpp with no input file reads standard input; a compiler driver says "no input files".
        passed.push("-".to_string());
        added.push("-".to_string());
    }
    Ok(Decision {
        compiler: need(&config.rucc, "rucc", "RB_RUCC")?,
        args: passed,
        added,
        invocation,
        origin: None,
        delegated: None,
        rucc: true,
    })
}

/// The arguments rucc gets before the build's: the configured ones, and the sysroot when the call
/// names none.
fn rucc_added(config: &ShimConfig, rest: &[String]) -> Vec<String> {
    let mut added = config.rucc_args.clone();
    let names_sysroot = rest
        .iter()
        .any(|a| a == "--sysroot" || a.starts_with("--sysroot="));
    if let Some(sysroot) = config.rucc_sysroot.as_ref().filter(|_| !names_sysroot) {
        added.push(format!("--sysroot={sysroot}"));
    }
    added
}

/// Whether the call leaves an object behind, as opposed to preprocessed text or an answer.
fn makes_object(invocation: &Invocation) -> bool {
    matches!(invocation.mode, Mode::Compile | Mode::Link)
        && invocation
            .inputs
            .iter()
            .any(|i| args::is_c_source(i) || args::is_cxx_source(i) || args::is_asm_source(i))
        || invocation.mode == Mode::Compile && invocation.language.is_some()
}

/// Whether a C unit is one `cxx-c` hands to the reference.
fn named_unit(invocation: &Invocation, config: &ShimConfig) -> bool {
    invocation.inputs.iter().any(|input| {
        config
            .bringup_units
            .iter()
            .any(|unit| input == unit || input.ends_with(&format!("/{unit}")))
    })
}

/// The compiler for a slot, or the error that says how to name one.
fn need(path: &Path, key: &str, var: &str) -> Result<PathBuf, String> {
    if path.as_os_str().is_empty() {
        Err(format!(
            "no {key} to run: set {key} in rb-shim.toml or {var}"
        ))
    } else {
        Ok(path.to_path_buf())
    }
}

/// Options of `cpp` whose value is the next argument.
const CPP_TAKES_VALUE: &[&str] = &[
    "-o",
    "-I",
    "-D",
    "-U",
    "-A",
    "-x",
    "-include",
    "-imacros",
    "-isystem",
    "-iquote",
    "-idirafter",
    "-iprefix",
    "-iwithprefix",
    "-iwithprefixbefore",
    "-isysroot",
    "--sysroot",
    "-MF",
    "-MT",
    "-MQ",
];

/// A `cpp` command line in driver form.
///
/// `cpp [options] [infile [outfile]]` names its output as a second operand, which a compiler
/// driver would read as a second input. The second operand becomes `-o outfile`, and `-` as the
/// output, which means standard output, is dropped.
#[must_use]
pub fn cpp_operands(rest: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    let mut operands = 0;
    let mut output = None;
    let mut iter = rest.iter();
    while let Some(arg) = iter.next() {
        if CPP_TAKES_VALUE.contains(&arg.as_str()) {
            out.push(arg.clone());
            if let Some(value) = iter.next() {
                out.push(value.clone());
            }
            continue;
        }
        let operand = arg == "-" || !arg.starts_with('-');
        if operand {
            operands += 1;
            if operands == 2 {
                output = Some(arg.clone());
                continue;
            }
        }
        out.push(arg.clone());
    }
    match output.as_deref() {
        Some("-") | None => {}
        Some(file) => {
            out.push("-o".to_string());
            out.push(file.to_string());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn words(line: &str) -> Vec<String> {
        line.split_whitespace().map(String::from).collect()
    }

    fn config() -> ShimConfig {
        ShimConfig {
            rucc: "/opt/rucc".into(),
            rucc_args: words("--target=x86_64-unknown-freebsd15.1 -fgnuc-version=15.2.0"),
            ref_cc: "/usr/local/bin/x86_64-unknown-freebsd15.1-gcc15".into(),
            ref_cxx: "/usr/local/bin/x86_64-unknown-freebsd15.1-g++15".into(),
            ref_cpp: "/usr/local/bin/x86_64-unknown-freebsd15.1-cpp15".into(),
            ref_as: "/usr/local/x86_64-unknown-freebsd15.1/bin/as".into(),
            ..ShimConfig::default()
        }
    }

    fn decide_line(name: &str, config: &ShimConfig, line: &str) -> Result<Decision, String> {
        let called = resolve(name, config).expect("a known name");
        let exists = |p: &Path| {
            let name = p.to_string_lossy();
            [".c", ".S", ".s", ".cc", ".o", ".x", ".h"]
                .iter()
                .any(|s| name.ends_with(s))
        };
        decide(called, config, &words(line), Path::new("/b"), &exists)
    }

    #[test]
    fn names_say_the_slot_and_sometimes_the_side() {
        let config = ShimConfig {
            names: BTreeMap::from([
                ("x86_64--netbsd-gcc".to_string(), "cc".to_string()),
                ("x86_64--netbsd-cpp".to_string(), "cpp".to_string()),
            ]),
            ..ShimConfig::default()
        };
        let r = resolve("x86_64-unknown-freebsd15.1-rb-cc-rucc", &config).unwrap();
        assert_eq!((r.slot, r.side), (Slot::Cc, Some(Side::R)));
        let g = resolve("x86_64-unknown-freebsd15.1-rb-cc-ref", &config).unwrap();
        assert_eq!((g.slot, g.side), (Slot::Cc, Some(Side::G)));
        assert_eq!(resolve("rb-c++", &config).unwrap().slot, Slot::Cxx);
        assert_eq!(resolve("rb-cpp", &config).unwrap().slot, Slot::Cpp);
        assert_eq!(resolve("rb-as", &config).unwrap().slot, Slot::As);
        assert_eq!(resolve("rb-cc", &config).unwrap().side, None);
        assert_eq!(
            resolve("x86_64--netbsd-cpp", &config).unwrap().slot,
            Slot::Cpp
        );
        assert_eq!(
            resolve("x86_64--netbsd-gcc", &config).unwrap().slot,
            Slot::Cc
        );
        assert!(resolve("gcc", &config).is_none());
    }

    #[test]
    fn r_compiles_c_and_assembly_with_rucc_and_g_with_the_reference() {
        let config = config();
        let r = decide_line("x-rb-cc-rucc", &config, "-O2 -c foo.c -o foo.o").unwrap();
        assert_eq!(r.compiler, Path::new("/opt/rucc"));
        assert_eq!(r.args[..2], config.rucc_args[..]);
        assert_eq!(r.args[2..], words("-O2 -c foo.c -o foo.o"));
        assert_eq!(r.origin, Some("rucc-c"));
        assert!(r.rucc);
        let asm = decide_line("x-rb-cc-rucc", &config, "-c locore.S").unwrap();
        assert_eq!(asm.origin, Some("rucc-asm"));
        let g = decide_line("x-rb-cc-ref", &config, "-O2 -c foo.c -o foo.o").unwrap();
        assert_eq!(g.compiler, config.ref_cc);
        assert_eq!(g.args, words("-O2 -c foo.c -o foo.o"));
        assert!(g.added.is_empty());
        assert_eq!(g.origin, Some("ref"));
    }

    #[test]
    fn questions_and_preprocessing_make_no_object() {
        let config = config();
        let version = decide_line("x-rb-cc-rucc", &config, "--version").unwrap();
        assert_eq!(version.origin, None);
        assert!(version.rucc);
        let pre = decide_line("x-rb-cc-rucc", &config, "-E -P foo.c -o foo.i").unwrap();
        assert_eq!(pre.origin, None);
        let link = decide_line("x-rb-cc-ref", &config, "-o prog a.o b.o").unwrap();
        assert_eq!(link.origin, None);
    }

    #[test]
    fn cxx_goes_to_the_reference_on_both_sides() {
        let config = config();
        let on_cc = decide_line("x-rb-cc-rucc", &config, "-c dtc.cc").unwrap();
        assert_eq!(on_cc.compiler, config.ref_cxx);
        assert_eq!(on_cc.origin, Some("cxx-on-cc"));
        let typed = decide_line("x-rb-cc-rucc", &config, "-x c++ -c gen.x").unwrap();
        assert_eq!(typed.origin, Some("cxx-on-cc"));
        let cxx = decide_line("rb-c++", &config, "-c dtc.cc -o dtc.o").unwrap();
        assert_eq!(cxx.compiler, config.ref_cxx);
        assert_eq!(cxx.origin, Some("ref-cxx"));
    }

    #[test]
    fn bringup_hands_assembly_and_named_units_to_the_reference() {
        let config = ShimConfig {
            bringup: words("as cxx-c"),
            bringup_units: vec!["lib/libunwind/unwind.c".to_string()],
            ..config()
        };
        let asm = decide_line("x-rb-cc-rucc", &config, "-c locore.S").unwrap();
        assert_eq!(asm.compiler, config.ref_cc);
        assert_eq!(asm.delegated.as_deref(), Some("as"));
        assert_eq!(asm.origin, Some("ref-c"));
        let named = decide_line("x-rb-cc-rucc", &config, "-c /src/lib/libunwind/unwind.c").unwrap();
        assert_eq!(named.delegated.as_deref(), Some("cxx-c"));
        let other = decide_line("x-rb-cc-rucc", &config, "-c /src/lib/libc/x.c").unwrap();
        assert_eq!(other.delegated, None);
        assert_eq!(other.compiler, config.rucc);
    }

    #[test]
    fn the_sysroot_is_added_only_when_the_call_names_none() {
        let config = ShimConfig {
            rucc_sysroot: Some("/obj/destdir.amd64".to_string()),
            ..config()
        };
        let plain = decide_line("x-rb-cc-rucc", &config, "-c foo.c").unwrap();
        assert!(
            plain
                .added
                .contains(&"--sysroot=/obj/destdir.amd64".to_string())
        );
        let named = decide_line("x-rb-cc-rucc", &config, "--sysroot=/d -c foo.c").unwrap();
        assert!(!named.added.iter().any(|a| a.starts_with("--sysroot")));
    }

    #[test]
    fn a_side_and_a_compiler_are_needed() {
        let config = config();
        let err = decide_line("rb-cc", &config, "-c foo.c").unwrap_err();
        assert!(err.contains("side"), "{err}");
        let empty = ShimConfig {
            side: Some(Side::G),
            ..ShimConfig::default()
        };
        let err = decide_line("rb-cc", &empty, "-c foo.c").unwrap_err();
        assert!(err.contains("RB_REF_CC"), "{err}");
    }

    #[test]
    fn cpp_operands_become_a_driver_command_line() {
        assert_eq!(
            cpp_operands(&words("-P -I inc in.x out.y")),
            words("-P -I inc in.x -o out.y")
        );
        assert_eq!(cpp_operands(&words("-DX in.x -")), words("-DX in.x"));
        assert_eq!(cpp_operands(&words("-D X in.x")), words("-D X in.x"));
    }

    #[test]
    fn cpp_runs_rucc_in_r_only_where_configured() {
        let mut config = config();
        let reference = decide_line("rb-cpp", &config, "-P in.x out.y").unwrap();
        assert_eq!(reference.compiler, config.ref_cpp);
        assert_eq!(reference.args, words("-P in.x out.y"));
        assert_eq!(reference.invocation.outputs, ["out.y"]);
        config.rucc_cpp = true;
        config.side = Some(Side::R);
        let rucc = decide_line("rb-cpp", &config, "-P in.x out.y").unwrap();
        assert_eq!(rucc.compiler, config.rucc);
        assert!(rucc.args.ends_with(&words("-E -P in.x -o out.y")));
        let stdin = decide_line("rb-cpp", &config, "-P").unwrap();
        assert_eq!(stdin.args.last().map(String::as_str), Some("-"));
        config.side = Some(Side::G);
        let g = decide_line("rb-cpp", &config, "-P in.x").unwrap();
        assert_eq!(g.compiler, config.ref_cpp);
    }

    #[test]
    fn a_direct_assembler_call_is_recorded_as_other() {
        let config = config();
        let call = decide_line("rb-as", &config, "-o boot.o boot.s").unwrap();
        assert_eq!(call.compiler, config.ref_as);
        assert_eq!(call.origin, Some("other"));
        assert_eq!(call.invocation.outputs, ["boot.o"]);
    }
}
