//! `sets.toml`: the rules that say which trees are pinned (plan 04.5).

use serde::Deserialize;
use std::path::Path;

/// The file name at the top of the repository.
pub const FILE_NAME: &str = "sets.toml";

/// The whole of `sets.toml`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Sets {
    /// The FreeBSD sets.
    pub freebsd: System,
    /// The NetBSD sets.
    pub netbsd: System,
    /// How a branch pin moves.
    pub pin_policy: PinPolicy,
}

/// The two sets of one system.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct System {
    /// Branch heads.
    pub current: Current,
    /// Releases.
    pub releases: Releases,
}

/// The current set: named branches, and for FreeBSD the release branch being prepared.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Current {
    /// The branches, by their names in the project's git repository.
    pub branches: Vec<String>,
    /// Whether a `releng/X.Y` branch in BETA or RC joins the set.
    #[serde(default)]
    pub releng_in_progress: bool,
    /// Whether the set runs every night.
    #[serde(default)]
    pub nightly: bool,
}

/// The releases set.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Releases {
    /// Take the releases from the project's own table of supported releases.
    pub supported_only: bool,
    /// Keep only the newest supported release of each major version.
    #[serde(default)]
    pub latest_per_major: bool,
    /// Whether the set runs every week.
    #[serde(default)]
    pub weekly: bool,
}

/// How branch pins move (plan 04.6).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PinPolicy {
    /// `last-g-good` keeps a branch at the newest commit where the reference built and booted.
    /// `head` always takes the newest commit.
    pub head: String,
}

impl Sets {
    /// Parse the text of `sets.toml`.
    pub fn parse(text: &str) -> Result<Self, String> {
        let sets: Self = toml::from_str(text).map_err(|e| format!("{FILE_NAME}: {e}"))?;
        if !matches!(sets.pin_policy.head.as_str(), "last-g-good" | "head") {
            return Err(format!(
                "{FILE_NAME}: pin_policy.head is {:?}; it must be \"last-g-good\" or \"head\"",
                sets.pin_policy.head
            ));
        }
        for (name, system) in [("freebsd", &sets.freebsd), ("netbsd", &sets.netbsd)] {
            if !system.releases.supported_only {
                return Err(format!(
                    "{FILE_NAME}: {name}.releases.supported_only must be true; no other rule is known"
                ));
            }
        }
        Ok(sets)
    }

    /// Read `sets.toml` in `root`.
    pub fn load(root: &Path) -> Result<Self, String> {
        let path = root.join(FILE_NAME);
        let text =
            std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        Self::parse(&text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PLAN: &str = r#"
[freebsd.current]
branches = ["main", "stable/15", "stable/14"]
releng_in_progress = true
nightly = true

[freebsd.releases]
supported_only = true
weekly = true

[netbsd.current]
branches = ["trunk", "netbsd-11", "netbsd-10"]
nightly = true

[netbsd.releases]
supported_only = true
weekly = true

[pin_policy]
head = "last-g-good"
"#;

    #[test]
    fn the_rules_of_the_plan_parse() {
        let sets = Sets::parse(PLAN).unwrap();
        assert_eq!(sets.freebsd.current.branches.len(), 3);
        assert!(sets.freebsd.current.releng_in_progress);
        assert!(!sets.netbsd.current.releng_in_progress);
        assert!(!sets.freebsd.releases.latest_per_major);
        assert_eq!(sets.pin_policy.head, "last-g-good");
    }

    #[test]
    fn mistakes_are_refused() {
        let typo = PLAN.replace("nightly = true", "nighty = true");
        assert!(Sets::parse(&typo).unwrap_err().contains("nighty"));
        let policy = PLAN.replace("last-g-good", "newest");
        assert!(Sets::parse(&policy).unwrap_err().contains("pin_policy"));
        let all = PLAN.replacen("supported_only = true", "supported_only = false", 1);
        assert!(Sets::parse(&all).unwrap_err().contains("supported_only"));
    }
}
