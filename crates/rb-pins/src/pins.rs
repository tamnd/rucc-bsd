//! `pins.toml`: the trees, one pin each, that `rb sets --write` expands from `sets.toml`.

use serde::{Deserialize, Serialize};
use std::path::Path;

/// The file name at the top of the repository.
pub const FILE_NAME: &str = "pins.toml";

/// The lines at the top of the written file.
pub const HEADER: &str = "\
# Written by `rb sets --write` from sets.toml and what the projects publish (plan 04.5).
# Do not edit by hand. A branch pin is reference = \"good\" at a commit with a complete baseline
# under baselines/. Under pin_policy.head = \"last-g-good\" it keeps that commit until the
# reference builds and boots a newer one (04.6).

";

/// The whole of `pins.toml`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PinsFile {
    /// The time the projects were read at, as an RFC 3339 UTC time.
    pub as_of: String,
    /// The pins, FreeBSD first, each system's current set before its releases.
    #[serde(default)]
    pub pin: Vec<Pin>,
    /// Pins that left their set. Their results stay in the history.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub gone: Vec<Gone>,
}

/// One pinned tree.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pin {
    /// The name `rb` commands take, such as `freebsd-main` or `netbsd-11.1`.
    pub name: String,
    /// `freebsd` or `netbsd`.
    pub system: String,
    /// `current` or `releases`.
    pub set: String,
    /// `branch`, `releng` (a FreeBSD release branch in BETA or RC) or `release`.
    pub kind: String,
    /// The major version for FreeBSD. For NetBSD, `current` for trunk or the major version.
    pub series: String,
    /// The version the tree says it is: `newvers.sh` for FreeBSD, `param.h` for NetBSD.
    pub version: String,
    /// `nightly` or `weekly`.
    pub cadence: String,
    /// The git repository, for a pin fetched by commit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo: Option<String>,
    /// The branch, for a branch or releng pin.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    /// The tag, for a release pin in git.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tag: Option<String>,
    /// The pinned commit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commit: Option<String>,
    /// The committer date of the pinned commit, or the release date.
    pub date: String,
    /// The newest commit on the branch, which differs from `commit` when the reference failed on
    /// it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub head: Option<String>,
    /// The committer date of `head`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub head_date: Option<String>,
    /// For a branch: `untried` until a baseline runs, then `good` or `failed`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reference: Option<String>,
    /// For NetBSD: the in-tree GCC from `BASE-VER`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gcc: Option<String>,
    /// For a NetBSD release, the branch `gcc` was read from, since the source sets are not in git.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gcc_from: Option<String>,
    /// The release date, `YYYY-MM-DD`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub released: Option<String>,
    /// The expected end of life from the project's table, `YYYY-MM-DD`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub eol: Option<String>,
    /// The source tarballs of a release, with the digests the project publishes.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tarball: Vec<Tarball>,
}

/// One published source tarball.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Tarball {
    /// The file name, such as `src.txz` or `gnusrc.tgz`.
    pub name: String,
    /// Where it is published.
    pub url: String,
    /// The SHA-256, for FreeBSD.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
    /// The SHA-512, for NetBSD.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha512: Option<String>,
}

/// A pin that left its set.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Gone {
    /// The pin's name.
    pub name: String,
    /// `freebsd` or `netbsd`.
    pub system: String,
    /// The version it had.
    pub version: String,
    /// The commit it had, if it was in git.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commit: Option<String>,
    /// The date `rb sets` first saw it gone, `YYYY-MM-DD`.
    pub left: String,
}

impl PinsFile {
    /// Parse the text of `pins.toml`.
    pub fn parse(text: &str) -> Result<Self, String> {
        toml::from_str(text).map_err(|e| format!("{FILE_NAME}: {e}"))
    }

    /// Read `pins.toml` in `root`, or `None` if there is none yet.
    pub fn load(root: &Path) -> Result<Option<Self>, String> {
        let path = root.join(FILE_NAME);
        match std::fs::read_to_string(&path) {
            Ok(text) => Self::parse(&text).map(Some),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(format!("{}: {e}", path.display())),
        }
    }

    /// The text of the file.
    #[must_use]
    pub fn render(&self) -> String {
        let body = toml::to_string(self).expect("pins serialize");
        format!("{HEADER}{body}")
    }

    /// The pin with this name.
    #[must_use]
    pub fn find(&self, name: &str) -> Option<&Pin> {
        self.pin.iter().find(|p| p.name == name)
    }

    /// Whether two files name the same trees, whatever time they were read at.
    #[must_use]
    pub fn same_pins(&self, other: &Self) -> bool {
        self.pin == other.pin && self.gone == other.gone
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_reads_back_as_written() {
        let file = PinsFile {
            as_of: "2026-11-08T06:00:00Z".into(),
            pin: vec![
                Pin {
                    name: "freebsd-main".into(),
                    system: "freebsd".into(),
                    set: "current".into(),
                    kind: "branch".into(),
                    series: "16".into(),
                    version: "16.0-CURRENT".into(),
                    cadence: "nightly".into(),
                    repo: Some("https://github.com/freebsd/freebsd-src".into()),
                    branch: Some("main".into()),
                    commit: Some("a".repeat(40)),
                    date: "2026-11-08T01:36:00Z".into(),
                    head: Some("a".repeat(40)),
                    reference: Some("untried".into()),
                    ..Pin::default()
                },
                Pin {
                    name: "netbsd-11.1".into(),
                    tarball: vec![Tarball {
                        name: "src.tgz".into(),
                        url: "https://cdn.netbsd.org/x".into(),
                        sha512: Some("b".repeat(128)),
                        ..Tarball::default()
                    }],
                    ..Pin::default()
                },
            ],
            gone: vec![Gone {
                name: "freebsd-14.4".into(),
                ..Gone::default()
            }],
        };
        let text = file.render();
        assert!(text.starts_with("# Written by `rb sets --write`"));
        assert!(text.contains("[[pin]]"));
        assert!(text.contains("[[pin.tarball]]"));
        assert!(!text.contains("tag ="));
        let back = PinsFile::parse(&text).unwrap();
        assert_eq!(back, file);
        assert!(back.same_pins(&PinsFile {
            as_of: "later".into(),
            ..file.clone()
        }));
        assert_eq!(back.find("netbsd-11.1").unwrap().tarball.len(), 1);
    }
}
