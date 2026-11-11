//! `rb shim`: put `rb-cc` and its other names in a directory, with the `rb-shim.toml` that tells
//! them which compilers to run (plan 05.4 and 14.4).
//!
//! The settings come from the repository's files, never from a person typing them:
//! - rucc's `--target` and `-fgnuc-version=` come from the pin's persona in `personas.toml`;
//! - the reference compilers come from the persona's toolchain in `toolchains.toml`, or, for
//!   NetBSD, from the `TOOLDIR` that `build.sh tools` made;
//! - rucc is `--rucc PATH`, else `RB_RUCC`, else the command of `[rucc]` in `toolchains.toml`.
//!
//! FreeBSD's names carry the side, so one directory serves R and G: `XCC` names
//! `x86_64-unknown-freebsd15.1-rb-cc-rucc` in R and `...-rb-cc-ref` in G. NetBSD's names are fixed
//! by `TOOLDIR`, so the side goes in `rb-shim.toml`, and R and G each get a directory.

use harness_shim::bringup::split_list;
use rb_pins::pins::Pin;
use rb_pins::places::Row;
use rb_pins::toolchains::{Personas, RuccPin, Toolchain, Toolchains};
use rb_shim::config::{CLASSES, FILE_NAME, ShimConfig, Side};
use std::ffi::OsString;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Which compiler a build's C compiler slot runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cc {
    /// rucc: the R build.
    Rucc,
    /// The reference GCC: the G build.
    Ref,
}

impl Cc {
    /// Read `rucc` or `ref`.
    pub fn parse(text: &str) -> Result<Self, String> {
        match text {
            "rucc" | "r" | "R" => Ok(Self::Rucc),
            "ref" | "g" | "G" => Ok(Self::Ref),
            other => Err(format!("--cc {other}: rucc or ref")),
        }
    }

    /// The name, as on the command line.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Rucc => "rucc",
            Self::Ref => "ref",
        }
    }

    /// The side of the build.
    #[must_use]
    pub fn side(self) -> Side {
        match self {
            Self::Rucc => Side::R,
            Self::Ref => Side::G,
        }
    }
}

/// One rucc binary, as a build records it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rucc {
    /// The binary.
    pub path: PathBuf,
    /// Its SHA-256.
    pub sha256: String,
    /// The first line of its `--version`.
    pub version: String,
    /// The host triple of the pinned release whose binary this is, or `None` when it is not one.
    pub pinned: Option<String>,
}

impl Rucc {
    /// Find rucc: `flag`, else `env`, else the pin's command. A command with no slash is looked
    /// up on `path`.
    pub fn find(
        flag: Option<&str>,
        env: Option<&str>,
        pin: Option<&RuccPin>,
        path: Option<OsString>,
    ) -> Result<PathBuf, String> {
        let (named, from) = match (flag, env.filter(|v| !v.is_empty()), pin) {
            (Some(f), _, _) => (f.to_string(), "--rucc"),
            (None, Some(e), _) => (e.to_string(), "RB_RUCC"),
            (None, None, Some(p)) => (p.command.clone(), "the [rucc] command of toolchains.toml"),
            (None, None, None) => {
                return Err(
                    "no rucc: give --rucc PATH, set RB_RUCC, or add [rucc] to toolchains.toml"
                        .into(),
                );
            }
        };
        if named.contains('/') {
            let file = PathBuf::from(&named);
            return if file.is_file() {
                Ok(absolute(&file))
            } else {
                Err(format!("{from} names {named}, which is not a file"))
            };
        }
        path.and_then(|p| {
            std::env::split_paths(&p)
                .map(|d| d.join(&named))
                .find(|f| f.is_file())
        })
        .map(|f| absolute(&f))
        .ok_or_else(|| format!("{from} names {named}, which is not on PATH"))
    }

    /// Read a binary's digest and version, and match it against the pin.
    pub fn describe(path: &Path, pin: Option<&RuccPin>) -> Result<Self, String> {
        let sha256 = rb_pins::fetch::sha256(path)?;
        let out = Command::new(path)
            .arg("--version")
            .output()
            .map_err(|e| format!("{}: {e}", path.display()))?;
        let text = String::from_utf8_lossy(&out.stdout);
        let version = text.lines().next().unwrap_or_default().trim().to_string();
        if !out.status.success() || version.is_empty() {
            return Err(format!(
                "{} --version failed: {}",
                path.display(),
                String::from_utf8_lossy(&out.stderr).trim()
            ));
        }
        let pinned = pin.and_then(|p| p.host_of(&sha256)).map(String::from);
        Ok(Self {
            path: path.to_path_buf(),
            sha256,
            version,
            pinned,
        })
    }

