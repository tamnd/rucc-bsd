//! `rb baseline`: the reference results of one pin, row and configuration (plan 12.5).
//!
//! G is booted and tested several times, three by default, each time on a fresh overlay of the
//! same image. A case is a stable pass only if it passed in every run. Every other case is
//! `g-unstable`, and it is never graded (12.1).
//!
//! The command writes `baselines/<system>/<row>/<version>/<conf>/`. `baselines/README.md` gives
//! the format of each file:
//! - `baseline.toml`: what was run, the image hash, and per run its outcome and counts;
//! - `cases.tsv`: per case, the result and the duration of each run, and the verdict;
//! - `stable-pass.txt`: the cases that passed in every run;
//! - `kernel-log.txt`: the kernel messages of the runs, as the allowed set of 12.1.
//!
//! The runs come from `rb boot`, or with `--from` from the output directories of earlier boots.
//! The second way lets a workflow boot in separate jobs and write the baseline at the end.

use crate::boot::{self, Outcome};
use crate::cases::{self, Case};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

/// One run, read back from the output directory of `rb boot`.
#[derive(Debug, Clone)]
pub struct Run {
    /// The directory.
    pub dir: PathBuf,
    /// The outcome rb boot wrote to `boot.json`.
    pub outcome: Outcome,
    /// Every case of the run.
    pub cases: Vec<Case>,
    /// The kernel messages, from `dmesg.end` or else `dmesg.boot`, as [`cases::kernel_line`]
    /// gives them.
    pub kernel: BTreeSet<String>,
}

/// Read one run from the output directory of `rb boot`.
pub fn read_run(dir: &Path) -> Result<Run, String> {
    let file = dir.join("boot.json");
    let text = std::fs::read_to_string(&file).map_err(|e| format!("{}: {e}", file.display()))?;
    let outcome: Outcome =
        serde_json::from_str(&text).map_err(|e| format!("{}: {e}", file.display()))?;
    let boot = if outcome.passed() {
        "passed"
    } else {
        outcome.result.as_str()
    };
    let results = dir.join("results");
    let cases = cases::read_run(boot, &results)?;
    let mut kernel = BTreeSet::new();
    for name in ["dmesg.end", "dmesg.boot"] {
        if let Ok(bytes) = std::fs::read(results.join(name)) {
            kernel.extend(
                String::from_utf8_lossy(&bytes)
                    .lines()
                    .map(cases::kernel_line)
                    .filter(|l| !l.is_empty()),
            );
            break;
        }
    }
    Ok(Run {
        dir: dir.to_path_buf(),
        outcome,
        cases,
        kernel,
    })
}

/// What a baseline is of.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
pub struct Meta {
    /// The pin, such as `freebsd-15.1`.
    pub pin: String,
    /// `freebsd` or `netbsd`.
    pub system: String,
    /// The version the tree says it is, if the pin is in pins.toml.
    pub version: String,
    /// The pinned commit, if there is one.
    pub commit: String,
    /// The row, such as `amd64`.
    pub row: String,
    /// The build configuration, such as `graded`.
    pub conf: String,
    /// When the baseline was written.
    pub made: String,
    /// The image's file name.
    pub image: String,
    /// The image's SHA-256, so that a rebuild of G can be checked to be the same.
    pub image_sha256: String,
}

impl Meta {
    /// The directory under `baselines/`: `<system>/<row>/<version>/<conf>`. The version part is
    /// the pin name without the system, such as `15.1` for `freebsd-15.1`.
    #[must_use]
    pub fn place(&self) -> PathBuf {
        let short = self
            .pin
            .strip_prefix(&format!("{}-", self.system))
            .unwrap_or(&self.pin);
        [&self.system, &self.row, short, &self.conf]
            .iter()
            .collect()
    }
}

/// `stable-pass` or `g-unstable`.
#[must_use]
pub fn verdict(results: &[Option<&str>]) -> &'static str {
    if !results.is_empty() && results.iter().all(|r| *r == Some("passed")) {
        "stable-pass"
    } else {
        "g-unstable"
    }
}

/// Each case of any run, with its result and duration in each run. A case a run did not report
/// is `None` there.
type Table<'a> = BTreeMap<(&'a str, &'a str), Vec<Option<&'a Case>>>;

