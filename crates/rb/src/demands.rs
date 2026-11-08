//! `rb demands`: the feature census of one build (plan 14.5).
//!
//! The input is the `compile.jsonl` of a build, normally a G build. For every C and assembly unit
//! that the C compiler slot built, the census:
//!
//! - reads the `-f`, `-m`, `-W`, `--param` and `-std=` flags from the command line;
//! - asks the compiler that built the unit for its preprocessed text, by running the recorded
//!   command again in the recorded directory with `-E` in place of `-c` and `-o`;
//! - scans that text for the features of [`crate::scan`].
//!
//! A `.i` or `.s` unit is already preprocessed, so it is read as it is. The replay needs the
//! build's source and object trees, so `rb demands` runs where the build ran, after it.
//!
//! The result is counted per feature: the units that use it and the uses. `--write` puts it in
//! `demands.toml`, one `[[census]]` per pin, row and configuration. Plan 14.5 also wants rucc's
//! state for each feature, from `rucc --print-features`. rucc does not print that table yet, so
//! the census does not have a state column.

use crate::scan::{self, Features, kind};
use harness_shim::args::{is_asm_source, is_c_source};
use harness_shim::record::{CompileRecord, read_log};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::Path;
use std::process::{Command, Stdio};

/// The file name at the top of the repository.
pub const FILE_NAME: &str = "demands.toml";

const HEADER: &str = "\
# The demand census of plan 14.5, written by `rb demands --write`. Do not edit it by hand.
# There is one [[census]] per pin, row and configuration. `units` is how many C and assembly
# units use a feature, and `uses` is how many times they use it.
";

/// What one census counted.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Census {
    /// The pin of `pins.toml`.
    pub pin: String,
    /// The row of `rows.toml`.
    pub row: String,
    /// The configuration, such as `graded` or `c-only`.
    pub conf: String,
    /// The SHA-256 of the `compile.jsonl` that was read.
    pub log_sha256: String,
    /// The C and assembly units of the C compiler slot.
    pub units: usize,
    /// The units whose text was read.
    pub read: usize,
    /// The features, by kind, then by units, most first.
    pub feature: Vec<Feature>,
}

/// One feature.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Feature {
    /// What kind of feature it is: see [`crate::scan::kind`].
    pub kind: String,
    /// Its name.
    pub name: String,
    /// The units that use it.
    pub units: usize,
    /// How many times they use it.
    pub uses: usize,
}

/// The whole of `demands.toml`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DemandsFile {
    /// The censuses.
    #[serde(default)]
    pub census: Vec<Census>,
}

/// One unit to read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unit {
    /// The source, as the record spelled it.
    pub source: String,
    /// Whether it is assembly.
    pub asm: bool,
    /// Whether its text must be made with `-E`.
    pub preprocess: bool,
    /// The record's index in the log.
    pub record: usize,
}

/// The units of the C compiler slot that built: C and assembly, not C++.
#[must_use]
pub fn units(records: &[CompileRecord]) -> Vec<Unit> {
    let mut out = Vec::new();
    for (i, r) in records.iter().enumerate() {
        let slot = r.slot.as_deref().unwrap_or("cc");
        if !r.is_unit()
            || !r.succeeded()
            || slot != "cc"
            || r.origin.as_deref() == Some("cxx-on-cc")
        {
            continue;
        }
        let Some(source) = r
            .inputs
            .iter()
            .map(|i| i.path.as_str())
            .find(|p| is_c_source(p) || is_asm_source(p))
        else {
            continue;
        };
        out.push(Unit {
            source: source.to_string(),
            asm: is_asm_source(source),
            preprocess: !Path::new(source)
                .extension()
                .is_some_and(|e| e == "i" || e == "s"),
            record: i,
        });
    }
    out
}

