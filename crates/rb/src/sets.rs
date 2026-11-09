//! `rb sets`: expand `sets.toml` into `pins.toml` (plan 04.5).

use crate::repo::Repo;
use rb_pins::expand::{Expanded, expand};
use rb_pins::pins::{FILE_NAME, PinsFile};
use rb_pins::sets::Sets;
use rb_pins::web::Web;
use std::fmt::Write as _;

/// What `rb sets` found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    /// The report to print.
    pub report: String,
    /// Whether `pins.toml` already named these trees.
    pub up_to_date: bool,
}

/// Run `rb sets`. `as_of` is an RFC 3339 UTC time, or `None` for now.
pub fn run(
    repo: &Repo,
    web: &dyn Web,
    as_of: Option<&str>,
    write: bool,
) -> Result<Outcome, String> {
    if let Some(t) = as_of {
        check_time(t)?;
    }
    let sets = Sets::load(&repo.root)?;
    let old = PinsFile::load(&repo.root)?;
    let time = as_of.map_or_else(now, str::to_string);
    let good = rb_pins::good::from_baselines(&repo.root, &time)?;
    let Expanded { file, notes } = expand(&sets, web, &time, as_of, old.as_ref(), &good)?;
    let up_to_date = old.as_ref().is_some_and(|o| o.same_pins(&file));

    let mut report = table(&file);
    for note in &notes {
        let _ = writeln!(report, "note: {note}");
    }
    if write {
        let path = repo.root.join(FILE_NAME);
        std::fs::write(&path, file.render()).map_err(|e| format!("{}: {e}", path.display()))?;
        let _ = writeln!(report, "wrote {FILE_NAME} as of {time}");
    } else if up_to_date {
        let _ = writeln!(report, "{FILE_NAME} is up to date");
    } else {
        let _ = writeln!(report, "{FILE_NAME} is out of date; run rb sets --write");
    }
    Ok(Outcome { report, up_to_date })
}

fn table(file: &PinsFile) -> String {
    let mut out = String::new();
    for pin in &file.pin {
        let source = match &pin.commit {
            Some(commit) => commit.get(..9).unwrap_or(commit).to_string(),
            None if pin.tarball.is_empty() => "-".into(),
            None => format!("{} tarballs", pin.tarball.len()),
        };
        let _ = writeln!(
            out,
            "{:<22} {:<16} {:<10} {}",
            pin.name,
            pin.version,
            source,
            pin.date.get(..10).unwrap_or(&pin.date)
        );
    }
    for gone in &file.gone {
        let _ = writeln!(
            out,
            "{:<22} {:<16} gone on {}",
            gone.name, gone.version, gone.left
        );
    }
    out
}

/// Refuse a time GitHub would read differently from the way it is written in `pins.toml`.
fn check_time(t: &str) -> Result<(), String> {
    let b = t.as_bytes();
    let digits =
        |r: std::ops::Range<usize>| r.clone().all(|i| b.get(i).is_some_and(u8::is_ascii_digit));
    let ok = b.len() == 20
        && digits(0..4)
        && b[4] == b'-'
        && digits(5..7)
        && b[7] == b'-'
        && digits(8..10)
        && b[10] == b'T'
        && digits(11..13)
        && b[13] == b':'
        && digits(14..16)
        && b[16] == b':'
        && digits(17..19)
        && b[19] == b'Z';
    if ok {
        Ok(())
    } else {
        Err(format!(
            "--as-of {t}: give a UTC time such as 2026-11-08T06:00:00Z"
        ))
    }
}

/// The time now, as `YYYY-MM-DDTHH:MM:SSZ`.
#[must_use]
pub fn now() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    utc(secs)
}

/// Seconds since 1970 as `YYYY-MM-DDTHH:MM:SSZ`, by the civil-from-days method.
#[must_use]
pub fn utc(secs: u64) -> String {
    let days = i64::try_from(secs / 86_400).unwrap_or(0);
    let rest = secs % 86_400;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rest / 3600,
        rest % 3600 / 60,
        rest % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn times() {
        assert_eq!(utc(0), "1970-01-01T00:00:00Z");
        assert_eq!(utc(1_794_117_600), "2026-11-08T06:00:00Z");
        assert_eq!(utc(951_782_400), "2000-02-29T00:00:00Z");
        assert!(check_time("2026-11-08T06:00:00Z").is_ok());
        assert!(check_time("2026-11-08").is_err());
        assert!(check_time("2026-11-08T06:00:00+07:00").is_err());
    }
}