fn table(runs: &[Run]) -> Table<'_> {
    let mut t: Table = BTreeMap::new();
    for (i, run) in runs.iter().enumerate() {
        for c in &run.cases {
            let row = t
                .entry((c.suite.as_str(), c.name.as_str()))
                .or_insert_with(|| vec![None; runs.len()]);
            // A name twice in one run keeps the worse result, so a pass is never made up.
            if row[i].is_none_or(|old| old.result == "passed") {
                row[i] = Some(c);
            }
        }
    }
    t
}

#[derive(Serialize)]
struct RunEntry {
    n: usize,
    result: String,
    passed: bool,
    seconds: f64,
    firmware: String,
    accel: String,
    restarts: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    panic: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    panic_case: Option<String>,
    cases: usize,
    counts: BTreeMap<String, BTreeMap<String, u64>>,
}

#[derive(Serialize)]
struct File<'a> {
    #[serde(flatten)]
    meta: &'a Meta,
    suites: Vec<String>,
    runs: usize,
    complete: bool,
    cases: usize,
    stable_pass: usize,
    g_unstable: usize,
    run: Vec<RunEntry>,
}

/// What a baseline came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Written {
    /// The directory it was written to.
    pub dir: PathBuf,
    /// Whether every run passed, so the baseline can be used.
    pub complete: bool,
    /// The cases seen in any run.
    pub cases: usize,
    /// The cases that passed in every run.
    pub stable_pass: usize,
}

fn seconds(c: Option<&Case>) -> String {
    match c.and_then(|c| c.seconds) {
        Some(s) => format!("{s:.3}"),
        None => "-".into(),
    }
}

fn cases_tsv(runs: &[Run], t: &Table) -> (String, String, usize) {
    let mut tsv = String::from("# suite\tcase");
    for n in 1..=runs.len() {
        let _ = write!(tsv, "\trun-{n}");
    }
    for n in 1..=runs.len() {
        let _ = write!(tsv, "\tseconds-{n}");
    }
    tsv.push_str("\tverdict\n");
    let mut stable = String::new();
    let mut count = 0;
    for ((suite, name), row) in t {
        let results: Vec<Option<&str>> = row.iter().map(|c| c.map(|c| c.result.as_str())).collect();
        let v = verdict(&results);
        let _ = write!(tsv, "{suite}\t{name}");
        for r in &results {
            let _ = write!(tsv, "\t{}", r.unwrap_or("missing"));
        }
        for c in row {
            let _ = write!(tsv, "\t{}", seconds(*c));
        }
        let _ = writeln!(tsv, "\t{v}");
        if v == "stable-pass" {
            let _ = writeln!(stable, "{suite}\t{name}");
            count += 1;
        }
    }
    (tsv, stable, count)
}

fn kernel_txt(runs: &[Run]) -> String {
    let mut seen: BTreeMap<&str, usize> = BTreeMap::new();
    for run in runs {
        for line in &run.kernel {
            *seen.entry(line.as_str()).or_default() += 1;
        }
    }
    let mut s = format!(
        "# The kernel messages of {} runs, with the numbers that change replaced by #.\n# runs\tmessage\n",
        runs.len()
    );
    for (line, n) in seen {
        let _ = writeln!(s, "{n}\t{line}");
    }
    s
}

/// Write the baseline of `runs` to `top/<place>`, where `top` is the `baselines` directory.
pub fn write(top: &Path, meta: &Meta, runs: &[Run]) -> Result<Written, String> {
    let dir = top.join(meta.place());
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let t = table(runs);
    let (tsv, stable, stable_pass) = cases_tsv(runs, &t);
    let complete = !runs.is_empty() && runs.iter().all(|r| r.outcome.passed());
    let entries = runs
        .iter()
        .enumerate()
        .map(|(i, r)| RunEntry {
            n: i + 1,
            result: r.outcome.result.clone(),
            passed: r.outcome.passed(),
            seconds: (r.outcome.seconds * 10.0).round() / 10.0,
            firmware: r.outcome.firmware.clone(),
            accel: r.outcome.accel.clone(),
            restarts: r.outcome.restarts,
            panic: r.outcome.seen.panic.clone(),
            panic_case: r.outcome.seen.panic_case.clone(),
            cases: r.cases.len(),
            counts: cases::counts(&r.cases),
        })
        .collect();
    let file = File {
        meta,
        suites: runs
            .first()
            .map(|r| r.outcome.asked.clone())
            .unwrap_or_default(),
        runs: runs.len(),
        complete,
        cases: t.len(),
        stable_pass,
        g_unstable: t.len() - stable_pass,
        run: entries,
    };
    let toml = toml::to_string(&file).map_err(|e| format!("baseline.toml: {e}"))?;
    let head = "# Written by rb baseline (plan 12.5). Do not edit by hand. baselines/README.md has the format.\n\n";
    for (name, text) in [
        ("baseline.toml", format!("{head}{toml}")),
        ("cases.tsv", tsv),
        ("stable-pass.txt", stable),
        ("kernel-log.txt", kernel_txt(runs)),
    ] {
        let path = dir.join(name);
        std::fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))?;
    }
    Ok(Written {
        dir,
        complete,
        cases: t.len(),
        stable_pass,
    })
}