/// The flags of a command line that 14.5 counts, with paths in their values replaced by `PATH`.
#[must_use]
pub fn flags(argv: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    let mut i = 1;
    while i < argv.len() {
        let a = argv[i].as_str();
        if a == "--param" {
            if let Some(v) = argv.get(i + 1) {
                out.push(format!("--param {}", without_path(v)));
            }
            i += 2;
            continue;
        }
        if let Some(v) = a.strip_prefix("--param=") {
            out.push(format!("--param {}", without_path(v)));
        } else if let Some(rest) = ["-Wl,", "-Wa,", "-Wp,"].iter().find(|p| a.starts_with(**p)) {
            let first = a[rest.len()..].split(',').next().unwrap_or_default();
            out.push(format!("{rest}{}", without_path(first)));
        } else if a.len() > 2 && (a.starts_with("-f") || a.starts_with("-m") || a.starts_with("-W"))
            || a.starts_with("-std=")
        {
            out.push(without_path(a));
        }
        i += 1;
    }
    out
}

fn without_path(flag: &str) -> String {
    match flag.split_once('=') {
        Some((k, v)) if v.contains('/') => format!("{k}=PATH"),
        _ => flag.to_string(),
    }
}

/// The command that prints a unit's preprocessed text: the record's arguments with `-E` in place
/// of `-c`, and without the output and dependency file options.
#[must_use]
pub fn replay_args(record: &CompileRecord) -> Vec<String> {
    let mut out: Vec<String> = record.added.clone();
    out.push("-E".to_string());
    let argv = &record.argv;
    let mut i = 1;
    while i < argv.len() {
        let a = argv[i].as_str();
        let skip_next = matches!(a, "-o" | "-MF" | "-MT" | "-MQ");
        let skip = skip_next
            || matches!(a, "-c" | "-MD" | "-MMD" | "-MP" | "-M" | "-MM")
            || a.starts_with("-o")
            || a.starts_with("-MF")
            || a.starts_with("-MT")
            || a.starts_with("-MQ")
            || a.starts_with("-Wp,-M")
            || a.starts_with("-save-temps");
        if !skip {
            out.push(a.to_string());
        }
        i += if skip_next { 2 } else { 1 };
    }
    out
}

/// Read one unit's features.
pub fn read_unit(unit: &Unit, record: &CompileRecord) -> Result<Features, String> {
    let text = if unit.preprocess {
        let out = Command::new(&record.compiler)
            .args(replay_args(record))
            .current_dir(&record.cwd)
            .stdin(Stdio::null())
            .output()
            .map_err(|e| format!("cannot run {}: {e}", record.compiler))?;
        if !out.status.success() {
            let err = String::from_utf8_lossy(&out.stderr);
            return Err(err.lines().next().unwrap_or("failed").to_string());
        }
        String::from_utf8_lossy(&out.stdout).into_owned()
    } else {
        let path = Path::new(&record.cwd).join(&unit.source);
        std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?
    };
    Ok(if unit.asm {
        scan::asm_unit(&text)
    } else {
        scan::c(&text)
    })
}

/// What a run found, before it is named.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Found {
    /// The units.
    pub units: usize,
    /// The assembly units among them.
    pub asm_units: usize,
    /// The units whose text was read.
    pub read: usize,
    /// Units whose text could not be made, with the reason.
    pub unread: Vec<(String, String)>,
    /// Lines of the log that did not parse.
    pub skipped_lines: usize,
    /// (kind, name) to (units, uses).
    pub features: BTreeMap<(String, String), (usize, usize)>,
}

/// Count the features of the records. With `flags_only`, no unit is replayed.
#[must_use]
pub fn count(records: &[CompileRecord], jobs: usize, flags_only: bool) -> Found {
    let units = units(records);
    let mut found = Found {
        units: units.len(),
        asm_units: units.iter().filter(|u| u.asm).count(),
        ..Found::default()
    };
    let mut merge = |f: Features| {
        for (key, n) in f {
            let e = found.features.entry(key).or_default();
            e.0 += 1;
            e.1 += n;
        }
    };
    for u in &units {
        let mut f = Features::new();
        for flag in flags(&records[u.record].argv) {
            *f.entry((kind::FLAG.to_string(), flag)).or_default() += 1;
        }
        merge(f);
    }
    if flags_only {
        return found;
    }
    let tasks: Vec<harness_agent::pool::Task<'_, Result<Features, String>>> = units
        .iter()
        .map(|u| {
            let record = &records[u.record];
            Box::new(move || read_unit(u, record)) as harness_agent::pool::Task<'_, _>
        })
        .collect();
    let results = harness_agent::pool::run(jobs, tasks);
    let mut unread = Vec::new();
    let mut read = 0;
    for (u, result) in units.iter().zip(results) {
        match result {
            Ok(f) => {
                read += 1;
                merge(f);
            }
            Err(e) => unread.push((
                crate::census::absolute(&records[u.record].cwd, &u.source),
                e,
            )),
        }
    }
    found.read = read;
    found.unread = unread;
    found
}

