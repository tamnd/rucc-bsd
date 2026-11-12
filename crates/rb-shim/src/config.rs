//! What the shim needs to know: the compilers, the side of the build, and where to write.
//!
//! `rb` puts the shim and its names in a toolchain directory and writes `rb-shim.toml` next to
//! them. The shim reads that file from the directory it was called from, and environment
//! variables of the same meaning override it. The file is what makes the shim work when a sub
//! make cleans the environment, which both FreeBSD's `buildworld` and NetBSD's `build.sh` do.

use harness_shim::bringup;
use harness_shim::env::truthy;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// The name of the file next to the shim.
pub const FILE_NAME: &str = "rb-shim.toml";

/// Which build a call belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Side {
    /// The rucc build.
    R,
    /// The reference build.
    G,
}

impl Side {
    /// The letter, as it goes on the record.
    #[must_use]
    pub fn letter(self) -> &'static str {
        match self {
            Self::R => "R",
            Self::G => "G",
        }
    }

    /// Read `R` or `G`.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        match text.trim() {
            "R" | "r" => Some(Self::R),
            "G" | "g" => Some(Self::G),
            _ => None,
        }
    }
}

/// The shim's settings.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct ShimConfig {
    /// The `compile.jsonl` to append to. Overridden by `RB_COMPILE_LOG`. Empty means no record.
    #[serde(default)]
    pub log: PathBuf,
    /// The side for a name that does not say. A name ending in `-rucc` is always R and one ending
    /// in `-ref` is always G (05.4). Overridden by `RB_SIDE`.
    #[serde(default)]
    pub side: Option<Side>,
    /// rucc. Overridden by `RB_RUCC`.
    #[serde(default)]
    pub rucc: PathBuf,
    /// The arguments rucc gets before the build's: the target and the persona, as in
    /// `--target=x86_64-unknown-freebsd15.1` and `-fgnuc-version=15.2.0`. Overridden by
    /// `RB_RUCC_ARGS`, split on spaces.
    #[serde(default)]
    pub rucc_args: Vec<String>,
    /// A sysroot rucc gets when the call names none. NetBSD's in-tree GCC is configured with
    /// `--with-sysroot`, so the reference has one built in and rucc has to be told.
    #[serde(default)]
    pub rucc_sysroot: Option<String>,
    /// Whether the `cpp` slot runs rucc in R. NetBSD's does from the start; FreeBSD's `XCPP` stays
    /// the reference's until the census says otherwise (05.4).
    #[serde(default)]
    pub rucc_cpp: bool,
    /// The reference C compiler. Overridden by `RB_REF_CC`.
    #[serde(default)]
    pub ref_cc: PathBuf,
    /// The reference C++ compiler, which both sides use. Overridden by `RB_REF_CXX`.
    #[serde(default)]
    pub ref_cxx: PathBuf,
    /// The reference preprocessor. Overridden by `RB_REF_CPP`.
    #[serde(default)]
    pub ref_cpp: PathBuf,
    /// The reference assembler, for a direct `${AS}` call. Overridden by `RB_REF_AS`.
    #[serde(default)]
    pub ref_as: PathBuf,
    /// The reference linker, for FreeBSD's `XLD` (see [`crate::link`]). Overridden by `RB_REF_LD`.
    #[serde(default)]
    pub ref_ld: PathBuf,
    /// Names the shim is called by that do not end in one of the `rb-` names, each mapped to its
    /// slot: `cc`, `c++`, `cpp`, `as` or `ld`. NetBSD's `TOOLDIR` calls it `x86_64--netbsd-gcc`
    /// and `x86_64--netbsd-cpp`. FreeBSD's `XLD` calls it `ld`.
    #[serde(default)]
    pub names: BTreeMap<String, String>,
    /// Directories whose files are probe outputs. A build's object directory must not be under
    /// one of them. Empty means `/tmp`, `/var/tmp` and `TMPDIR`.
    #[serde(default)]
    pub temp_dirs: Vec<PathBuf>,
    /// Whether to add `-frucc-trace`. Overridden by `RB_RUCC_TRACE`.
    #[serde(default)]
    pub rucc_trace: bool,
    /// Whether to compile every unit twice and compare. Overridden by `RB_TWICE`.
    #[serde(default)]
    pub twice: bool,
    /// The bring-up classes, `as` and `cxx-c`. Overridden by `RB_BRINGUP`, a comma separated list.
    /// Empty in every graded run.
    #[serde(default)]
    pub bringup: Vec<String>,
    /// The C units `cxx-c` hands to the reference, each a path suffix of the source file and the
    /// issue that tracks it, as in `stand/i386/boot2/boot2.c=tamnd/rucc#4097`. Overridden by
    /// `RB_BRINGUP_UNITS`, a comma separated list. Plan 16 asks for an issue for each delegated
    /// unit, so a unit without one is refused.
    #[serde(default)]
    pub bringup_units: Vec<String>,
}

