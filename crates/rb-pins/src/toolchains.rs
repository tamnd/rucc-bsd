//! `toolchains.toml` and `personas.toml`: the reference that grades each tree and the persona
//! rucc wears for it (plan 04.3).

use crate::pins::Pin;
use crate::places::Row;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::Path;

/// The file of reference toolchains.
pub const TOOLCHAINS: &str = "toolchains.toml";
/// The file of personas.
pub const PERSONAS: &str = "personas.toml";

/// The whole of `toolchains.toml`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Toolchains {
    /// The toolchains by name.
    pub toolchain: BTreeMap<String, Toolchain>,
    /// The rucc release that R builds with when no other rucc is named.
    pub rucc: Option<RuccPin>,
}

/// The pinned rucc release: where it comes from, and the digests of its tarballs and binaries.
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuccPin {
    /// The GitHub repository of the release.
    pub repo: String,
    /// The release tag, such as `v0.30.15`.
    pub version: String,
    /// The command `rb` runs when neither `--rucc` nor `RB_RUCC` names one, looked up on `PATH`
    /// when it has no slash.
    pub command: String,
    /// The SHA-256 of the release tarball, by host triple.
    #[serde(default)]
    pub tarball_sha256: BTreeMap<String, String>,
    /// The SHA-256 of the `rucc` binary in the tarball, by host triple.
    #[serde(default)]
    pub binary_sha256: BTreeMap<String, String>,
}

impl RuccPin {
    /// The release asset for a host triple.
    #[must_use]
    pub fn asset(&self, host: &str) -> String {
        format!("rucc-{}-{host}.tar.gz", self.version)
    }

    /// The host triple whose pinned binary has this digest, if any.
    #[must_use]
    pub fn host_of(&self, sha256: &str) -> Option<&str> {
        self.binary_sha256
            .iter()
            .find(|(_, d)| d.as_str() == sha256)
            .map(|(h, _)| h.as_str())
    }
}

/// One reference toolchain: a FreeBSD package, or the tools `build.sh` builds from a NetBSD tree.
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Toolchain {
    /// `freebsd` or `netbsd`.
    pub system: String,
    /// `pkg` or `build.sh tools`.
    pub kind: String,
    /// The package repository.
    pub repo: Option<String>,
    /// The package name.
    pub package: Option<String>,
    /// The package version.
    pub version: Option<String>,
    /// The port the package is built from.
    pub origin: Option<String>,
    /// The package file, relative to `repo`.
    pub repopath: Option<String>,
    /// The size of the package file in bytes.
    pub size: Option<u64>,
    /// The SHA-256 of the package file.
    pub sha256: Option<String>,
    /// The sum in the repository's catalogue.
    pub pkg_sum: Option<String>,
    /// Other toolchains this one needs.
    #[serde(default)]
    pub needs: Vec<String>,
    /// The GCC version.
    pub gcc: Option<String>,
    /// The binutils version.
    pub binutils: Option<String>,
    /// The C compiler.
    pub cc: Option<String>,
    /// The C++ compiler.
    pub cxx: Option<String>,
    /// The preprocessor.
    pub cpp: Option<String>,
    /// The value of FreeBSD's `CROSS_TOOLCHAIN` that selects this compiler.
    pub cross_toolchain: Option<String>,
    /// FreeBSD's `CROSS_BINUTILS_PREFIX`.
    pub cross_prefix: Option<String>,
    /// The C compiler in a NetBSD `TOOLDIR`.
    pub tooldir_cc: Option<String>,
    /// The preprocessor in a NetBSD `TOOLDIR`.
    pub tooldir_cpp: Option<String>,
}

/// The whole of `personas.toml`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Personas {
    /// The personas, matched to pins by system and series.
    pub persona: Vec<Persona>,
}