    /// One line for a report.
    #[must_use]
    pub fn line(&self, pin: Option<&RuccPin>) -> String {
        let release = match (&self.pinned, pin) {
            (Some(host), Some(p)) => format!("the pinned {} for {host}", p.version),
            (None, Some(p)) => format!("not the pinned {}", p.version),
            _ => "no pin to compare with".to_string(),
        };
        format!(
            "{}: {}, sha256 {}, {release}",
            self.path.display(),
            self.version,
            self.sha256
        )
    }
}

fn absolute(path: &Path) -> PathBuf {
    std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf())
}

/// What `rb shim` needs to know beyond the repository's files.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Ask {
    /// rucc, when the directory can run it.
    pub rucc: Option<PathBuf>,
    /// The `compile.jsonl` the shims append to. Empty means none.
    pub log: PathBuf,
    /// For NetBSD: the `TOOLDIR` that `build.sh tools` made, whose compilers are the reference.
    pub tools: Option<PathBuf>,
    /// For NetBSD: the `DESTDIR`, which rucc gets as its sysroot.
    pub destdir: Option<PathBuf>,
    /// For NetBSD: the side, since the names do not say it.
    pub side: Option<Side>,
    /// The bring-up classes of 00, which only R uses. Any makes the run ungraded.
    pub bringup: Vec<String>,
    /// The C units the `cxx-c` class hands to the reference.
    pub bringup_units: Vec<String>,
}

/// The bring-up classes and units of a command: `--bringup` and `--bringup-units`, else
/// `RB_BRINGUP` and `RB_BRINGUP_UNITS`.
///
/// # Errors
///
/// When a class is not one of 00's two, when `cxx-c` names no unit, or when units are named
/// without `cxx-c`.
pub fn bringup(
    flag: Option<&str>,
    units: Option<&str>,
    env: &dyn Fn(&str) -> Option<String>,
) -> Result<(Vec<String>, Vec<String>), String> {
    let classes = flag
        .map(String::from)
        .or_else(|| env("RB_BRINGUP"))
        .map(|v| split_list(&v))
        .unwrap_or_default();
    let units = units
        .map(String::from)
        .or_else(|| env("RB_BRINGUP_UNITS"))
        .map(|v| split_list(&v))
        .unwrap_or_default();
    CLASSES.check(&classes)?;
    let cxx_c = classes.iter().any(|c| c == "cxx-c");
    if cxx_c && units.is_empty() {
        return Err("the cxx-c class needs the units it hands to the reference: give --bringup-units or RB_BRINGUP_UNITS".into());
    }
    if !cxx_c && !units.is_empty() {
        return Err("bring-up units are named, but the cxx-c class is not on".into());
    }
    Ok((classes, units))
}

/// The shims of one pin and row: the names and the settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shims {
    /// `freebsd` or `netbsd`.
    pub system: String,
    /// The names the shim binary gets in the directory.
    pub names: Vec<String>,
    /// The contents of `rb-shim.toml`.
    pub config: ShimConfig,
    /// rucc's `--target`.
    pub target: String,
    /// The persona, rucc's `-fgnuc-version=`.
    pub gnuc: String,
    /// The reference toolchain's name in `toolchains.toml`.
    pub reference: String,
}