impl Found {
    /// Name the result.
    #[must_use]
    pub fn census(&self, pin: &str, row: &str, conf: &str, log_sha256: &str) -> Census {
        let mut feature: Vec<Feature> = self
            .features
            .iter()
            .map(|((kind, name), (units, uses))| Feature {
                kind: kind.clone(),
                name: name.clone(),
                units: *units,
                uses: *uses,
            })
            .collect();
        feature.sort_by(|a, b| {
            a.kind
                .cmp(&b.kind)
                .then(b.units.cmp(&a.units))
                .then(a.name.cmp(&b.name))
        });
        Census {
            pin: pin.to_string(),
            row: row.to_string(),
            conf: conf.to_string(),
            log_sha256: log_sha256.to_string(),
            units: self.units,
            read: self.read,
            feature,
        }
    }

    /// The result as markdown, the first `limit` features of each kind.
    #[must_use]
    pub fn report(&self, title: &str, flags_only: bool, limit: usize) -> String {
        let mut s = String::new();
        let _ = writeln!(s, "# Demands of {title}\n");
        let _ = writeln!(
            s,
            "Units: {} ({} C, {} assembly). Units read: {}.{}",
            self.units,
            self.units - self.asm_units,
            self.asm_units,
            self.read,
            if flags_only {
                " Only the command lines were read."
            } else {
                ""
            }
        );
        if self.skipped_lines > 0 {
            let _ = writeln!(
                s,
                "\nLines of the log that did not parse, which are not counted: {}.",
                self.skipped_lines
            );
        }
        let _ = writeln!(
            s,
            "\nrucc's state for each feature needs `rucc --print-features`, which rucc does not have yet."
        );
        let mut by_kind: BTreeMap<&str, Vec<(&str, usize, usize)>> = BTreeMap::new();
        for ((kind, name), (units, uses)) in &self.features {
            by_kind.entry(kind).or_default().push((name, *units, *uses));
        }
        for (kind, mut list) in by_kind {
            list.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
            let _ = writeln!(
                s,
                "\n## {kind}\n\nDistinct: {}.\n\n| name | units | uses |\n|---|---|---|",
                list.len()
            );
            for (name, units, uses) in list.iter().take(limit) {
                let _ = writeln!(s, "| `{}` | {units} | {uses} |", name.replace('|', "\\|"));
            }
            if list.len() > limit {
                let _ = writeln!(s, "\nNot listed: {} more.", list.len() - limit);
            }
        }
        if !self.unread.is_empty() {
            let _ = writeln!(
                s,
                "\n## Units not read\n\nUnits: {}.\n\n| unit | reason |\n|---|---|",
                self.unread.len()
            );
            for (unit, why) in self.unread.iter().take(limit) {
                let _ = writeln!(s, "| `{unit}` | {} |", why.replace('|', "\\|"));
            }
            if self.unread.len() > limit {
                let _ = writeln!(s, "\nNot listed: {} more.", self.unread.len() - limit);
            }
        }
        s
    }
}

impl DemandsFile {
    /// Parse the text of `demands.toml`.
    pub fn parse(text: &str) -> Result<Self, String> {
        toml::from_str(text).map_err(|e| format!("{FILE_NAME}: {e}"))
    }

    /// Put `census` in place of the census of the same pin, row and configuration.
    pub fn put(&mut self, census: Census) {
        self.census
            .retain(|c| (&c.pin, &c.row, &c.conf) != (&census.pin, &census.row, &census.conf));
        self.census.push(census);
        self.census
            .sort_by(|a, b| (&a.pin, &a.row, &a.conf).cmp(&(&b.pin, &b.row, &b.conf)));
    }

