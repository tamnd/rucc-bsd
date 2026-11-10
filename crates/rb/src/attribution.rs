//! `rb test`: the graded run with the 2x2 and its attribution (plan 12.6).
//!
//! Two builds give four systems:
//!
//! | | G world | R world |
//! |---|---|---|
//! | G kernel | GG, the baseline | GR |
//! | R kernel | RG | RR, the claim |
//!
//! A case is graded when GG passes it: in every GG run, and in the baseline's stable passes when
//! `--baseline` is given. Each other case is not graded. The claim is RR. When only RG is given,
//! as for the NetBSD kernel (B3), the claim is RG, and each case it fails is the kernel's; when only
//! GR is given, each case it fails is the world's.
//!
//! A graded case the claim does not pass is attributed from RG and GR:
//!
//! | RG | GR | verdict |
//! |---|---|---|
//! | fail | pass | kernel |
//! | pass | fail | world |
//! | fail | fail | both |
//! | pass | pass | interaction |
//!
//! When one of them was not run, the verdict names the two it can still be, such as `kernel or
//! both`.
//!
//! Each cell is the output directories of earlier `rb boot` commands, as `rb baseline --from`
//! takes them, or an image that `rb test` boots. GG and the claim run every suite asked for. RG
//! and GR next to an RR claim run only for the cases the claim failed: the suites of those cases,
//! and of the tests only their top-level directories.

use crate::baseline::{self, Run};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

/// The cells other than GG, in the order the report gives them.
pub const CELLS: [&str; 3] = ["RG", "GR", "RR"];

/// A case: its suite and name.
pub type Key = (String, String);

/// What GG says.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Reference {
    /// The graded cases.
    pub pass: BTreeSet<Key>,
    /// Each case GG ran, with its results, as the report gives them.
    pub text: BTreeMap<Key, String>,
}

impl Reference {
    /// GG from its runs: a case is graded when every run passed it.
    #[must_use]
    pub fn from_runs(runs: &[Run]) -> Self {
        let mut r = Self::default();
        for (key, (pass, text)) in cell(runs) {
            if pass {
                r.pass.insert(key.clone());
            }
            r.text.insert(key, text);
        }
        r
    }

    /// GG from the `cases.tsv` of a baseline: a case is graded when its verdict is
    /// `stable-pass`.
    pub fn from_baseline(tsv: &str) -> Result<Self, String> {
        let mut r = Self::default();
        for line in tsv.lines().filter(|l| !l.starts_with('#') && !l.is_empty()) {
            let fields: Vec<&str> = line.split('\t').collect();
            let (Some(suite), Some(name), Some(verdict)) =
                (fields.first(), fields.get(1), fields.last())
            else {
                return Err(format!("cases.tsv: a line with too few fields: {line}"));
            };
            let key = ((*suite).to_string(), (*name).to_string());
            if *verdict == "stable-pass" {
                r.pass.insert(key.clone());
            }
            r.text.insert(key, (*verdict).to_string());
        }
        Ok(r)
    }

    /// Only the cases both grade.
    #[must_use]
    pub fn and(mut self, other: &Self) -> Self {
        self.pass.retain(|k| other.pass.contains(k));
        for (k, t) in &other.text {
            self.text
                .entry(k.clone())
                .and_modify(|old| {
                    if old != t {
                        *old = format!("{old} {t}");
                    }
                })
                .or_insert_with(|| t.clone());
        }
        self
    }
}

/// Each case of the runs of one cell, with whether every run passed it and its results. A run
/// that did not report the case gives `not-run`. The results are one word when every run gave
/// the same, else one per run, separated by `,`.
#[must_use]
pub fn cell(runs: &[Run]) -> BTreeMap<Key, (bool, String)> {
    let t = baseline::table(runs);
    t.into_iter()
        .map(|((suite, name), row)| {
            let results: Vec<&str> = row
                .iter()
                .map(|c| c.map_or("not-run", |c| c.result.as_str()))
                .collect();
            let pass = !results.is_empty() && results.iter().all(|r| *r == "passed");
            (
                (suite.to_string(), name.to_string()),
                (pass, join(&results)),
            )
        })
        .collect()
}