/// The report of a baseline, as markdown.
#[must_use]
pub fn report(meta: &Meta, runs: &[Run], w: &Written) -> String {
    let mut s = format!("# rb baseline: {} {} {}\n\n", meta.pin, meta.row, meta.conf);
    let _ = writeln!(s, "| run | result | seconds | cases | passed | directory |");
    let _ = writeln!(s, "|---|---|---|---|---|---|");
    for (i, r) in runs.iter().enumerate() {
        let passed = r.cases.iter().filter(|c| c.result == "passed").count();
        let _ = writeln!(
            s,
            "| {} | {} | {:.0} | {} | {passed} | {} |",
            i + 1,
            r.outcome.result,
            r.outcome.seconds,
            r.cases.len(),
            r.dir.display()
        );
    }
    let _ = writeln!(
        s,
        "\n{} of {} cases passed in every run. The baseline is in {}.",
        w.stable_pass,
        w.cases,
        w.dir.display()
    );
    if !w.complete {
        s.push_str("\nNot every run passed, so this baseline is not complete. Fix the rig and run it again.\n");
    }
    s
}

/// Boot the image `runs` times, each with its output in `work/run-N`, and read the runs back.
/// `request` gives the boot request for one output directory.
pub fn boot_runs(
    runs: usize,
    work: &Path,
    request: impl Fn(&Path) -> Result<boot::Request, String>,
) -> Result<Vec<Run>, String> {
    let mut out = Vec::new();
    for n in 1..=runs {
        let dir = work.join(format!("run-{n}"));
        let req = request(&dir)?;
        eprintln!("rb baseline: run {n} of {runs} in {}", dir.display());
        let o = boot::run(&req)?;
        eprintln!("rb baseline: run {n}: {} in {:.0} s", o.result, o.seconds);
        out.push(read_run(&dir)?);
    }
    Ok(out)
}

/// The command line of `rb baseline`.
pub mod cli {
    use super::{Meta, Run, boot_runs, read_run, report, write};
    use crate::boot::{cli as boot_cli, kvm_usable};
    use crate::cli::Args;
    use rb_pins::pins::PinsFile;
    use rb_pins::places::Rows;
    use std::fmt::Write as _;
    use std::path::{Path, PathBuf};

    /// The suites a baseline runs by default: all of 12.2 that rb-rc has.
    pub const SUITES: &str = "boot,smoke,tests";