    /// The text of `demands.toml`.
    #[must_use]
    pub fn render(&self) -> String {
        let q = |s: &str| toml::Value::String(s.to_string()).to_string();
        let mut s = HEADER.to_string();
        for c in &self.census {
            let _ = writeln!(
                s,
                "\n[[census]]\npin = {}\nrow = {}\nconf = {}\nlog_sha256 = {}\nunits = {}\nread = {}\nfeature = [",
                q(&c.pin),
                q(&c.row),
                q(&c.conf),
                q(&c.log_sha256),
                c.units,
                c.read
            );
            for f in &c.feature {
                let _ = writeln!(
                    s,
                    "  {{ kind = {}, name = {}, units = {}, uses = {} }},",
                    q(&f.kind),
                    q(&f.name),
                    f.units,
                    f.uses
                );
            }
            s.push_str("]\n");
        }
        s
    }
}

/// What `rb demands` was asked.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Ask<'a> {
    /// The `compile.jsonl`.
    pub log: &'a str,
    /// The pin, row and configuration, which `--write` needs.
    pub names: Option<(&'a str, &'a str, &'a str)>,
    /// Whether to update `demands.toml`.
    pub write: bool,
    /// Whether to read the command lines only.
    pub flags_only: bool,
    /// How many units are replayed at once.
    pub jobs: usize,
    /// How many lines each list of the report gets.
    pub limit: usize,
}