fn join(results: &[&str]) -> String {
    if results.windows(2).all(|w| w[0] == w[1]) {
        results.first().copied().unwrap_or("not-run").to_string()
    } else {
        results.join(",")
    }
}

/// The verdict of a graded case the claim does not pass. `rg` and `gr` are whether that cell
/// passed it, or `None` when it was not run.
#[must_use]
pub fn verdict(claim: &str, rg: Option<bool>, gr: Option<bool>) -> &'static str {
    match claim {
        "RG" => "kernel",
        "GR" => "world",
        _ => match (rg, gr) {
            (Some(false), Some(true)) => "kernel",
            (Some(true), Some(false)) => "world",
            (Some(false), Some(false)) => "both",
            (Some(true), Some(true)) => "interaction",
            (Some(false), None) => "kernel or both",
            (Some(true), None) => "world or interaction",
            (None, Some(false)) => "world or both",
            (None, Some(true)) => "kernel or interaction",
            (None, None) => "open",
        },
    }
}

/// One graded case the claim does not pass.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    /// The case.
    pub key: Key,
    /// GG's results.
    pub gg: String,
    /// The results of RG, GR and RR, or `-` for a cell that was not given or not run.
    pub cells: [String; 3],
    /// The verdict.
    pub verdict: &'static str,
}

/// The 2x2 of one graded run.
#[derive(Debug, Clone, Default)]
pub struct Test {
    /// GG.
    pub reference: Reference,
    /// The runs of GG, when it was given as runs.
    pub gg_runs: Vec<Run>,
    /// The runs of RG, GR and RR.
    pub runs: [Option<Vec<Run>>; 3],
}

/// What the 2x2 came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attribution {
    /// The claim: RR, RG or GR.
    pub claim: &'static str,
    /// The graded cases.
    pub graded: usize,
    /// The cases GG ran and does not grade.
    pub not_graded: usize,
    /// The graded cases the claim passes.
    pub passed: usize,
    /// The graded cases the claim does not pass.
    pub rows: Vec<Row>,
}

impl Attribution {
    /// How many cases have each verdict.
    #[must_use]
    pub fn verdicts(&self) -> BTreeMap<&'static str, usize> {
        let mut out = BTreeMap::new();
        for r in &self.rows {
            *out.entry(r.verdict).or_insert(0) += 1;
        }
        out
    }

    /// Whether the claim passes every graded case, and there is one.
    #[must_use]
    pub fn passed(&self) -> bool {
        self.graded > 0 && self.rows.is_empty()
    }
}

/// Which cell is the claim, from the cells given.
pub fn claim(given: [bool; 3]) -> Result<&'static str, String> {
    match given {
        [_, _, true] => Ok("RR"),
        [true, false, false] => Ok("RG"),
        [false, true, false] => Ok("GR"),
        [true, true, false] => {
            Err("RG and GR are given with no RR: give RR, the claim, or one of the two".into())
        }
        [false, false, false] => Err("give RG, GR or RR".into()),
    }
}