/// Decide the shims of a pin and a row.
pub fn plan(
    pin: &Pin,
    row: &Row,
    personas: &Personas,
    toolchains: &Toolchains,
    ask: &Ask,
) -> Result<Shims, String> {
    let persona = personas
        .for_pin(pin)
        .ok_or_else(|| format!("personas.toml has no persona for {}", pin.name))?;
    let reference_name = persona.reference_for(row, toolchains);
    let reference = toolchains
        .toolchain
        .get(&reference_name)
        .ok_or_else(|| format!("toolchains.toml has no {reference_name}"))?;
    let arch = row.rucc_arch.as_str();
    let target = persona.target(pin, arch);
    let mut config = ShimConfig {
        log: ask.log.clone(),
        rucc: ask.rucc.clone().unwrap_or_default(),
        rucc_args: persona.rucc_args(pin, arch),
        ..ShimConfig::default()
    };
    // G runs the reference for every unit, so bring-up means nothing there.
    if ask.side != Some(Side::G) {
        config.bringup.clone_from(&ask.bringup);
        config.bringup_units.clone_from(&ask.bringup_units);
    }
    let names = match pin.system.as_str() {
        "freebsd" => freebsd(&mut config, reference, toolchains, &target)?,
        "netbsd" => netbsd(&mut config, reference, ask)?,
        other => return Err(format!("{}: no shims for the system {other}", pin.name)),
    };
    Ok(Shims {
        system: pin.system.clone(),
        names,
        config,
        target,
        gnuc: persona.gnuc.clone(),
        reference: reference_name,
    })
}

/// FreeBSD: the reference package's compilers, and names prefixed with its triple (05.4).
fn freebsd(
    config: &mut ShimConfig,
    reference: &Toolchain,
    toolchains: &Toolchains,
    target: &str,
) -> Result<Vec<String>, String> {
    let need = |v: &Option<String>, what: &str| {
        v.clone()
            .map(PathBuf::from)
            .ok_or_else(|| format!("the reference toolchain has no {what}"))
    };
    config.ref_cc = need(&reference.cc, "cc")?;
    config.ref_cxx = need(&reference.cxx, "cxx")?;
    config.ref_cpp = need(&reference.cpp, "cpp")?;
    let prefix = reference
        .cross_prefix
        .clone()
        .or_else(|| {
            reference.needs.iter().find_map(|n| {
                toolchains
                    .toolchain
                    .get(n)
                    .and_then(|t| t.cross_prefix.clone())
            })
        })
        .ok_or("the reference toolchain and what it needs have no cross_prefix")?;
    config.ref_as = PathBuf::from(format!("{prefix}as"));
    Ok(["rb-cc-rucc", "rb-cc-ref", "rb-c++", "rb-cpp", "rb-as"]
        .iter()
        .map(|n| format!("{target}-{n}"))
        .collect())
}

/// NetBSD: the compilers of the `TOOLDIR` that `build.sh tools` made, under the names `TOOLDIR`
/// gives them, and rucc in the preprocessor slot too (05.4).
fn netbsd(
    config: &mut ShimConfig,
    reference: &Toolchain,
    ask: &Ask,
) -> Result<Vec<String>, String> {
    let tools = ask.tools.as_ref().ok_or(
        "NetBSD's reference compilers are in the TOOLDIR of build.sh tools: give --tools DIR",
    )?;
    let cc = reference
        .tooldir_cc
        .as_deref()
        .ok_or("the reference toolchain has no tooldir_cc")?;
    let cpp = reference
        .tooldir_cpp
        .as_deref()
        .ok_or("the reference toolchain has no tooldir_cpp")?;
    let base = |p: &str| -> String {
        Path::new(p)
            .file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned())
    };
    let platform = base(cc)
        .strip_suffix("-gcc")
        .map(String::from)
        .ok_or_else(|| format!("tooldir_cc {cc} does not end in -gcc"))?;
    config.ref_cc = tools.join(cc);
    config.ref_cpp = tools.join(cpp);
    config.ref_cxx = tools.join(format!("bin/{platform}-c++"));
    config.ref_as = tools.join(format!("bin/{platform}-as"));
    config.side = Some(
        ask.side
            .ok_or("NetBSD's names do not say the side: give --cc rucc or ref")?,
    );
    config.rucc_cpp = true;
    config.rucc_sysroot = ask.destdir.as_ref().map(|d| d.display().to_string());
    config.names = [(base(cc), "cc"), (base(cpp), "cpp")]
        .into_iter()
        .map(|(n, s)| (n, s.to_string()))
        .collect();
    Ok(vec![base(cc), base(cpp)])
}