/// Run `rb demands`. Returns the report.
pub fn run(ask: &Ask<'_>) -> Result<String, String> {
    let log = Path::new(ask.log);
    let repo = if ask.write {
        Some(crate::repo::Repo::find()?)
    } else {
        None
    };
    if let (Some(repo), Some((pin, row, _))) = (&repo, ask.names) {
        let pins = rb_pins::pins::PinsFile::load(&repo.root)?
            .ok_or_else(|| format!("no {} yet", rb_pins::pins::FILE_NAME))?;
        if pins.find(pin).is_none() {
            return Err(format!("no pin {pin} in {}", rb_pins::pins::FILE_NAME));
        }
        if !rb_pins::places::Rows::load(&repo.root)?
            .row
            .contains_key(row)
        {
            return Err(format!("no row {row} in {}", rb_pins::places::ROWS));
        }
    }
    let (records, skipped) = read_log(log).map_err(|e| format!("{}: {e}", log.display()))?;
    let mut found = count(&records, ask.jobs, ask.flags_only);
    found.skipped_lines = skipped;
    let title = match ask.names {
        Some((pin, row, conf)) => format!("{pin}, row {row}, {conf}"),
        None => ask.log.to_string(),
    };
    let mut report = found.report(&title, ask.flags_only, ask.limit);
    if let Some(repo) = repo {
        let (pin, row, conf) = ask.names.ok_or("--write needs --pin, --row and --conf")?;
        let path = repo.root.join(FILE_NAME);
        let mut file = match std::fs::read_to_string(&path) {
            Ok(text) => DemandsFile::parse(&text)?,
            Err(_) => DemandsFile::default(),
        };
        let digest = rb_pins::fetch::sha256(log)?;
        file.put(found.census(pin, row, conf, &digest));
        std::fs::write(&path, file.render()).map_err(|e| format!("{}: {e}", path.display()))?;
        let _ = writeln!(report, "\nWrote {}.", path.display());
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use harness_shim::record::parse_log;

    fn argv(line: &str) -> Vec<String> {
        line.split_whitespace().map(String::from).collect()
    }

    #[test]
    fn flags_are_counted_without_paths() {
        let got = flags(&argv(
            "cc -O2 -pipe -fno-common -fdebug-prefix-map=/usr/src=/x -march=x86-64 -mno-sse -Wall -Werror=format \
             -Wl,-soname,libc.so.7 -Wa,--noexecstack --param ssp-buffer-size=4 --param=max-inline-insns-single=1 \
             -std=gnu99 -MD -MF a.d -c a.c -o a.o",
        ));
        assert_eq!(
            got,
            [
                "-fno-common",
                "-fdebug-prefix-map=PATH",
                "-march=x86-64",
                "-mno-sse",
                "-Wall",
                "-Werror=format",
                "-Wl,-soname",
                "-Wa,--noexecstack",
                "--param ssp-buffer-size=4",
                "--param max-inline-insns-single=1",
                "-std=gnu99",
            ]
        );
    }

    #[test]
    fn the_replay_prints_instead_of_compiling() {
        let mut r = CompileRecord::new(
            argv(
                "rb-cc -O2 -MD -MF .depend.a.o -MT a.o -Wp,-MD,x -c a.c -o a.o -oa2.o -save-temps",
            ),
            "/gcc".into(),
            "/o".into(),
        );
        r.added = vec!["--target=x".into()];
        assert_eq!(replay_args(&r), ["--target=x", "-E", "-O2", "a.c"]);
    }

    const LOG: &str = r#"{"started":1,"argv":["rb-cc","-O2","-fno-common","-c","a.c","-o","a.o"],"compiler":"/gcc","cwd":"/o","inputs":[{"path":"a.c","sha256":"x"}],"outputs":[{"path":"a.o","sha256":"y"}],"wall-seconds":0.1,"exit":0,"slot":"cc","side":"G","origin":"ref"}
{"started":1,"argv":["rb-cc","-fno-common","-c","b.S"],"compiler":"/gcc","cwd":"/o","inputs":[{"path":"b.S","sha256":"x"}],"wall-seconds":0.1,"exit":0,"slot":"cc","side":"G","origin":"ref"}
{"started":1,"argv":["rb-c++","-c","c.cc"],"compiler":"/g++","cwd":"/o","inputs":[{"path":"c.cc","sha256":"x"}],"wall-seconds":0.1,"exit":0,"slot":"c++","side":"G","origin":"ref-cxx"}
{"started":1,"argv":["rb-cc","-E","-dM","-"],"compiler":"/gcc","cwd":"/o","wall-seconds":0.1,"exit":0,"probe":true,"slot":"cc","side":"G"}
{"started":1,"argv":["rb-cc","-c","d.c"],"compiler":"/gcc","cwd":"/o","inputs":[{"path":"d.c","sha256":"x"}],"wall-seconds":0.1,"exit":1,"slot":"cc","side":"G","origin":"ref"}
"#;

    #[test]
    fn only_the_c_slot_units_that_built_are_counted() {
        let records = parse_log(LOG).0;
        let u = units(&records);
        assert_eq!(u.len(), 2);
        assert!(u[1].asm && u[1].preprocess);
        let found = count(&records, 1, true);
        assert_eq!((found.units, found.asm_units, found.read), (2, 1, 0));
        assert_eq!(
            found.features[&("flag".to_string(), "-fno-common".to_string())],
            (2, 2)
        );
        let census = found.census("freebsd-15.1", "amd64", "graded", "00");
        assert_eq!(census.feature.len(), 1);
        let report = found.report("t", true, 5);
        assert!(report.contains("| `-fno-common` | 2 | 2 |"), "{report}");
    }

    #[test]
    fn a_unit_whose_text_cannot_be_made_is_listed_and_not_counted() {
        let log = LOG.replace("\"compiler\":\"/gcc\"", "\"compiler\":\"/nonexistent/gcc\"");
        let found = count(&parse_log(&log).0, 2, false);
        assert_eq!((found.units, found.read), (2, 0));
        assert_eq!(found.unread[0].0, "/o/a.c");
        assert!(
            found.unread[0].1.contains("cannot run"),
            "{:?}",
            found.unread
        );
        assert!(found.report("t", false, 1).contains("Not listed: 1 more."));
    }

    #[test]
    fn the_file_keeps_one_census_per_pin_row_and_conf_and_reads_back() {
        let one = |pin: &str, units| Census {
            pin: pin.into(),
            row: "amd64".into(),
            conf: "graded".into(),
            log_sha256: "ab".into(),
            units,
            read: units,
            feature: vec![Feature {
                kind: "attribute".into(),
                name: "section(set_)".into(),
                units,
                uses: 3,
            }],
        };
        let mut file = DemandsFile::default();
        file.put(one("netbsd-trunk", 1));
        file.put(one("freebsd-15.1", 2));
        file.put(one("netbsd-trunk", 5));
        assert_eq!(file.census.len(), 2);
        assert_eq!(file.census[0].pin, "freebsd-15.1");
        assert_eq!(file.census[1].units, 5);
        let text = file.render();
        assert!(text.contains(
            "  { kind = \"attribute\", name = \"section(set_)\", units = 5, uses = 3 },"
        ));
        assert_eq!(DemandsFile::parse(&text).unwrap(), file);
    }
}