impl Test {
    /// Attribute each graded case the claim does not pass.
    pub fn attribute(&self) -> Result<Attribution, String> {
        let given = [
            self.runs[0].is_some(),
            self.runs[1].is_some(),
            self.runs[2].is_some(),
        ];
        let claim = claim(given)?;
        let cells: Vec<Option<BTreeMap<Key, (bool, String)>>> =
            self.runs.iter().map(|r| r.as_deref().map(cell)).collect();
        let at = CELLS.iter().position(|c| *c == claim).unwrap_or(2);
        let claim_cell = cells[at].clone().unwrap_or_default();
        let mut rows = Vec::new();
        let mut passed = 0;
        for key in &self.reference.pass {
            if claim_cell.get(key).is_some_and(|(p, _)| *p) {
                passed += 1;
                continue;
            }
            let look = |i: usize| -> (Option<bool>, String) {
                match &cells[i] {
                    None => (None, "-".into()),
                    Some(c) => match c.get(key) {
                        Some((p, t)) => (Some(*p), t.clone()),
                        // A cell that was given but did not run the case says nothing of it when
                        // it is not the claim: it ran only the cases the claim failed.
                        None if i == at => (Some(false), "not-run".into()),
                        None => (None, "not-run".into()),
                    },
                }
            };
            let (kernel_swap, kernel_text) = look(0);
            let (world_swap, world_text) = look(1);
            let (_, rr_text) = look(2);
            rows.push(Row {
                key: key.clone(),
                gg: self.reference.text.get(key).cloned().unwrap_or_default(),
                cells: [kernel_text, world_text, rr_text],
                verdict: verdict(claim, kernel_swap, world_swap),
            });
        }
        Ok(Attribution {
            claim,
            graded: self.reference.pass.len(),
            not_graded: self.reference.text.len() - self.reference.pass.len(),
            passed,
            rows,
        })
    }
}

/// The suites and top-level test directories of the cases, for the boots of RG and GR that run
/// only the cases the claim failed.
#[must_use]
pub fn narrow(rows: &[Row]) -> (Vec<String>, Vec<String>) {
    let mut suites = BTreeSet::from(["boot".to_string()]);
    let mut dirs = BTreeSet::new();
    for r in rows {
        suites.insert(r.key.0.clone());
        if r.key.0 == "tests" {
            dirs.insert(r.key.1.split('/').next().unwrap_or_default().to_string());
        }
    }
    let order = ["boot", "smoke", "tests"];
    let mut suites: Vec<String> = suites.into_iter().collect();
    suites.sort_by_key(|s| order.iter().position(|o| o == s).unwrap_or(order.len()));
    (suites, dirs.into_iter().collect())
}

fn runs_text(runs: &[Run]) -> String {
    runs.iter()
        .map(|r| format!("{} ({})", r.outcome.result, r.dir.display()))
        .collect::<Vec<_>>()
        .join(", ")
}

/// The report, as markdown. The case list stops after `limit` lines.
#[must_use]
pub fn report(t: &Test, a: &Attribution, limit: usize) -> String {
    let mut s = String::from("# rb test: the 2x2\n\n| cell | runs |\n|---|---|\n");
    if t.gg_runs.is_empty() {
        s.push_str("| GG | the baseline |\n");
    } else {
        let _ = writeln!(s, "| GG | {} |", runs_text(&t.gg_runs));
    }
    for (name, runs) in CELLS.iter().zip(&t.runs) {
        let text = match runs {
            Some(r) if r.is_empty() => "not booted: the claim failed no case".to_string(),
            Some(r) => runs_text(r),
            None => "not given".into(),
        };
        let _ = writeln!(s, "| {name} | {text} |");
    }
    let _ = writeln!(
        s,
        "\nGG passes {} cases. {} more cases are g-unstable or did not pass in GG, and are not graded.\n",
        a.graded, a.not_graded
    );
    let _ = writeln!(
        s,
        "{}, the claim, passes {} of the {} cases. It does not pass {}.\n",
        a.claim,
        a.passed,
        a.graded,
        a.rows.len()
    );
    if !a.rows.is_empty() {
        s.push_str("| verdict | cases |\n|---|---|\n");
        for (v, n) in a.verdicts() {
            let _ = writeln!(s, "| {v} | {n} |");
        }
        let at = CELLS.iter().position(|c| *c == a.claim).unwrap_or(2);
        let mut by: BTreeMap<(&str, &str), usize> = BTreeMap::new();
        for r in &a.rows {
            let dir = r.key.1.split('/').next().unwrap_or_default();
            *by.entry((dir, r.cells[at].as_str())).or_insert(0) += 1;
        }
        let _ = writeln!(
            s,
            "\n| top-level directory | {} gave | cases |\n|---|---|---|",
            a.claim
        );
        for ((dir, result), n) in by {
            let _ = writeln!(s, "| {dir} | {result} | {n} |");
        }
        s.push_str(
            "\n| suite | case | GG | RG | GR | RR | verdict |\n|---|---|---|---|---|---|---|\n",
        );
        for r in a.rows.iter().take(limit) {
            let _ = writeln!(
                s,
                "| {} | {} | {} | {} | {} | {} | {} |",
                r.key.0, r.key.1, r.gg, r.cells[0], r.cells[1], r.cells[2], r.verdict
            );
        }
        if a.rows.len() > limit {
            let _ = writeln!(s, "\n... and {} more cases.", a.rows.len() - limit);
        }
        s.push('\n');
    }
    let _ = writeln!(
        s,
        "Gate: {}.",
        if a.passed() {
            format!("{} passes every case GG passes", a.claim)
        } else if a.graded == 0 {
            "GG passes no case, so nothing is graded".to_string()
        } else {
            format!("{} does not pass every case GG passes", a.claim)
        }
    );
    s
}