/// A unit of `cxx-c` split into the path suffix and the issue, from `PATH=ISSUE`.
#[must_use]
pub fn unit_and_issue(entry: &str) -> (&str, Option<&str>) {
    match entry.split_once('=') {
        Some((unit, issue)) => (unit.trim(), Some(issue.trim())),
        None => (entry.trim(), None),
    }
}

/// Check that each unit of `cxx-c` names its issue as `OWNER/REPO#N`.
///
/// # Errors
///
/// Names the first unit that has no issue or whose issue is not of that form.
pub fn check_units(units: &[String]) -> Result<(), String> {
    for entry in units {
        let (unit, issue) = unit_and_issue(entry);
        let issue = issue.unwrap_or_default();
        let good = issue.split_once('#').is_some_and(|(repo, number)| {
            repo.split('/').count() == 2
                && repo.split('/').all(|part| !part.is_empty())
                && !number.is_empty()
                && number.bytes().all(|b| b.is_ascii_digit())
        });
        if unit.is_empty() || !good {
            return Err(format!(
                "bring-up unit {entry} names no issue: write it as PATH=OWNER/REPO#N, as in stand/i386/boot2/boot2.c=tamnd/rucc#4097"
            ));
        }
    }
    Ok(())
}

/// The bring-up classes of 00: `as` sends assembly to the reference, and `cxx-c` sends the named C
/// units to it.
pub const CLASSES: bringup::Classes<'static> = bringup::Classes {
    known: &["as", "cxx-c"],
    retired: &[],
};

impl ShimConfig {
    /// Read the file in `dir`, if there is one, then apply the environment.
    ///
    /// # Errors
    ///
    /// When the file exists and does not parse. A shim that silently ignored a broken file would
    /// run whatever compiler the environment happened to name.
    pub fn load(dir: Option<&Path>, env: &dyn Fn(&str) -> Option<String>) -> Result<Self, String> {
        let mut config = match dir.map(|d| d.join(FILE_NAME)) {
            Some(path) if path.is_file() => {
                let text = std::fs::read_to_string(&path)
                    .map_err(|e| format!("reading {}: {e}", path.display()))?;
                toml::from_str::<Self>(&text).map_err(|e| format!("{}: {e}", path.display()))?
            }
            _ => Self::default(),
        };
        let set = |key: &str| env(key).filter(|v| !v.is_empty());
        if let Some(v) = set("RB_COMPILE_LOG") {
            config.log = PathBuf::from(v);
        }
        if let Some(v) = set("RB_SIDE") {
            config.side = Some(Side::parse(&v).ok_or_else(|| format!("RB_SIDE={v}: R or G"))?);
        }
        for (key, slot) in [
            ("RB_RUCC", &mut config.rucc),
            ("RB_REF_CC", &mut config.ref_cc),
            ("RB_REF_CXX", &mut config.ref_cxx),
            ("RB_REF_CPP", &mut config.ref_cpp),
            ("RB_REF_AS", &mut config.ref_as),
            ("RB_REF_LD", &mut config.ref_ld),
        ] {
            if let Some(v) = set(key) {
                *slot = PathBuf::from(v);
            }
        }
        if let Some(v) = set("RB_RUCC_ARGS") {
            config.rucc_args = v.split_whitespace().map(String::from).collect();
        }
        if let Some(v) = env("RB_RUCC_TRACE") {
            config.rucc_trace = truthy(&v);
        }
        if let Some(v) = env("RB_TWICE") {
            config.twice = truthy(&v);
        }
        if let Some(v) = env("RB_BRINGUP") {
            config.bringup = bringup::split_list(&v);
        }
        if let Some(v) = env("RB_BRINGUP_UNITS") {
            config.bringup_units = bringup::split_list(&v);
        }
        CLASSES.check(&config.bringup)?;
        check_units(&config.bringup_units)?;
        Ok(config)
    }

    /// The file's text.
    #[must_use]
    pub fn to_toml(&self) -> String {
        toml::to_string(self).unwrap_or_default()
    }

    /// Whether a bring-up class is on.
    #[must_use]
    pub fn delegates(&self, class: &str) -> bool {
        self.bringup.iter().any(|c| c == class)
    }