/// The `rb-cc` binary to install: `flag`, else `RB_SHIM`, else `rb-cc` next to `rb`.
pub fn shim_binary(flag: Option<&str>, env: Option<&str>) -> Result<PathBuf, String> {
    let path = match flag.or(env.filter(|v| !v.is_empty())) {
        Some(p) => PathBuf::from(p),
        None => std::env::current_exe()
            .map_err(|e| format!("where is rb: {e}"))?
            .with_file_name("rb-cc"),
    };
    if path.is_file() {
        Ok(path)
    } else {
        Err(format!(
            "{} is not a file: build rb-cc with cargo build -p rb-shim, or give --shim PATH",
            path.display()
        ))
    }
}

/// Put the shim binary in `dir` under every name, and write `rb-shim.toml` beside it. The first
/// name is a copy and the others are hard links to it, or copies where links do not work.
pub fn install(dir: &Path, binary: &Path, shims: &Shims) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let mut first: Option<PathBuf> = None;
    for name in &shims.names {
        let path = dir.join(name);
        if path.symlink_metadata().is_ok() {
            std::fs::remove_file(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        }
        let made = match &first {
            Some(f) => {
                std::fs::hard_link(f, &path).or_else(|_| std::fs::copy(f, &path).map(|_| ()))
            }
            None => std::fs::copy(binary, &path).map(|_| ()),
        };
        made.map_err(|e| format!("{}: {e}", path.display()))?;
        first.get_or_insert(path);
    }
    let file = dir.join(FILE_NAME);
    std::fs::write(&file, shims.config.to_toml()).map_err(|e| format!("{}: {e}", file.display()))
}

/// The report of `rb shim`.
#[must_use]
pub fn report(dir: &Path, shims: &Shims, rucc: Option<&Rucc>, pin: Option<&RuccPin>) -> String {
    let mut s = String::new();
    let _ = writeln!(s, "shims in {}:", dir.display());
    for name in &shims.names {
        let _ = writeln!(s, "  {name}");
    }
    let _ = writeln!(
        s,
        "persona: --target={} -fgnuc-version={} (reference {})",
        shims.target, shims.gnuc, shims.reference
    );
    if let Some(side) = shims.config.side {
        let _ = writeln!(s, "side: {}", side.letter());
    }
    if !shims.config.bringup.is_empty() {
        let _ = writeln!(
            s,
            "bring-up: {}, so every run with these shims is ungraded",
            shims.config.bringup.join(", ")
        );
        if !shims.config.bringup_units.is_empty() {
            let _ = writeln!(
                s,
                "units cxx-c hands to the reference: {}",
                shims.config.bringup_units.join(", ")
            );
        }
    }
    match rucc {
        Some(r) => {
            let _ = writeln!(s, "rucc: {}", r.line(pin));
        }
        None => {
            let _ = writeln!(s, "rucc: none, so only the reference names work");
        }
    }
    let _ = writeln!(
        s,
        "log: {}",
        if shims.config.log.as_os_str().is_empty() {
            "none".to_string()
        } else {
            shims.config.log.display().to_string()
        }
    );
    s
}

/// `rb shim` on the command line.
pub mod cli {
    use super::{Ask, Cc, Rucc, bringup, install, plan, report, shim_binary};
    use crate::cli::Args;
    use rb_pins::pins::PinsFile;
    use rb_pins::places::Rows;
    use rb_pins::toolchains::{Personas, Toolchains};
    use std::path::{Path, PathBuf};