/// The attribution as tab-separated values, one line per graded case the claim does not pass.
#[must_use]
pub fn tsv(a: &Attribution) -> String {
    let mut s = String::from("# suite\tcase\tGG\tRG\tGR\tRR\tverdict\n");
    for r in &a.rows {
        let _ = writeln!(
            s,
            "{}\t{}\t{}\t{}\t{}\t{}\t{}",
            r.key.0, r.key.1, r.gg, r.cells[0], r.cells[1], r.cells[2], r.verdict
        );
    }
    s
}

/// Parse runs as `rb baseline --from` takes them: runs separated by `,`, and the shards of one
/// run by `+`.
#[must_use]
pub fn parse_runs(v: &str) -> Vec<Vec<PathBuf>> {
    v.split(',')
        .filter(|s| !s.is_empty())
        .map(|run| {
            run.split('+')
                .filter(|s| !s.is_empty())
                .map(PathBuf::from)
                .collect()
        })
        .collect()
}

/// Read the runs of one cell.
pub fn read_runs(runs: &[Vec<PathBuf>]) -> Result<Vec<Run>, String> {
    runs.iter().map(|s| baseline::read_shards(s)).collect()
}

/// Read the baseline in `dir`: its `cases.tsv`.
pub fn read_baseline(dir: &Path) -> Result<Reference, String> {
    let f = dir.join("cases.tsv");
    let text = std::fs::read_to_string(&f).map_err(|e| format!("{}: {e}", f.display()))?;
    Reference::from_baseline(&text).map_err(|e| format!("{}: {e}", f.display()))
}

/// The command line of `rb test`.
pub mod cli {
    use super::{
        Attribution, CELLS, Reference, Test, narrow, parse_runs, read_baseline, read_runs, report,
        tsv,
    };
    use crate::baseline::{Run, read_run};
    use crate::boot::{cli as boot_cli, kvm_usable};
    use crate::cli::Args;
    use rb_pins::places::Rows;
    use std::fmt::Write as _;
    use std::path::{Path, PathBuf};

    /// The suites GG and the claim run by default.
    pub const SUITES: &str = "boot,smoke,tests";

    /// The options of `rb test` that are not `rb boot`'s.
    const OWN: [&str; 12] = [
        "gg", "rg", "gr", "rr", "gg-image", "rg-image", "gr-image", "rr-image", "baseline", "out",
        "work", "limit",
    ];

    /// The `rb boot` arguments of one boot of `image` into `out`. With `narrowed`, only those
    /// suites and top-level test directories run.
    fn boot_args(
        args: &Args,
        image: &str,
        out: &Path,
        narrowed: Option<&(Vec<String>, Vec<String>)>,
    ) -> Args {
        let mut a = args.clone();
        a.command = "boot".into();
        a.target = None;
        for k in OWN {
            a.values.remove(k);
        }
        a.switches.retain(|s| s != "dry-run");
        a.values.insert("image".into(), image.into());
        a.values.insert("out".into(), out.display().to_string());
        a.values
            .entry("suite".into())
            .or_insert_with(|| SUITES.into());
        if let Some((suites, dirs)) = narrowed {
            let asked: Vec<&str> = a.values["suite"].split(',').collect();
            let suites: Vec<&str> = suites
                .iter()
                .map(String::as_str)
                .filter(|s| asked.contains(s))
                .collect();
            a.values.insert("suite".into(), suites.join(","));
            a.values.remove("tests-except");
            if dirs.is_empty() {
                a.values.remove("tests-only");
            } else {
                a.values.insert("tests-only".into(), dirs.join(","));
            }
        }
        a
    }