    /// The directories whose files are probe outputs.
    #[must_use]
    pub fn probe_dirs(&self, tmpdir: Option<&str>) -> Vec<PathBuf> {
        if !self.temp_dirs.is_empty() {
            return self.temp_dirs.clone();
        }
        let mut dirs = vec![PathBuf::from("/tmp"), PathBuf::from("/var/tmp")];
        if let Some(dir) = tmpdir.filter(|d| !d.is_empty()) {
            dirs.push(PathBuf::from(dir));
        }
        dirs
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn env_of(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let map: HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect();
        move |key| map.get(key).cloned()
    }

    #[test]
    fn the_environment_alone_is_enough() {
        let config = ShimConfig::load(
            None,
            &env_of(&[
                ("RB_RUCC", "/opt/rucc/bin/rucc"),
                (
                    "RB_RUCC_ARGS",
                    "--target=x86_64-unknown-netbsd11.99 -fgnuc-version=14.4.0",
                ),
                ("RB_SIDE", "R"),
                ("RB_TWICE", "1"),
            ]),
        )
        .unwrap();
        assert_eq!(config.rucc, Path::new("/opt/rucc/bin/rucc"));
        assert_eq!(config.rucc_args.len(), 2);
        assert_eq!(config.side, Some(Side::R));
        assert!(config.twice);
        assert!(!config.rucc_trace);
    }

    #[test]
    fn the_file_is_read_and_the_environment_wins() {
        let dir = std::env::temp_dir().join(format!("rb-shim-config-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let written = ShimConfig {
            log: "/b/compile.jsonl".into(),
            side: Some(Side::G),
            ref_cc: "/usr/local/bin/x86_64-unknown-freebsd15.1-gcc15".into(),
            names: BTreeMap::from([("x86_64--netbsd-gcc".to_string(), "cc".to_string())]),
            ..ShimConfig::default()
        };
        std::fs::write(dir.join(FILE_NAME), written.to_toml()).unwrap();
        let read = ShimConfig::load(Some(&dir), &env_of(&[])).unwrap();
        assert_eq!(read, written);
        let overridden = ShimConfig::load(Some(&dir), &env_of(&[("RB_SIDE", "R")])).unwrap();
        assert_eq!(overridden.side, Some(Side::R));
        std::fs::write(dir.join(FILE_NAME), "side = 3\n").unwrap();
        assert!(ShimConfig::load(Some(&dir), &env_of(&[])).is_err());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn bringup_takes_only_the_two_classes_of_00() {
        let config = ShimConfig::load(None, &env_of(&[("RB_BRINGUP", "as, cxx-c")])).unwrap();
        assert!(config.delegates("as") && config.delegates("cxx-c"));
        let wrong = ShimConfig::load(None, &env_of(&[("RB_BRINGUP", "m16")])).unwrap_err();
        assert!(wrong.contains("m16"), "{wrong}");
        let side = ShimConfig::load(None, &env_of(&[("RB_SIDE", "X")])).unwrap_err();
        assert!(side.contains("RB_SIDE"), "{side}");
    }

    #[test]
    fn each_bringup_unit_names_its_issue() {
        let units = "stand/i386/boot2/boot2.c=tamnd/rucc#4097, lib/a.c = tamnd/rucc#1";
        let config = ShimConfig::load(None, &env_of(&[("RB_BRINGUP_UNITS", units)])).unwrap();
        assert_eq!(config.bringup_units.len(), 2);
        assert_eq!(
            unit_and_issue(&config.bringup_units[1]),
            ("lib/a.c", Some("tamnd/rucc#1"))
        );
        for wrong in [
            "lib/a.c",
            "lib/a.c=",
            "lib/a.c=#4097",
            "lib/a.c=rucc#4097",
            "=tamnd/rucc#1",
        ] {
            let said = ShimConfig::load(None, &env_of(&[("RB_BRINGUP_UNITS", wrong)])).unwrap_err();
            assert!(said.contains("names no issue"), "{wrong}: {said}");
        }
    }

    #[test]
    fn probe_directories_default_to_the_temporary_ones() {
        let config = ShimConfig::default();
        assert_eq!(config.probe_dirs(Some("/scratch/t")).len(), 3);
        let set = ShimConfig {
            temp_dirs: vec!["/x".into()],
            ..ShimConfig::default()
        };
        assert_eq!(set.probe_dirs(Some("/scratch/t")), [PathBuf::from("/x")]);
    }
}