/// One persona.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Persona {
    /// `freebsd` or `netbsd`.
    pub system: String,
    /// The pin series it covers.
    pub series: Vec<String>,
    /// The toolchain in `toolchains.toml` that builds G.
    pub reference: String,
    /// rucc's `-fgnuc-version=`.
    pub gnuc: String,
    /// The toolchain used where `reference` fails and this one does not.
    pub fallback: Option<String>,
    /// The persona when `fallback` built G.
    pub fallback_gnuc: Option<String>,
    /// The OS part of rucc's `--target`. `{version}` is the pin's version to two places.
    pub target_os: String,
    /// The reason for the choice.
    pub why: String,
}

impl Toolchains {
    /// Parse the text of `toolchains.toml` and check that every `needs` names a toolchain.
    pub fn parse(text: &str) -> Result<Self, String> {
        let file: Self = toml::from_str(text).map_err(|e| format!("{TOOLCHAINS}: {e}"))?;
        for (name, t) in &file.toolchain {
            for need in &t.needs {
                if !file.toolchain.contains_key(need) {
                    return Err(format!(
                        "{TOOLCHAINS}: {name} needs {need}, which is not listed"
                    ));
                }
            }
            if t.kind == "pkg" && (t.sha256.is_none() || t.repopath.is_none()) {
                return Err(format!(
                    "{TOOLCHAINS}: the package {name} has no sha256 or repopath"
                ));
            }
        }
        Ok(file)
    }

    /// Read `toolchains.toml` in `root`.
    pub fn load(root: &Path) -> Result<Self, String> {
        Self::parse(&read(root, TOOLCHAINS)?)
    }
}

impl Personas {
    /// Parse the text of `personas.toml`.
    pub fn parse(text: &str) -> Result<Self, String> {
        toml::from_str(text).map_err(|e| format!("{PERSONAS}: {e}"))
    }

    /// Read `personas.toml` in `root`.
    pub fn load(root: &Path) -> Result<Self, String> {
        Self::parse(&read(root, PERSONAS)?)
    }

    /// The persona for a pin.
    #[must_use]
    pub fn for_pin(&self, pin: &Pin) -> Option<&Persona> {
        self.persona
            .iter()
            .find(|p| p.system == pin.system && p.series.contains(&pin.series))
    }

    /// Make the persona of a pin its fallback, for a G built by the fallback toolchain where the
    /// reference fails (plan 04.3): the fallback builds G, and rucc claims its version. An error
    /// when the persona has no fallback with its version.
    pub fn fall_back(&mut self, pin: &Pin) -> Result<(), String> {
        let persona = self
            .persona
            .iter_mut()
            .find(|p| p.system == pin.system && p.series.contains(&pin.series))
            .ok_or_else(|| format!("{PERSONAS} has no persona for {}", pin.name))?;
        let (Some(reference), Some(gnuc)) = (&persona.fallback, &persona.fallback_gnuc) else {
            return Err(format!(
                "the persona of {} in {PERSONAS} has no fallback and fallback_gnuc",
                pin.name
            ));
        };
        persona.reference = reference.clone();
        persona.gnuc = gnuc.clone();
        persona.fallback = None;
        persona.fallback_gnuc = None;
        Ok(())
    }
}

impl Persona {
    /// rucc's `--target` for a pin and an architecture, such as `x86_64-unknown-netbsd11.99`.
    #[must_use]
    pub fn target(&self, pin: &Pin, arch: &str) -> String {
        format!(
            "{arch}-unknown-{}",
            self.target_os
                .replace("{version}", &two_places(&pin.version))
        )
    }

    /// The reference toolchain of a row. A FreeBSD persona names the amd64 flavor of a port, such
    /// as `amd64-gcc15`. A row with another `TARGET_ARCH` takes the flavor of that `TARGET_ARCH`,
    /// such as `aarch64-gcc15`, when `toolchains.toml` lists it.
    #[must_use]
    pub fn reference_for(&self, row: &Row, toolchains: &Toolchains) -> String {
        let flavor = row
            .freebsd
            .target_arch
            .as_deref()
            .filter(|_| self.system == "freebsd")
            .and_then(|arch| {
                let port = self.reference.strip_prefix("amd64-")?;
                Some(format!("{arch}-{port}"))
            });
        match flavor {
            Some(name) if toolchains.toolchain.contains_key(&name) => name,
            _ => self.reference.clone(),
        }
    }

