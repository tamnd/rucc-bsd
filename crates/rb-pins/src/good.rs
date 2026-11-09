//! The G-good commits of the pins (plan 04.6): the commits at which G built and booted.
//!
//! `rb baseline` writes `baselines/<system>/<row>/<version>/<conf>/baseline.toml`. A baseline
//! with `complete = true` says that G built the pin at its `commit` and that every run booted and
//! finished. `rb sets` reads these files, so a branch pin is `good` at a commit only when a
//! baseline says so, and nobody edits `pins.toml` by hand.

use serde::Deserialize;
use std::path::{Path, PathBuf};

/// The directory of the baselines, under the top of the repository.
pub const DIR: &str = "baselines";

/// One complete baseline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Good {
    /// The pin, such as `freebsd-main`.
    pub pin: String,
    /// The commit G built.
    pub commit: String,
    /// When the baseline was written, as an RFC 3339 UTC time.
    pub made: String,
    /// The baseline's directory under `baselines/`, such as `freebsd/amd64/main/graded`.
    pub place: String,
}

/// The keys of `baseline.toml` that say whether G is good.
#[derive(Deserialize)]
struct Head {
    pin: String,
    #[serde(default)]
    commit: String,
    #[serde(default)]
    made: String,
    #[serde(default)]
    complete: bool,
}

/// The complete baselines with a commit under `root/baselines`, made at or before `until`,
/// newest first. A missing directory gives none.
pub fn from_baselines(root: &Path, until: &str) -> Result<Vec<Good>, String> {
    let top = root.join(DIR);
    let mut files = Vec::new();
    find(&top, 0, &mut files)?;
    let mut out = Vec::new();
    for file in files {
        let text =
            std::fs::read_to_string(&file).map_err(|e| format!("{}: {e}", file.display()))?;
        let head: Head = toml::from_str(&text).map_err(|e| format!("{}: {e}", file.display()))?;
        if !head.complete || head.commit.is_empty() || head.made.as_str() > until {
            continue;
        }
        let place = file
            .parent()
            .and_then(|d| d.strip_prefix(&top).ok())
            .map(|d| d.to_string_lossy().replace('\\', "/"))
            .unwrap_or_default();
        out.push(Good {
            pin: head.pin,
            commit: head.commit,
            made: head.made,
            place,
        });
    }
    out.sort_by(|a, b| b.made.cmp(&a.made).then_with(|| a.place.cmp(&b.place)));
    Ok(out)
}

/// The `baseline.toml` files at `<system>/<row>/<version>/<conf>/` under `dir`.
fn find(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) -> Result<(), String> {
    if depth == 4 {
        let file = dir.join("baseline.toml");
        if file.is_file() {
            out.push(file);
        }
        return Ok(());
    }
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(format!("{}: {e}", dir.display())),
    };
    let mut dirs: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    dirs.sort();
    for d in dirs {
        find(&d, depth + 1, out)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(top: &Path, place: &str, text: &str) {
        let dir = top.join(DIR).join(place);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("baseline.toml"), text).unwrap();
    }

    #[test]
    fn only_complete_baselines_made_in_time_count() {
        let top = std::env::temp_dir().join(format!("rb-good-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&top);
        assert!(
            from_baselines(&top, "2026-11-09T00:00:00Z")
                .unwrap()
                .is_empty()
        );
        let one = |pin: &str, commit: &str, made: &str, complete: bool| {
            format!(
                "# Written by rb baseline.\n\npin = \"{pin}\"\nsystem = \"freebsd\"\ncommit = \"{commit}\"\n\
                 made = \"{made}\"\ncomplete = {complete}\nruns = 1\n\n[[run]]\nn = 1\nresult = \"pass\"\n"
            )
        };
        write(
            &top,
            "freebsd/amd64/main/graded",
            &one("freebsd-main", "aaa", "2026-11-08T10:00:00Z", true),
        );
        write(
            &top,
            "freebsd/amd64/main/smoke",
            &one("freebsd-main", "bbb", "2026-11-08T12:00:00Z", true),
        );
        write(
            &top,
            "freebsd/amd64/stable-14/graded",
            &one("freebsd-stable-14", "ccc", "2026-11-08T11:00:00Z", false),
        );
        write(
            &top,
            "freebsd/amd64/stable-15/graded",
            &one("freebsd-stable-15", "", "2026-11-08T11:00:00Z", true),
        );
        write(
            &top,
            "netbsd/amd64/trunk/graded",
            &one("netbsd-trunk", "ddd", "2026-11-10T00:00:00Z", true),
        );
        let good = from_baselines(&top, "2026-11-09T00:00:00Z").unwrap();
        let got: Vec<(&str, &str, &str)> = good
            .iter()
            .map(|g| (g.pin.as_str(), g.commit.as_str(), g.place.as_str()))
            .collect();
        assert_eq!(
            got,
            [
                ("freebsd-main", "bbb", "freebsd/amd64/main/smoke"),
                ("freebsd-main", "aaa", "freebsd/amd64/main/graded"),
            ]
        );
        assert_eq!(
            from_baselines(&top, "2026-11-11T00:00:00Z").unwrap().len(),
            3
        );
        std::fs::remove_dir_all(&top).unwrap();
    }
}
