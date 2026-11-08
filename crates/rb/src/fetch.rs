//! `rb fetch`: fetch and verify pinned trees into the cache (plan 14.3).

use crate::repo::Repo;
use rb_pins::fetch::{Way, fetch, way};
use rb_pins::pins::{FILE_NAME, Pin, PinsFile};
use std::fmt::Write as _;

/// Fetch the pin named `name`, or every pin of `set`. Returns the report.
pub fn run(
    repo: &Repo,
    name: Option<&str>,
    set: Option<&str>,
    tarball: bool,
) -> Result<String, String> {
    let pins = PinsFile::load(&repo.root)?
        .ok_or_else(|| format!("no {FILE_NAME} yet; run rb sets --write"))?;
    let chosen: Vec<&Pin> = match (name, set) {
        (Some(name), None) => vec![pins.find(name).ok_or_else(|| {
            let names: Vec<&str> = pins.pin.iter().map(|p| p.name.as_str()).collect();
            format!(
                "no pin {name} in {FILE_NAME}; the pins are {}",
                names.join(", ")
            )
        })?],
        (None, Some(set)) => {
            let chosen: Vec<&Pin> = pins.pin.iter().filter(|p| p.set == set).collect();
            if chosen.is_empty() {
                return Err(format!(
                    "no pin is in the set {set}; the sets are current and releases"
                ));
            }
            chosen
        }
        _ => return Err("name one pin, or give --set current or --set releases".into()),
    };
    let mut report = String::new();
    for pin in chosen {
        let how = way(pin, tarball)?;
        let got = fetch(pin, &repo.cache, how)?;
        let source = match how {
            Way::Git => format!("commit {}", pin.commit.as_deref().unwrap_or_default()),
            Way::Tarball => format!("{} tarballs", pin.tarball.len()),
        };
        let _ = writeln!(
            report,
            "{} {} ({source}, version {}) {}",
            pin.name,
            if got.cached {
                "was in the cache"
            } else {
                "fetched and verified"
            },
            pin.version,
            got.src.display()
        );
    }
    Ok(report)
}