    /// The arguments rucc gets in front of the build's: the target and the persona (05.4).
    #[must_use]
    pub fn rucc_args(&self, pin: &Pin, arch: &str) -> Vec<String> {
        vec![
            format!("--target={}", self.target(pin, arch)),
            format!("-fgnuc-version={}", self.gnuc),
        ]
    }
}

/// The persona of one pin, checked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    /// The pin.
    pub pin: String,
    /// The pin's version.
    pub version: String,
    /// The reference toolchain.
    pub reference: String,
    /// The fallback toolchain, if any.
    pub fallback: Option<String>,
    /// rucc's `-fgnuc-version=`.
    pub gnuc: String,
    /// rucc's `--target`, for `arch`.
    pub target: String,
    /// The binutils of the reference.
    pub binutils: Option<String>,
    /// What does not agree, for a person to review. Empty when all agrees.
    pub review: Vec<String>,
}

/// Check every pin's persona against the toolchains and, for NetBSD, the tree's `BASE-VER`.
#[must_use]
pub fn check(pins: &[Pin], personas: &Personas, toolchains: &Toolchains, arch: &str) -> Vec<Line> {
    pins.iter()
        .map(|pin| {
            let Some(persona) = personas.for_pin(pin) else {
                return Line {
                    pin: pin.name.clone(),
                    version: pin.version.clone(),
                    reference: "-".into(),
                    fallback: None,
                    gnuc: "-".into(),
                    target: "-".into(),
                    binutils: None,
                    review: vec![format!(
                        "no persona for {} series {}",
                        pin.system, pin.series
                    )],
                };
            };
            let mut review = Vec::new();
            let reference = toolchains.toolchain.get(&persona.reference);
            match reference {
                None => review.push(format!("{} is not in {TOOLCHAINS}", persona.reference)),
                Some(t) if t.system != pin.system => {
                    review.push(format!("{} is a {} toolchain", persona.reference, t.system));
                }
                Some(t) if t.gcc.as_deref() != Some(&persona.gnuc) => review.push(format!(
                    "{} is GCC {} but the persona is {}",
                    persona.reference,
                    t.gcc.as_deref().unwrap_or("?"),
                    persona.gnuc
                )),
                Some(_) => {}
            }
            if let Some(fallback) = &persona.fallback {
                let gcc = toolchains
                    .toolchain
                    .get(fallback)
                    .and_then(|t| t.gcc.as_deref());
                if gcc.is_none() || gcc != persona.fallback_gnuc.as_deref() {
                    review.push(format!(
                        "the fallback {fallback} is GCC {} but fallback_gnuc is {}",
                        gcc.unwrap_or("?"),
                        persona.fallback_gnuc.as_deref().unwrap_or("?")
                    ));
                }
            }
            if let Some(tree) = &pin.gcc
                && tree != &persona.gnuc
            {
                review.push(format!(
                    "the tree's BASE-VER is {tree} but the persona is {} (04.7)",
                    persona.gnuc
                ));
            }
            let binutils = reference.and_then(|t| {
                t.binutils.clone().or_else(|| {
                    t.needs
                        .iter()
                        .find_map(|n| toolchains.toolchain.get(n).and_then(|d| d.binutils.clone()))
                })
            });
            Line {
                pin: pin.name.clone(),
                version: pin.version.clone(),
                reference: persona.reference.clone(),
                fallback: persona.fallback.clone(),
                gnuc: persona.gnuc.clone(),
                target: persona.target(pin, arch),
                binutils,
                review,
            }
        })
        .collect()
}