    /// Where a cell's runs come from.
    enum Source {
        Runs(Vec<Vec<PathBuf>>),
        Image(String),
        None,
    }

    fn source(args: &Args, cell: &str) -> Result<Source, String> {
        let lower = cell.to_ascii_lowercase();
        match (args.get(&lower), args.get(&format!("{lower}-image"))) {
            (Some(_), Some(_)) => Err(format!("give --{lower} or --{lower}-image, not both")),
            (Some(v), None) => {
                let runs = parse_runs(v);
                if runs.is_empty() {
                    return Err(format!("--{lower} names no directory"));
                }
                Ok(Source::Runs(runs))
            }
            (None, Some(img)) => Ok(Source::Image(img.to_string())),
            (None, None) => Ok(Source::None),
        }
    }

    /// Boots one cell, or prints what it would boot.
    struct Booter<'a> {
        args: &'a Args,
        rows: Option<&'a Rows>,
        work: PathBuf,
        dry: Option<String>,
    }

    impl Booter<'_> {
        fn boot(
            &mut self,
            cell: &str,
            image: &str,
            narrowed: Option<&(Vec<String>, Vec<String>)>,
        ) -> Result<Vec<Run>, String> {
            let out = self.work.join(cell);
            let a = boot_args(self.args, image, &out, narrowed);
            let rows = self.rows.ok_or(
                "booting an image needs the repository's rows.toml; run rb in the repository",
            )?;
            let req = boot_cli::request(&a, rows, kvm_usable)?;
            if let Some(s) = self.dry.as_mut() {
                if narrowed.is_some() {
                    let _ = writeln!(
                        s,
                        "- {cell}: after the claim, for the suites and top-level test directories of the cases it fails only, as {}\n",
                        out.display()
                    );
                } else {
                    let _ = writeln!(
                        s,
                        "- {cell}: every suite asked for\n\n```\n{}\n```\n",
                        crate::boot::qemu_command(&req).join(" ")
                    );
                }
                return Ok(Vec::new());
            }
            eprintln!(
                "rb test: booting {cell} from {image} into {}",
                out.display()
            );
            let o = crate::boot::run(&req)?;
            eprintln!("rb test: {cell}: {} in {:.0} s", o.result, o.seconds);
            Ok(vec![read_run(&out)?])
        }
    }

    /// Run `rb test`. `rows` is the repository's rows.toml, which booting an image needs. Returns
    /// the report and whether the claim passes every case GG passes.
    pub fn run(args: &Args, rows: Option<&Rows>) -> Result<(String, bool), String> {
        let limit = args.get("limit").map_or(Ok(50), |n| {
            n.parse()
                .map_err(|_| format!("--limit {n} is not a number"))
        })?;
        let mut booter = Booter {
            args,
            rows,
            work: PathBuf::from(args.get("work").unwrap_or("work/test")),
            dry: args
                .has("dry-run")
                .then(|| "# rb test: a dry run\n\n".to_string()),
        };
        let mut t = Test::default();
        let gg = source(args, "GG")?;
        let baseline = args
            .get("baseline")
            .map(|d| read_baseline(Path::new(d)))
            .transpose()?;
        t.gg_runs = match gg {
            Source::Runs(r) => read_runs(&r)?,
            Source::Image(img) => booter.boot("GG", &img, None)?,
            Source::None => Vec::new(),
        };
        t.reference = match (t.gg_runs.is_empty(), baseline) {
            (false, Some(b)) => Reference::from_runs(&t.gg_runs).and(&b),
            (false, None) => Reference::from_runs(&t.gg_runs),
            (true, Some(b)) => b,
            (true, None) if booter.dry.is_some() => Reference::default(),
            (true, None) => {
                return Err("give GG: --gg RUNS, --gg-image IMG or --baseline DIR".into());
            }
        };
        let sources = [
            source(args, "RG")?,
            source(args, "GR")?,
            source(args, "RR")?,
        ];
        let claim = super::claim([
            !matches!(sources[0], Source::None),
            !matches!(sources[1], Source::None),
            !matches!(sources[2], Source::None),
        ])?;
        let at = CELLS.iter().position(|c| *c == claim).unwrap_or(2);
        // The claim first, with every suite; then the other cells, for its failures only.
        let mut order = vec![at];
        order.extend((0..3).filter(|i| *i != at));
        for i in order {
            let runs = match &sources[i] {
                Source::None => None,
                Source::Runs(r) => Some(read_runs(r)?),
                Source::Image(img) if i == at => Some(booter.boot(CELLS[i], img, None)?),
                Source::Image(img) => {
                    let a = t.attribute()?;
                    if a.rows.is_empty() && booter.dry.is_none() {
                        Some(Vec::new())
                    } else {
                        let n = narrow(&a.rows);
                        Some(booter.boot(CELLS[i], img, Some(&n))?)
                    }
                }
            };
            t.runs[i] = runs;
        }
        if let Some(s) = booter.dry {
            return Ok((s, true));
        }
        let a: Attribution = t.attribute()?;
        let text = report(&t, &a, limit);
        if let Some(out) = args.get("out") {
            let out = Path::new(out);
            std::fs::create_dir_all(out).map_err(|e| format!("{}: {e}", out.display()))?;
            for (name, body) in [("attribution.tsv", tsv(&a)), ("test.md", text.clone())] {
                let f = out.join(name);
                std::fs::write(&f, body).map_err(|e| format!("{}: {e}", f.display()))?;
            }
        }
        Ok((text, a.passed()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::boot::Outcome;
    use crate::cases::Case;

    fn run(cases: &[(&str, &str)]) -> Run {
        Run {
            dir: PathBuf::from("run"),
            outcome: Outcome {
                result: "pass".into(),
                ..Outcome::default()
            },
            cases: cases
                .iter()
                .map(|(name, result)| Case {
                    suite: "tests".into(),
                    name: (*name).into(),
                    result: (*result).into(),
                    seconds: None,
                })
                .collect(),
            kernel: BTreeSet::new(),
        }
    }

    fn key(name: &str) -> Key {
        ("tests".to_string(), name.to_string())
    }

    #[test]
    fn the_four_verdicts() {
        let gg = run(&[
            ("lib/a:k", "passed"),
            ("lib/a:w", "passed"),
            ("net/b:both", "passed"),
            ("net/b:mix", "passed"),
            ("net/b:fine", "passed"),
            ("sys/c:skip", "skipped"),
        ]);
        let rr = run(&[
            ("lib/a:k", "failed"),
            ("lib/a:w", "failed"),
            ("net/b:both", "broken"),
            ("net/b:mix", "failed"),
            ("net/b:fine", "passed"),
        ]);
        let rg = run(&[
            ("lib/a:k", "failed"),
            ("lib/a:w", "passed"),
            ("net/b:both", "failed"),
            ("net/b:mix", "passed"),
        ]);
        let gr = run(&[
            ("lib/a:k", "passed"),
            ("lib/a:w", "failed"),
            ("net/b:both", "failed"),
            ("net/b:mix", "passed"),
        ]);
        let t = Test {
            reference: Reference::from_runs(std::slice::from_ref(&gg)),
            gg_runs: vec![gg],
            runs: [Some(vec![rg]), Some(vec![gr]), Some(vec![rr])],
        };
        let a = t.attribute().unwrap();
        assert_eq!(a.claim, "RR");
        assert_eq!((a.graded, a.not_graded, a.passed), (5, 1, 1));
        let v: Vec<(&str, &str)> = a
            .rows
            .iter()
            .map(|r| (r.key.1.as_str(), r.verdict))
            .collect();
        assert_eq!(
            v,
            [
                ("lib/a:k", "kernel"),
                ("lib/a:w", "world"),
                ("net/b:both", "both"),
                ("net/b:mix", "interaction"),
            ]
        );
        assert!(!a.passed());
        let text = report(&t, &a, 10);
        assert!(
            text.contains("RR, the claim, passes 1 of the 5 cases. It does not pass 4."),
            "{text}"
        );
        assert!(tsv(&a).contains("tests\tnet/b:both\tpassed\tfailed\tfailed\tbroken\tboth\n"));
        assert_eq!(
            narrow(&a.rows),
            (
                vec!["boot".to_string(), "tests".to_string()],
                vec!["lib".to_string(), "net".to_string()]
            )
        );
    }

    #[test]
    fn rg_alone_is_the_claim() {
        let gg = vec![
            run(&[("lib/a:x", "passed"), ("lib/a:y", "passed")]),
            run(&[("lib/a:x", "passed"), ("lib/a:y", "failed")]),
        ];
        let rg = run(&[("lib/a:x", "broken")]);
        let t = Test {
            reference: Reference::from_runs(&gg),
            gg_runs: gg,
            runs: [Some(vec![rg]), None, None],
        };
        let a = t.attribute().unwrap();
        assert_eq!(a.claim, "RG");
        assert_eq!((a.graded, a.not_graded), (1, 1));
        assert_eq!(a.rows[0].verdict, "kernel");
        assert_eq!(
            a.rows[0].cells,
            ["broken".to_string(), "-".into(), "-".into()]
        );
        assert_eq!(t.reference.text[&key("lib/a:y")], "passed,failed");
    }

    #[test]
    fn a_case_the_claim_did_not_run_is_not_passed() {
        let gg = run(&[("lib/a:x", "passed")]);
        let rr = run(&[]);
        let t = Test {
            reference: Reference::from_runs(std::slice::from_ref(&gg)),
            gg_runs: vec![gg],
            runs: [
                Some(vec![run(&[("lib/a:x", "failed")])]),
                None,
                Some(vec![rr]),
            ],
        };
        let a = t.attribute().unwrap();
        assert_eq!(a.rows[0].cells[2], "not-run");
        assert_eq!(a.rows[0].verdict, "kernel or both");
    }

    #[test]
    fn the_claim_from_the_cells_given() {
        assert_eq!(claim([true, true, true]), Ok("RR"));
        assert_eq!(claim([false, false, true]), Ok("RR"));
        assert_eq!(claim([true, false, false]), Ok("RG"));
        assert_eq!(claim([false, true, false]), Ok("GR"));
        assert!(claim([true, true, false]).is_err());
        assert!(claim([false, false, false]).is_err());
    }

    #[test]
    fn a_baseline_and_runs_both_grade() {
        let b = Reference::from_baseline(
            "# suite\tcase\trun-1\trun-2\tseconds-1\tseconds-2\tverdict\n\
             tests\tlib/a:x\tpassed\tpassed\t1\t1\tstable-pass\n\
             tests\tlib/a:y\tpassed\tfailed\t1\t1\tg-unstable\n",
        )
        .unwrap();
        assert_eq!(b.pass.len(), 1);
        let r = Reference::from_runs(&[run(&[("lib/a:x", "passed"), ("lib/a:y", "passed")])]);
        let both = r.and(&b);
        assert_eq!(both.pass, BTreeSet::from([key("lib/a:x")]));
        assert_eq!(both.text[&key("lib/a:y")], "passed g-unstable");
    }

    #[test]
    fn every_verdict_with_a_cell_missing() {
        assert_eq!(verdict("RR", Some(false), None), "kernel or both");
        assert_eq!(verdict("RR", None, Some(true)), "kernel or interaction");
        assert_eq!(verdict("RR", None, None), "open");
        assert_eq!(verdict("GR", None, None), "world");
    }
}