    /// What the command line asks for, apart from the boots.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct Plan {
        /// What the baseline is of. The image hash is filled in later.
        pub meta: Meta,
        /// The image to boot, or to hash with `--from`.
        pub image: Option<PathBuf>,
        /// The directories of earlier boots.
        pub from: Vec<PathBuf>,
        /// How many runs.
        pub runs: usize,
        /// The `baselines` directory.
        pub top: PathBuf,
        /// Where the boots write.
        pub work: PathBuf,
    }

    fn word(v: &str, what: &str) -> Result<String, String> {
        if v.is_empty()
            || !v
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '.' | '_'))
        {
            return Err(format!(
                "{what} {v} must be letters, digits, '-', '.' or '_'"
            ));
        }
        Ok(v.to_string())
    }

    /// Read the command line. `root` is the repository and `pins` its pins.toml, if any.
    pub fn plan(args: &Args, root: &Path, pins: Option<&PinsFile>) -> Result<Plan, String> {
        let pin = word(
            args.target
                .as_deref()
                .ok_or("give the pin, such as rb baseline freebsd-15.1")?,
            "the pin",
        )?;
        let found = pins.and_then(|p| p.find(&pin));
        let system = match (args.get("system"), found) {
            (Some(s), _) => s.to_string(),
            (None, Some(p)) => p.system.clone(),
            (None, None) => {
                return Err(format!(
                    "{pin} is not in pins.toml; give --system freebsd|netbsd"
                ));
            }
        };
        if system != "freebsd" && system != "netbsd" {
            return Err(format!("--system {system}: give freebsd or netbsd"));
        }
        let row = word(args.get("row").unwrap_or("amd64"), "the row")?;
        let conf = word(
            args.get("conf")
                .ok_or("give --conf C, such as --conf graded")?,
            "the conf",
        )?;
        let runs: usize = match args.get("runs") {
            Some(v) => v
                .parse()
                .map_err(|_| format!("--runs {v} is not a number"))?,
            None => 3,
        };
        let from: Vec<PathBuf> = args
            .get("from")
            .map(|v| {
                v.split(',')
                    .filter(|s| !s.is_empty())
                    .map(PathBuf::from)
                    .collect()
            })
            .unwrap_or_default();
        let image = args.get("image").map(PathBuf::from);
        if from.is_empty() && image.is_none() {
            return Err(
                "give --image IMG to boot, or --from DIR,DIR,DIR with earlier boots".into(),
            );
        }
        if runs == 0 {
            return Err("--runs must be 1 or more".into());
        }
        if !from.is_empty() && args.get("runs").is_some() && from.len() != runs {
            return Err(format!(
                "--runs {runs}, but --from has {} directories",
                from.len()
            ));
        }
        let meta = Meta {
            pin: pin.clone(),
            system,
            version: found.map(|p| p.version.clone()).unwrap_or_default(),
            commit: found.and_then(|p| p.commit.clone()).unwrap_or_default(),
            row,
            conf,
            made: crate::sets::now(),
            image: String::new(),
            image_sha256: String::new(),
        };
        let top = args
            .get("dir")
            .map_or_else(|| root.join("baselines"), PathBuf::from);
        let work = args.get("work").map_or_else(
            || root.join("work/baseline").join(meta.place()),
            PathBuf::from,
        );
        Ok(Plan {
            runs: if from.is_empty() { runs } else { from.len() },
            meta,
            image,
            from,
            top,
            work,
        })
    }

    /// The `rb boot` arguments of one run: the baseline's own, with `--out` set, and the suites
    /// all of them unless `--suite` says otherwise.
    fn boot_args(args: &Args, out: &Path) -> Args {
        let mut a = args.clone();
        a.command = "boot".into();
        a.target = None;
        for k in ["conf", "runs", "from", "dir", "work", "system"] {
            a.values.remove(k);
        }
        a.values.insert("out".into(), out.display().to_string());
        a.values
            .entry("suite".into())
            .or_insert_with(|| SUITES.into());
        a
    }

    /// Run `rb baseline`, and return the report and whether the baseline is complete.
    pub fn run(
        args: &Args,
        root: &Path,
        pins: Option<&PinsFile>,
        rows: &Rows,
    ) -> Result<(String, bool), String> {
        let mut plan = plan(args, root, pins)?;
        if args.has("dry-run") {
            let mut s = format!(
                "# rb baseline: a dry run\n\nThe baseline goes to {}.\n\n",
                plan.top.join(plan.meta.place()).display()
            );
            if plan.from.is_empty() {
                let one = boot_args(args, &plan.work.join("run-1"));
                let req = boot_cli::request(&one, rows, kvm_usable)?;
                let _ = writeln!(
                    s,
                    "{} boots, in {}/run-1 to run-{}, each like this:\n\n```\n{}\n```",
                    plan.runs,
                    plan.work.display(),
                    plan.runs,
                    crate::boot::qemu_command(&req).join(" ")
                );
            } else {
                for d in &plan.from {
                    let _ = writeln!(s, "- a run from {}", d.display());
                }
            }
            return Ok((s, true));
        }
        let runs: Vec<Run> = if plan.from.is_empty() {
            boot_runs(plan.runs, &plan.work, |out| {
                boot_cli::request(&boot_args(args, out), rows, kvm_usable)
            })?
        } else {
            plan.from
                .iter()
                .map(|d| read_run(d))
                .collect::<Result<_, _>>()?
        };
        let image = plan
            .image
            .clone()
            .or_else(|| runs.first().map(|r| PathBuf::from(&r.outcome.image)));
        if let Some(image) = image.filter(|p| p.is_file()) {
            plan.meta.image = image
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            plan.meta.image_sha256 = rb_pins::fetch::sha256(&image)?;
        }
        let w = write(&plan.top, &plan.meta, &runs)?;
        Ok((report(&plan.meta, &runs, &w), w.complete))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::{Args, accepted};

    fn case(suite: &str, name: &str, result: &str, seconds: Option<f64>) -> Case {
        Case {
            suite: suite.into(),
            name: name.into(),
            result: result.into(),
            seconds,
        }
    }

    fn run(result: &str, cases: Vec<Case>, kernel: &[&str]) -> Run {
        Run {
            dir: PathBuf::from("run"),
            outcome: Outcome {
                result: result.into(),
                firmware: "ovmf".into(),
                accel: "kvm".into(),
                asked: vec!["boot".into(), "tests".into()],
                seconds: 61.26,
                ..Outcome::default()
            },
            cases,
            kernel: kernel.iter().map(|s| (*s).to_string()).collect(),
        }
    }

    fn meta() -> Meta {
        Meta {
            pin: "freebsd-15.1".into(),
            system: "freebsd".into(),
            version: "15.1-RELEASE".into(),
            commit: "abc".into(),
            row: "amd64".into(),
            conf: "graded".into(),
            made: "2026-11-09T00:00:00Z".into(),
            image: "g.img".into(),
            image_sha256: "00ff".into(),
        }
    }

    #[test]
    fn verdicts() {
        assert_eq!(verdict(&[Some("passed"); 3]), "stable-pass");
        assert_eq!(
            verdict(&[Some("passed"), None, Some("passed")]),
            "g-unstable"
        );
        assert_eq!(verdict(&[Some("passed"), Some("skipped")]), "g-unstable");
        assert_eq!(verdict(&[Some("failed"); 3]), "g-unstable");
        assert_eq!(verdict(&[]), "g-unstable");
        assert_eq!(meta().place(), Path::new("freebsd/amd64/15.1/graded"));
    }

    #[test]
    fn three_runs() {
        let boot = || case("boot", "boot", "passed", None);
        let runs = vec![
            run(
                "pass",
                vec![
                    boot(),
                    case("tests", "bin/sh/t:a", "passed", Some(0.5)),
                    case("tests", "bin/sh/t:b", "passed", Some(1.0)),
                    case("tests", "bin/sh/t:c", "skipped", Some(0.0)),
                ],
                &["vtnet0: link state changed to UP", "rare message"],
            ),
            run(
                "pass",
                vec![
                    boot(),
                    case("tests", "bin/sh/t:a", "passed", Some(0.25)),
                    case("tests", "bin/sh/t:b", "failed", Some(2.0)),
                    case("tests", "bin/sh/t:c", "skipped", None),
                ],
                &["vtnet0: link state changed to UP"],
            ),
            run(
                "pass",
                vec![boot(), case("tests", "bin/sh/t:a", "passed", Some(0.75))],
                &["vtnet0: link state changed to UP"],
            ),
        ];
        let top = std::env::temp_dir().join(format!("rb-baseline-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&top);
        let w = write(&top, &meta(), &runs).unwrap();
        assert_eq!((w.complete, w.cases, w.stable_pass), (true, 4, 2));
        assert_eq!(w.dir, top.join("freebsd/amd64/15.1/graded"));
        let read = |name: &str| std::fs::read_to_string(w.dir.join(name)).unwrap();
        assert_eq!(read("stable-pass.txt"), "boot\tboot\ntests\tbin/sh/t:a\n");
        let tsv = read("cases.tsv");
        let lines: Vec<&str> = tsv.lines().collect();
        assert_eq!(
            lines[0],
            "# suite\tcase\trun-1\trun-2\trun-3\tseconds-1\tseconds-2\tseconds-3\tverdict"
        );
        assert_eq!(
            lines[1],
            "boot\tboot\tpassed\tpassed\tpassed\t-\t-\t-\tstable-pass"
        );
        assert_eq!(
            lines[2],
            "tests\tbin/sh/t:a\tpassed\tpassed\tpassed\t0.500\t0.250\t0.750\tstable-pass"
        );
        assert_eq!(
            lines[3],
            "tests\tbin/sh/t:b\tpassed\tfailed\tmissing\t1.000\t2.000\t-\tg-unstable"
        );
        assert_eq!(
            lines[4],
            "tests\tbin/sh/t:c\tskipped\tskipped\tmissing\t0.000\t-\t-\tg-unstable"
        );
        let kernel = read("kernel-log.txt");
        assert!(kernel.contains("\n1\trare message\n3\tvtnet0: link state changed to UP\n"));
        let toml_text = read("baseline.toml");
        let v: toml::Value = toml::from_str(&toml_text).unwrap();
        assert_eq!(v["pin"].as_str(), Some("freebsd-15.1"));
        assert_eq!(v["image_sha256"].as_str(), Some("00ff"));
        assert_eq!(v["complete"].as_bool(), Some(true));
        assert_eq!(v["stable_pass"].as_integer(), Some(2));
        assert_eq!(v["g_unstable"].as_integer(), Some(2));
        let second = &v["run"].as_array().unwrap()[1];
        assert_eq!(second["seconds"].as_float(), Some(61.3));
        assert_eq!(second["counts"]["tests"]["failed"].as_integer(), Some(1));
        let text = report(&meta(), &runs, &w);
        assert!(text.contains("2 of 4 cases passed in every run."), "{text}");

        // A run that panicked makes the baseline incomplete, and its cases are missing.
        let mut runs = runs;
        runs[2] = run("panic", vec![case("boot", "boot", "panic", None)], &[]);
        let w = write(&top, &meta(), &runs).unwrap();
        assert_eq!((w.complete, w.stable_pass), (false, 0));
        assert!(report(&meta(), &runs, &w).contains("not complete"));
        std::fs::remove_dir_all(&top).unwrap();
    }

    #[test]
    fn plans() {
        let pins = rb_pins::pins::PinsFile::parse(include_str!("../../../pins.toml")).unwrap();
        let root = Path::new("/r");
        let parse = |words: &[&str]| {
            let words: Vec<String> = words.iter().map(|w| (*w).to_string()).collect();
            Args::parse(&words, accepted).unwrap()
        };
        let p = cli::plan(
            &parse(&[
                "baseline",
                "freebsd-15.1",
                "--conf",
                "graded",
                "--image",
                "g.img",
            ]),
            root,
            Some(&pins),
        )
        .unwrap();
        assert_eq!(p.runs, 3);
        assert_eq!(p.meta.system, "freebsd");
        assert!(p.meta.version.starts_with("15.1"), "{}", p.meta.version);
        assert_eq!(p.top, Path::new("/r/baselines"));
        assert_eq!(
            p.work,
            Path::new("/r/work/baseline/freebsd/amd64/15.1/graded")
        );
        let p = cli::plan(
            &parse(&[
                "baseline",
                "netbsd-trunk",
                "--conf",
                "graded",
                "--from",
                "a,b",
            ]),
            root,
            Some(&pins),
        )
        .unwrap();
        assert_eq!((p.runs, p.meta.system.as_str()), (2, "netbsd"));
        assert_eq!(p.from, [PathBuf::from("a"), PathBuf::from("b")]);
        for (words, want) in [
            (
                &["baseline", "--conf", "graded", "--image", "g"][..],
                "give the pin",
            ),
            (
                &["baseline", "freebsd-15.1", "--image", "g"][..],
                "give --conf",
            ),
            (
                &["baseline", "freebsd-15.1", "--conf", "graded"][..],
                "give --image",
            ),
            (
                &["baseline", "nope", "--conf", "graded", "--image", "g"][..],
                "not in pins.toml",
            ),
            (
                &["baseline", "freebsd-15.1", "--conf", "a/b", "--image", "g"][..],
                "must be letters",
            ),
            (
                &[
                    "baseline",
                    "freebsd-15.1",
                    "--conf",
                    "g",
                    "--from",
                    "a",
                    "--runs",
                    "3",
                ][..],
                "--from has 1",
            ),
        ] {
            let err = cli::plan(&parse(words), root, Some(&pins)).unwrap_err();
            assert!(err.contains(want), "{words:?}: {err}");
        }
    }
}