/// `11.99` from `11.99.10`, `11.1` from `11.1_STABLE`.
fn two_places(version: &str) -> String {
    let numeric: String = version
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    numeric.split('.').take(2).collect::<Vec<_>>().join(".")
}

fn read(root: &Path, name: &str) -> Result<String, String> {
    let path = root.join(name);
    std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pin(name: &str, system: &str, series: &str, version: &str, gcc: Option<&str>) -> Pin {
        Pin {
            name: name.into(),
            system: system.into(),
            series: series.into(),
            version: version.into(),
            gcc: gcc.map(Into::into),
            ..Pin::default()
        }
    }

    fn files() -> (Personas, Toolchains) {
        let personas = Personas::parse(
            r#"
[[persona]]
system = "freebsd"
series = ["15"]
reference = "gcc15"
gnuc = "15.2.0"
fallback = "gcc14"
fallback_gnuc = "14.2.0"
target_os = "freebsd15.1"
why = "w"

[[persona]]
system = "netbsd"
series = ["current"]
reference = "tools"
gnuc = "14.4.0"
target_os = "netbsd{version}"
why = "w"
"#,
        )
        .unwrap();
        let toolchains = Toolchains::parse(
            r#"
[toolchain.gcc15]
system = "freebsd"
kind = "pkg"
repopath = "a.pkg"
sha256 = "x"
gcc = "15.2.0"
needs = ["bu"]

[toolchain.gcc14]
system = "freebsd"
kind = "pkg"
repopath = "b.pkg"
sha256 = "y"
gcc = "14.2.0"

[toolchain.bu]
system = "freebsd"
kind = "pkg"
repopath = "c.pkg"
sha256 = "z"
binutils = "2.44"

[toolchain.tools]
system = "netbsd"
kind = "build.sh tools"
gcc = "14.4.0"
binutils = "2.46"
"#,
        )
        .unwrap();
        (personas, toolchains)
    }

    #[test]
    fn every_pin_gets_its_persona_and_target() {
        let (personas, toolchains) = files();
        let pins = [
            pin("freebsd-15.1", "freebsd", "15", "15.1-RELEASE", None),
            pin(
                "netbsd-trunk",
                "netbsd",
                "current",
                "11.99.10",
                Some("14.4.0"),
            ),
        ];
        let lines = check(&pins, &personas, &toolchains, "x86_64");
        assert_eq!(lines[0].target, "x86_64-unknown-freebsd15.1");
        assert_eq!(lines[0].binutils.as_deref(), Some("2.44"));
        assert_eq!(lines[0].fallback.as_deref(), Some("gcc14"));
        assert_eq!(lines[1].target, "x86_64-unknown-netbsd11.99");
        assert!(lines.iter().all(|l| l.review.is_empty()), "{lines:?}");
        let netbsd = personas.for_pin(&pins[1]).unwrap();
        assert_eq!(
            netbsd.rucc_args(&pins[1], "x86_64"),
            [
                "--target=x86_64-unknown-netbsd11.99",
                "-fgnuc-version=14.4.0"
            ]
        );
    }

    #[test]
    fn the_fallback_builds_g_and_gives_rucc_its_version() {
        let (mut personas, _) = files();
        let freebsd = pin("freebsd-stable-15", "freebsd", "15", "15.1-STABLE", None);
        personas.fall_back(&freebsd).unwrap();
        let persona = personas.for_pin(&freebsd).unwrap();
        assert_eq!(persona.reference, "gcc14");
        assert_eq!(
            persona.rucc_args(&freebsd, "x86_64")[1],
            "-fgnuc-version=14.2.0"
        );
        assert_eq!(persona.fallback, None);
        // Once is all there is, and a persona with no fallback has none to take.
        let again = personas.fall_back(&freebsd).unwrap_err();
        assert!(again.contains("no fallback"), "{again}");
        let netbsd = pin("netbsd-trunk", "netbsd", "current", "11.99.10", None);
        assert!(personas.fall_back(&netbsd).is_err());
        let none = pin("freebsd-13.5", "freebsd", "13", "13.5-RELEASE", None);
        let why = personas.fall_back(&none).unwrap_err();
        assert!(why.contains("no persona for freebsd-13.5"), "{why}");
    }

    #[test]
    fn a_row_takes_the_flavor_of_its_target_arch() {
        let mut personas = files().0;
        let toolchains = Toolchains::parse(
            "[toolchain.amd64-gcc15]\nsystem = \"freebsd\"\nkind = \"pkg\"\nrepopath = \"a.pkg\"\nsha256 = \"x\"\n\n[toolchain.aarch64-gcc15]\nsystem = \"freebsd\"\nkind = \"pkg\"\nrepopath = \"b.pkg\"\nsha256 = \"y\"\n",
        )
        .unwrap();
        personas.persona[0].reference = "amd64-gcc15".into();
        let row = |arch: Option<&str>| -> Row {
            let names = arch.map_or_else(String::new, |a| format!("target_arch = \"{a}\""));
            toml::from_str(&format!(
                "from = {{}}\nrucc_arch = \"x\"\nfreebsd = {{ {names} }}\nnetbsd = {{}}\n"
            ))
            .unwrap()
        };
        let freebsd = &personas.persona[0];
        assert_eq!(
            freebsd.reference_for(&row(Some("aarch64")), &toolchains),
            "aarch64-gcc15"
        );
        assert_eq!(
            freebsd.reference_for(&row(Some("amd64")), &toolchains),
            "amd64-gcc15"
        );
        // No riscv64-gcc15 in the file, and no TARGET_ARCH: the persona's own reference.
        assert_eq!(
            freebsd.reference_for(&row(Some("riscv64")), &toolchains),
            "amd64-gcc15"
        );
        assert_eq!(
            freebsd.reference_for(&row(None), &toolchains),
            "amd64-gcc15"
        );
        // A NetBSD persona keeps its reference.
        assert_eq!(
            personas.persona[1].reference_for(&row(Some("aarch64")), &toolchains),
            "tools"
        );
    }

    #[test]
    fn a_new_base_ver_or_a_missing_persona_asks_for_review() {
        let (personas, toolchains) = files();
        let pins = [
            pin(
                "netbsd-trunk",
                "netbsd",
                "current",
                "11.99.12",
                Some("15.1.0"),
            ),
            pin("netbsd-12", "netbsd", "12", "12.0", Some("15.1.0")),
        ];
        let lines = check(&pins, &personas, &toolchains, "x86_64");
        assert!(lines[0].review[0].contains("BASE-VER is 15.1.0"));
        assert!(lines[1].review[0].contains("no persona"));
    }

    #[test]
    fn the_rucc_pin_names_its_assets_and_knows_its_binaries() {
        let text = "[toolchain]\n\n[rucc]\nrepo = \"tamnd/rucc\"\nversion = \"v0.30.15\"\ncommand = \"rucc\"\nbinary_sha256 = { x86_64-unknown-linux-gnu = \"ab\" }\n";
        let file = Toolchains::parse(text).unwrap();
        let rucc = file.rucc.unwrap();
        assert_eq!(
            rucc.asset("x86_64-unknown-linux-gnu"),
            "rucc-v0.30.15-x86_64-unknown-linux-gnu.tar.gz"
        );
        assert_eq!(rucc.host_of("ab"), Some("x86_64-unknown-linux-gnu"));
        assert_eq!(rucc.host_of("cd"), None);
    }

    #[test]
    fn a_need_that_is_not_listed_is_refused() {
        let text = "[toolchain.a]\nsystem = \"freebsd\"\nkind = \"pkg\"\nrepopath = \"a\"\nsha256 = \"b\"\nneeds = [\"nope\"]\n";
        assert!(Toolchains::parse(text).unwrap_err().contains("nope"));
        assert_eq!(two_places("11.1_STABLE"), "11.1");
        assert_eq!(two_places("10.2"), "10.2");
    }
}