    /// Run `rb shim`.
    pub fn run(args: &Args, root: &Path) -> Result<String, String> {
        let name = args
            .target
            .as_deref()
            .ok_or("name the pin, such as netbsd-trunk")?;
        let dir = PathBuf::from(args.get("dir").ok_or("give --dir DIR")?);
        let pins = PinsFile::load(root)?.ok_or("no pins.toml yet")?;
        let pin = pins
            .find(name)
            .ok_or_else(|| format!("pins.toml has no pin {name}"))?;
        let rows = Rows::load(root)?;
        let row_name = args.get("row").unwrap_or("amd64");
        let row = rows
            .row
            .get(row_name)
            .ok_or_else(|| format!("rows.toml has no row {row_name}"))?;
        let personas = Personas::load(root)?;
        let toolchains = Toolchains::load(root)?;
        let cc = args.get("cc").map(Cc::parse).transpose()?;
        let found = Rucc::find(
            args.get("rucc"),
            std::env::var("RB_RUCC").ok().as_deref(),
            toolchains.rucc.as_ref(),
            std::env::var_os("PATH"),
        );
        let rucc = match (found, cc) {
            (Ok(path), _) => Some(Rucc::describe(&path, toolchains.rucc.as_ref())?),
            (Err(e), Some(Cc::Rucc)) => return Err(e),
            // A G directory, or a FreeBSD one without rucc, still works for the reference names.
            (Err(_), _) => None,
        };
        let (classes, units) = bringup(args.get("bringup"), args.get("bringup-units"), &|k| {
            std::env::var(k).ok()
        })?;
        let ask = Ask {
            rucc: rucc.as_ref().map(|r| r.path.clone()),
            log: args.get("log").map(PathBuf::from).unwrap_or_default(),
            tools: args.get("tools").map(PathBuf::from),
            destdir: args.get("destdir").map(PathBuf::from),
            side: cc.map(Cc::side),
            bringup: classes,
            bringup_units: units,
        };
        let shims = plan(pin, row, &personas, &toolchains, &ask)?;
        let binary = shim_binary(args.get("shim"), std::env::var("RB_SHIM").ok().as_deref())?;
        install(&dir, &binary, &shims)?;
        Ok(report(
            &dir,
            &shims,
            rucc.as_ref(),
            toolchains.rucc.as_ref(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rb_pins::places::Rows;

    fn files() -> (Personas, Toolchains, Rows) {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        (
            Personas::load(&root).unwrap(),
            Toolchains::load(&root).unwrap(),
            Rows::load(&root).unwrap(),
        )
    }

    fn pin(system: &str, series: &str, version: &str) -> Pin {
        Pin {
            name: format!("{system}-{series}"),
            system: system.into(),
            series: series.into(),
            version: version.into(),
            ..Pin::default()
        }
    }

    #[test]
    fn freebsd_names_carry_the_triple_and_the_side() {
        let (personas, toolchains, rows) = files();
        let ask = Ask {
            rucc: Some("/opt/rucc/rucc".into()),
            ..Ask::default()
        };
        let shims = plan(
            &pin("freebsd", "15", "15.1-RELEASE"),
            &rows.row["amd64"],
            &personas,
            &toolchains,
            &ask,
        )
        .unwrap();
        assert_eq!(shims.names[0], "x86_64-unknown-freebsd15.1-rb-cc-rucc");
        assert_eq!(shims.names[1], "x86_64-unknown-freebsd15.1-rb-cc-ref");
        assert_eq!(
            shims.config.rucc_args,
            [
                "--target=x86_64-unknown-freebsd15.1",
                "-fgnuc-version=15.2.0"
            ]
        );
        assert_eq!(
            shims.config.ref_cc,
            Path::new("/usr/local/bin/x86_64-unknown-freebsd15.1-gcc15")
        );
        assert_eq!(
            shims.config.ref_as,
            Path::new("/usr/local/x86_64-unknown-freebsd15.1/bin/as")
        );
        assert_eq!(shims.config.side, None);
        assert!(!shims.config.rucc_cpp);
    }

    #[test]
    fn freebsd_rows_take_the_reference_of_their_target_arch() {
        let (personas, toolchains, rows) = files();
        for (row, triple) in [
            ("aarch64", "aarch64-unknown-freebsd15.1"),
            ("riscv64", "riscv64-unknown-freebsd15.1"),
        ] {
            let shims = plan(
                &pin("freebsd", "15", "15.1-RELEASE"),
                &rows.row[row],
                &personas,
                &toolchains,
                &Ask::default(),
            )
            .unwrap();
            assert_eq!(shims.reference, format!("{row}-gcc15"));
            assert_eq!(shims.names[0], format!("{triple}-rb-cc-rucc"));
            assert_eq!(shims.config.rucc_args[0], format!("--target={triple}"));
            assert_eq!(
                shims.config.ref_cc,
                PathBuf::from(format!("/usr/local/bin/{triple}-gcc15"))
            );
            assert_eq!(
                shims.config.ref_as,
                PathBuf::from(format!("/usr/local/{triple}/bin/as"))
            );
        }
    }

    #[test]
    fn netbsd_takes_the_tooldir_names_and_needs_a_side() {
        let (personas, toolchains, rows) = files();
        let mut ask = Ask {
            rucc: Some("/opt/rucc/rucc".into()),
            tools: Some("/nb/tools".into()),
            destdir: Some("/nb/destdir".into()),
            ..Ask::default()
        };
        let trunk = pin("netbsd", "current", "11.99.10");
        let row = &rows.row["amd64"];
        let err = plan(&trunk, row, &personas, &toolchains, &ask).unwrap_err();
        assert!(err.contains("side"), "{err}");
        ask.side = Some(Side::R);
        let shims = plan(&trunk, row, &personas, &toolchains, &ask).unwrap();
        assert_eq!(shims.names, ["x86_64--netbsd-gcc", "x86_64--netbsd-cpp"]);
        assert_eq!(
            shims.config.rucc_args,
            [
                "--target=x86_64-unknown-netbsd11.99",
                "-fgnuc-version=14.4.0"
            ]
        );
        assert_eq!(
            shims.config.ref_cc,
            Path::new("/nb/tools/bin/x86_64--netbsd-gcc")
        );
        assert_eq!(
            shims.config.ref_cxx,
            Path::new("/nb/tools/bin/x86_64--netbsd-c++")
        );
        assert_eq!(shims.config.rucc_sysroot.as_deref(), Some("/nb/destdir"));
        assert_eq!(shims.config.names["x86_64--netbsd-cpp"], "cpp");
        assert!(shims.config.rucc_cpp);
        ask.tools = None;
        let err = plan(&trunk, row, &personas, &toolchains, &ask).unwrap_err();
        assert!(err.contains("--tools"), "{err}");
    }

    #[test]
    fn rucc_is_the_flag_then_the_environment_then_the_pin() {
        let dir = std::env::temp_dir().join(format!("rb-shims-find-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("rucc");
        std::fs::write(&file, "").unwrap();
        let pin = RuccPin {
            command: "rucc".into(),
            ..RuccPin::default()
        };
        let path = Some(dir.clone().into_os_string());
        let flag = file.display().to_string();
        assert_eq!(
            Rucc::find(Some(&flag), Some("/nope"), Some(&pin), None).unwrap(),
            file
        );
        let env = Rucc::find(None, Some("/no/such/rucc"), Some(&pin), path.clone()).unwrap_err();
        assert!(env.contains("RB_RUCC"), "{env}");
        assert_eq!(Rucc::find(None, None, Some(&pin), path).unwrap(), file);
        let none = Rucc::find(None, None, Some(&pin), None).unwrap_err();
        assert!(none.contains("not on PATH"), "{none}");
        assert!(Rucc::find(None, Some(""), None, None).is_err());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn cc_reads_both_spellings() {
        assert_eq!(Cc::parse("rucc").unwrap(), Cc::Rucc);
        assert_eq!(Cc::parse("G").unwrap().side(), Side::G);
        assert!(Cc::parse("clang").is_err());
    }
    #[test]
    fn bring_up_comes_from_the_flags_or_the_environment_and_only_r_gets_it() {
        let none = |_: &str| None;
        assert_eq!(bringup(None, None, &none).unwrap(), (vec![], vec![]));
        let env = |k: &str| (k == "RB_BRINGUP").then(|| "as".to_string());
        assert_eq!(bringup(None, None, &env).unwrap().0, ["as"]);
        let (classes, units) = bringup(Some("as,cxx-c"), Some("lib/a.c"), &env).unwrap();
        assert_eq!((classes.len(), units), (2, vec!["lib/a.c".to_string()]));
        assert!(bringup(Some("m16"), None, &none).is_err());
        assert!(bringup(Some("cxx-c"), None, &none).is_err());
        assert!(bringup(Some("as"), Some("lib/a.c"), &none).is_err());

        let (personas, toolchains, rows) = files();
        let mut ask = Ask {
            rucc: Some("/opt/rucc/rucc".into()),
            tools: Some("/nb/tools".into()),
            side: Some(Side::R),
            bringup: vec!["as".into()],
            ..Ask::default()
        };
        let trunk = pin("netbsd", "current", "11.99.10");
        let row = &rows.row["amd64"];
        let r = plan(&trunk, row, &personas, &toolchains, &ask).unwrap();
        assert_eq!(r.config.bringup, ["as"]);
        assert!(report(Path::new("/d"), &r, None, None).contains("ungraded"));
        ask.side = Some(Side::G);
        let g = plan(&trunk, row, &personas, &toolchains, &ask).unwrap();
        assert!(g.config.bringup.is_empty());
    }
}
