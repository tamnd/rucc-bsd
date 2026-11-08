//! `rb personas`: the persona and the reference toolchain of every pin (plan 04.3).

use crate::repo::Repo;
use rb_pins::pins::PinsFile;
use rb_pins::toolchains::{Personas, Toolchains, check};
use std::fmt::Write as _;

/// The report, and whether any pin needs a person to review its persona.
pub fn run(repo: &Repo, arch: &str) -> Result<(String, bool), String> {
    let pins = PinsFile::load(&repo.root)?
        .ok_or_else(|| format!("no {} yet; run rb sets --write", rb_pins::pins::FILE_NAME))?;
    let personas = Personas::load(&repo.root)?;
    let toolchains = Toolchains::load(&repo.root)?;
    let lines = check(&pins.pin, &personas, &toolchains, arch);
    let mut out = String::new();
    let _ = writeln!(
        out,
        "{:<22} {:<14} {:<8} {:<34} {:<9} {:<8} target",
        "pin", "version", "persona", "reference", "binutils", "check"
    );
    let mut review = false;
    for line in &lines {
        let reference = match &line.fallback {
            Some(f) => format!("{}, else {f}", line.reference),
            None => line.reference.clone(),
        };
        let _ = writeln!(
            out,
            "{:<22} {:<14} {:<8} {:<34} {:<9} {:<8} {}",
            line.pin,
            line.version,
            line.gnuc,
            reference,
            line.binutils.as_deref().unwrap_or("-"),
            if line.review.is_empty() {
                "ok"
            } else {
                "REVIEW"
            },
            line.target,
        );
        for r in &line.review {
            review = true;
            let _ = writeln!(out, "  review: {r}");
        }
    }
    Ok((out, review))
}
