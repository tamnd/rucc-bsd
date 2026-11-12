//! `rb bench`: the benchmark set of plan 13.2, R against G (13.3).
//!
//! The bench suite of rb-rc (`guest/rb-rc/bench.sh`) runs the set once per boot and writes
//! `results/bench/bench.tsv`: one line per benchmark with the group, the name, the value and
//! whether lower or higher is better. `rb bench` boots R and G in turn, R first, N times each,
//! 10 by default, each with the rb-bench that its own compiler built. Then for each benchmark it
//! takes the median per side and the speed of R against G:
//! - for a time (lower is better), G's median over R's;
//! - for a rate (higher is better), R's median over G's.
//!
//! So a speed under 1 means R is slower. A benchmark is gated when both sides have a value in
//! every run, there are at least [`MIN_RUNS`] runs, and the coefficient of variation on G is at
//! most [`CV_LIMIT`]. The other benchmarks are reported, not gated. The headline is the geometric
//! mean of the gated speeds. `rb speed --bench` applies the gates of 02.5 to it.
//!
//! The command writes `results/bench/<system>/<row>/<version>/`:
//! - `runs.tsv`: every value of every run: side, run, group, name, value, better;
//! - `bench.tsv`: the summary, with `#` lines for what was run and one line per benchmark.
//!
//! With `--g-from` and `--r-from` the runs are the output directories of earlier boots, and
//! nothing boots, so that a workflow can boot in separate jobs. A directory is one of `rb boot`,
//! with `results/bench`, or one of `scripts/freebsd/boot.sh`, with `results/out/bench`.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

/// The coefficient of variation on G above which a benchmark is not gated (13.3).
pub const CV_LIMIT: f64 = 0.01;
/// The fewest runs per side for a gated benchmark.
pub const MIN_RUNS: usize = 3;
/// The speed under which a benchmark is behind and listed: 2% slower.
pub const BEHIND: f64 = 0.98;

/// One line of a bench suite's `bench.tsv`.
#[derive(Debug, Clone, PartialEq)]
pub struct Sample {
    /// The group, such as `syscalls`.
    pub group: String,
    /// The benchmark.
    pub name: String,
    /// Seconds, or a rate.
    pub value: f64,
    /// Whether a lower value is better.
    pub lower: bool,
}

/// Read a `bench.tsv` of the bench suite. A line that is not four fields with a number and
/// `lower` or `higher` is an error.
pub fn parse_run(text: &str) -> Result<Vec<Sample>, String> {
    let mut out = Vec::new();
    for (n, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let f: Vec<&str> = line.split('\t').collect();
        let bad = || format!("line {}: {line:?} is not GROUP NAME VALUE BETTER", n + 1);
        let [group, name, value, better] = f.as_slice() else {
            return Err(bad());
        };
        let value: f64 = value.parse().map_err(|_| bad())?;
        let lower = match *better {
            "lower" => true,
            "higher" => false,
            _ => return Err(bad()),
        };
        if !value.is_finite() || value < 0.0 {
            return Err(bad());
        }
        out.push(Sample {
            group: (*group).to_string(),
            name: (*name).to_string(),
            value,
            lower,
        });
    }
    Ok(out)
}

/// Where the bench suite wrote its results in a boot's output directory: `results/bench` for
/// `rb boot`, `results/out/bench` for `scripts/freebsd/boot.sh`.
#[must_use]
pub fn suite_dir(dir: &Path) -> PathBuf {
    let freebsd = dir.join("results/out/bench");
    if freebsd.is_dir() {
        freebsd
    } else {
        dir.join("results/bench")
    }
}

/// The benchmark results of one boot's output directory, or `None` when the boot wrote none.
pub fn read_run(dir: &Path) -> Result<Option<Vec<Sample>>, String> {
    let file = suite_dir(dir).join("bench.tsv");
    match std::fs::read_to_string(&file) {
        Ok(text) => parse_run(&text)
            .map(Some)
            .map_err(|e| format!("{}: {e}", file.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("{}: {e}", file.display())),
    }
}

/// The coefficient of variation: the sample standard deviation over the mean. `None` for fewer
/// than two values or a mean of zero.
#[must_use]
#[allow(clippy::cast_precision_loss)]
pub fn cv(values: &[f64]) -> Option<f64> {
    if values.len() < 2 {
        return None;
    }
    let n = values.len() as f64;
    let mean = values.iter().sum::<f64>() / n;
    if mean <= 0.0 {
        return None;
    }
    let var = values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / (n - 1.0);
    Some(var.sqrt() / mean)
}

/// One benchmark over all runs.
#[derive(Debug, Clone, PartialEq)]
pub struct Bench {
    /// The group.
    pub group: String,
    /// The benchmark.
    pub name: String,
    /// Whether a lower value is better.
    pub lower: bool,
    /// G's values, one per run that had one.
    pub g: Vec<f64>,
    /// R's values.
    pub r: Vec<f64>,
}

impl Bench {
    /// The median of G.
    #[must_use]
    pub fn g_median(&self) -> Option<f64> {
        harness_stats::median(&self.g)
    }

    /// The median of R.
    #[must_use]
    pub fn r_median(&self) -> Option<f64> {
        harness_stats::median(&self.r)
    }

    /// R's speed against G's: above 1 is faster, under 1 slower.
    #[must_use]
    pub fn speed(&self) -> Option<f64> {
        let (g, r) = (self.g_median()?, self.r_median()?);
        let (num, den) = if self.lower { (g, r) } else { (r, g) };
        (den > 0.0 && num > 0.0).then(|| num / den)
    }

    /// Why the benchmark is not gated, or `None` when it is.
    #[must_use]
    pub fn not_gated(&self, runs: usize) -> Option<String> {
        if runs < MIN_RUNS {
            return Some(format!("{runs} runs"));
        }
        if self.g.len() < runs || self.r.len() < runs {
            return Some(format!(
                "values in {} G and {} R runs of {runs}",
                self.g.len(),
                self.r.len()
            ));
        }
        if self.speed().is_none() {
            return Some("no speed".into());
        }
        match cv(&self.g) {
            Some(c) if c <= CV_LIMIT => None,
            Some(c) => Some(format!("G CV {:.1}%", c * 100.0)),
            None => Some("no CV".into()),
        }
    }
}

/// What a summary is of.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Meta {
    /// The pin, such as `freebsd-15.1`.
    pub pin: String,
    /// `freebsd` or `netbsd`.
    pub system: String,
    /// The row, such as `amd64`.
    pub row: String,
    /// When it was written.
    pub made: String,
    /// The images, by file name.
    pub g_image: String,
    /// R's image.
    pub r_image: String,
    /// The version of rb-bench, from `rb-bench -V`.
    pub version: String,
}

impl Meta {
    /// The directory under `results/bench`: `<system>/<row>/<version>`, where the version is the
    /// pin name without the system, as for baselines.
    #[must_use]
    pub fn place(&self) -> PathBuf {
        let short = self
            .pin
            .strip_prefix(&format!("{}-", self.system))
            .unwrap_or(&self.pin);
        [&self.system, &self.row, short].iter().collect()
    }
}

/// The results of all runs of both sides.
#[derive(Debug, Clone, PartialEq)]
pub struct Summary {
    /// What was run.
    pub meta: Meta,
    /// How many runs per side were asked for.
    pub runs: usize,
    /// The runs of G that wrote results.
    pub g_runs: usize,
    /// The runs of R that wrote results.
    pub r_runs: usize,
    /// Each benchmark, in the order of G's first run, then any that only R has.
    pub benches: Vec<Bench>,
}

impl Summary {
    /// Join the runs of each side. A run is `None` when its boot wrote no results.
    #[must_use]
    pub fn new(meta: Meta, g: &[Option<Vec<Sample>>], r: &[Option<Vec<Sample>>]) -> Self {
        let mut order: Vec<(String, String)> = Vec::new();
        let mut by: BTreeMap<(String, String), Bench> = BTreeMap::new();
        for (side, runs) in [(0, g), (1, r)] {
            for run in runs.iter().flatten() {
                for s in run {
                    let key = (s.group.clone(), s.name.clone());
                    let b = by.entry(key.clone()).or_insert_with(|| {
                        order.push(key);
                        Bench {
                            group: s.group.clone(),
                            name: s.name.clone(),
                            lower: s.lower,
                            g: Vec::new(),
                            r: Vec::new(),
                        }
                    });
                    if side == 0 {
                        b.g.push(s.value);
                    } else {
                        b.r.push(s.value);
                    }
                }
            }
        }
        let benches = order.into_iter().filter_map(|k| by.remove(&k)).collect();
        Summary {
            meta,
            runs: g.len().max(r.len()),
            g_runs: g.iter().flatten().count(),
            r_runs: r.iter().flatten().count(),
            benches,
        }
    }

    /// The gated benchmarks.
    pub fn gated(&self) -> impl Iterator<Item = &Bench> {
        self.benches
            .iter()
            .filter(|b| b.not_gated(self.runs).is_none())
    }

    /// The geometric mean of the gated speeds.
    #[must_use]
    pub fn geomean(&self) -> Option<f64> {
        let speeds: Vec<f64> = self.gated().filter_map(Bench::speed).collect();
        harness_stats::geometric_mean(&speeds)
    }

    /// Whether every run of both sides wrote results.
    #[must_use]
    pub fn complete(&self) -> bool {
        self.g_runs == self.runs && self.r_runs == self.runs
    }

    /// `bench.tsv`: the summary.
    #[must_use]
    pub fn tsv(&self) -> String {
        let m = &self.meta;
        let mut s = String::from("# rb bench: the benchmark set of plan 13.2, R against G\n");
        for (k, v) in [
            ("pin", m.pin.as_str()),
            ("system", &m.system),
            ("row", &m.row),
            ("made", &m.made),
            ("g-image", &m.g_image),
            ("r-image", &m.r_image),
            ("rb-bench", &m.version),
        ] {
            let _ = writeln!(s, "# {k}\t{v}");
        }
        let _ = writeln!(
            s,
            "# runs\t{}\n# g-runs\t{}\n# r-runs\t{}",
            self.runs, self.g_runs, self.r_runs
        );
        if let Some(g) = self.geomean() {
            let _ = writeln!(s, "# geomean\t{g:.4}");
        }
        s.push_str(
            "group\tname\tbetter\tg_median\tr_median\tg_cv\tr_cv\tg_runs\tr_runs\tspeed\tgated\n",
        );
        let num = |v: Option<f64>| v.map_or_else(|| "-".into(), |v| format!("{v:.6}"));
        for b in &self.benches {
            let _ = writeln!(
                s,
                "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
                b.group,
                b.name,
                if b.lower { "lower" } else { "higher" },
                num(b.g_median()),
                num(b.r_median()),
                num(cv(&b.g)),
                num(cv(&b.r)),
                b.g.len(),
                b.r.len(),
                b.speed().map_or_else(|| "-".into(), |v| format!("{v:.4}")),
                b.not_gated(self.runs).unwrap_or_else(|| "yes".into()),
            );
        }
        s
    }

    /// The report: the headline, the benchmarks more than 2% behind, and every benchmark.
    #[must_use]
    pub fn report(&self) -> String {
        let m = &self.meta;
        let mut s = format!(
            "# rb bench: {} {}\n\nG {} and R {}, {} runs each; results from {} G and {} R runs. \
             rb-bench {}.\n\n",
            m.pin,
            m.row,
            or_dash(&m.g_image),
            or_dash(&m.r_image),
            self.runs,
            self.g_runs,
            self.r_runs,
            or_dash(&m.version),
        );
        let gated = self.gated().count();
        match self.geomean() {
            Some(g) => {
                let _ = writeln!(
                    s,
                    "Geometric mean of R's speed over {gated} gated benchmarks: {g:.4}.\n"
                );
            }
            None => s.push_str("No benchmark is gated.\n\n"),
        }
        let behind: Vec<&Bench> = self
            .benches
            .iter()
            .filter(|b| b.speed().is_some_and(|v| v < BEHIND))
            .collect();
        let _ = writeln!(s, "## Behind by more than 2%: {}\n", behind.len());
        if !behind.is_empty() {
            s.push_str("| Benchmark | Group | Speed | Gated |\n|---|---|---|---|\n");
            for b in behind {
                let _ = writeln!(
                    s,
                    "| {} | {} | {:.4} | {} |",
                    b.name,
                    b.group,
                    b.speed().unwrap_or_default(),
                    b.not_gated(self.runs).unwrap_or_else(|| "yes".into())
                );
            }
            s.push('\n');
        }
        s.push_str("## All benchmarks\n\n| Benchmark | Group | Better | G median | R median | G CV | Speed | Gated |\n|---|---|---|---|---|---|---|---|\n");
        for b in &self.benches {
            let _ = writeln!(
                s,
                "| {} | {} | {} | {} | {} | {} | {} | {} |",
                b.name,
                b.group,
                if b.lower { "lower" } else { "higher" },
                b.g_median()
                    .map_or_else(|| "-".into(), |v| format!("{v:.4}")),
                b.r_median()
                    .map_or_else(|| "-".into(), |v| format!("{v:.4}")),
                cv(&b.g).map_or_else(|| "-".into(), |v| format!("{:.2}%", v * 100.0)),
                b.speed().map_or_else(|| "-".into(), |v| format!("{v:.4}")),
                b.not_gated(self.runs).unwrap_or_else(|| "yes".into())
            );
        }
        s
    }
}

fn or_dash(s: &str) -> &str {
    if s.is_empty() { "-" } else { s }
}

/// `runs.tsv`: every value of every run.
#[must_use]
pub fn runs_tsv(g: &[Option<Vec<Sample>>], r: &[Option<Vec<Sample>>]) -> String {
    let mut s = String::from("side\trun\tgroup\tname\tvalue\tbetter\n");
    for (side, runs) in [("g", g), ("r", r)] {
        for (n, run) in runs.iter().enumerate() {
            for x in run.iter().flatten() {
                let _ = writeln!(
                    s,
                    "{side}\t{}\t{}\t{}\t{}\t{}",
                    n + 1,
                    x.group,
                    x.name,
                    x.value,
                    if x.lower { "lower" } else { "higher" }
                );
            }
        }
    }
    s
}

/// A summary `bench.tsv` read back: each benchmark's speed and whether it is gated, for
/// `rb speed --bench`.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    /// The group.
    pub group: String,
    /// The benchmark.
    pub name: String,
    /// R's speed against G's.
    pub speed: Option<f64>,
    /// Why it is not gated, or `None` when it is.
    pub not_gated: Option<String>,
}

/// Read the benchmarks of a summary `bench.tsv`.
pub fn parse_summary(text: &str) -> Result<Vec<Row>, String> {
    let mut lines = text.lines().filter(|l| !l.starts_with('#'));
    let head: Vec<&str> = lines.next().ok_or("no header line")?.split('\t').collect();
    let col = |name: &str| {
        head.iter()
            .position(|h| *h == name)
            .ok_or(format!("no column {name}"))
    };
    let (g, n, sp, gt) = (col("group")?, col("name")?, col("speed")?, col("gated")?);
    let mut out = Vec::new();
    for line in lines.filter(|l| !l.trim().is_empty()) {
        let f: Vec<&str> = line.split('\t').collect();
        if f.len() != head.len() {
            return Err(format!(
                "{line:?} has {} fields, not {}",
                f.len(),
                head.len()
            ));
        }
        out.push(Row {
            group: f[g].to_string(),
            name: f[n].to_string(),
            speed: f[sp].parse().ok(),
            not_gated: (f[gt] != "yes").then(|| f[gt].to_string()),
        });
    }
    Ok(out)
}

/// The command line of `rb bench`.
pub mod cli {
    use super::{Meta, Sample, Summary, read_run, runs_tsv};
    use crate::boot::{self, cli as boot_cli, kvm_usable};
    use crate::cli::Args;
    use rb_pins::pins::PinsFile;
    use rb_pins::places::Rows;
    use std::fmt::Write as _;
    use std::path::{Path, PathBuf};

    /// The options of `rb bench` that `rb boot` does not take.
    const OURS: &[&str] = &[
        "g-image", "r-image", "g-bench", "r-bench", "g-from", "r-from", "runs", "dir", "work",
        "system", "budget",
    ];

    fn dirs(v: Option<&str>) -> Vec<PathBuf> {
        v.map(|v| {
            v.split(',')
                .filter(|s| !s.is_empty())
                .map(PathBuf::from)
                .collect()
        })
        .unwrap_or_default()
    }

    /// What the pin is, from pins.toml or `--system`.
    fn meta(args: &Args, pins: Option<&PinsFile>) -> Result<Meta, String> {
        let pin = args
            .target
            .clone()
            .ok_or("give the pin, such as rb bench freebsd-15.1")?;
        let ok = |v: &str| {
            !v.is_empty()
                && v.chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '.' | '_'))
        };
        let row = args.get("row").unwrap_or("amd64").to_string();
        if !ok(&pin) || !ok(&row) {
            return Err(format!(
                "the pin {pin} and the row {row} must be letters, digits, '-', '.' or '_'"
            ));
        }
        let system = match (args.get("system"), pins.and_then(|p| p.find(&pin))) {
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
        Ok(Meta {
            pin,
            system,
            row,
            made: crate::sets::now(),
            ..Meta::default()
        })
    }

    /// The `rb boot` arguments of one boot of one side.
    fn boot_args(args: &Args, image: &str, bench: &str, out: &Path) -> Args {
        let mut a = args.clone();
        a.command = "boot".into();
        a.target = None;
        for k in OURS {
            a.values.remove(*k);
        }
        a.values.insert("image".into(), image.into());
        a.values.insert("bench".into(), bench.into());
        a.values.insert("out".into(), out.display().to_string());
        a.values.insert("suite".into(), "bench".into());
        // The suite comes from this rb, so an image of an older rb-rc runs it too.
        if !a.switches.iter().any(|s| s == "scripts") {
            a.switches.push("scripts".into());
        }
        a
    }

    fn file_name(p: &str) -> String {
        Path::new(p)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
    }

    /// The version line of the first run that has one.
    fn version(dirs: &[PathBuf]) -> String {
        dirs.iter()
            .find_map(|d| std::fs::read_to_string(super::suite_dir(d).join("version")).ok())
            .map(|v| v.trim().trim_start_matches("rb-bench ").to_string())
            .unwrap_or_default()
    }

    /// Boot each request in turn, and return how many runs finished and a note when the budget
    /// stopped the boots. A run is two boots, R then G. It starts only when both can still end in
    /// the budget, each at its limit.
    fn boot_all(
        reqs: &[(&str, usize, boot::Request)],
        runs: usize,
        budget: Option<u64>,
    ) -> Result<(usize, String), String> {
        let start = std::time::Instant::now();
        for (side, n, req) in reqs {
            if *side == "r"
                && let Some(b) = budget
                && start.elapsed().as_secs() + 2 * req.timeout > b
            {
                eprintln!("rb bench: stop before run {n}: the budget of {b} s");
                return Ok((
                    n - 1,
                    format!(
                        "\nrb bench stopped before run {n} of {runs}: two more boots of up to {} s each would pass the budget of {b} s.\n",
                        req.timeout
                    ),
                ));
            }
            eprintln!(
                "rb bench: {side} run {n} of {runs} in {}",
                req.out.display()
            );
            let o = boot::run(req)?;
            eprintln!(
                "rb bench: {side} run {n}: {} in {:.0} s",
                o.result, o.seconds
            );
        }
        Ok((runs, String::new()))
    }

    /// Run `rb bench`, and return the report and whether every run wrote results.
    pub fn run(
        args: &Args,
        root: &Path,
        pins: Option<&PinsFile>,
        rows: &Rows,
    ) -> Result<(String, bool), String> {
        let mut meta = meta(args, pins)?;
        let mut stopped = String::new();
        let top = args
            .get("dir")
            .map_or_else(|| root.join("results/bench"), PathBuf::from);
        let (g_from, r_from) = (dirs(args.get("g-from")), dirs(args.get("r-from")));
        let (g_dirs, r_dirs) = if g_from.is_empty() && r_from.is_empty() {
            let need = |k: &str| {
                args.get(k)
                    .map(str::to_string)
                    .ok_or(format!("give --{k}, or --g-from and --r-from"))
            };
            let (gi, ri, gb, rb) = (
                need("g-image")?,
                need("r-image")?,
                need("g-bench")?,
                need("r-bench")?,
            );
            let runs: usize = match args.get("runs") {
                Some(v) => v
                    .parse()
                    .map_err(|_| format!("--runs {v} is not a number"))?,
                None => 10,
            };
            if runs == 0 {
                return Err("--runs must be 1 or more".into());
            }
            let budget: Option<u64> = args
                .get("budget")
                .map(|v| {
                    v.parse()
                        .map_err(|_| format!("--budget {v} is not a number"))
                })
                .transpose()?;
            let work = args
                .get("work")
                .map_or_else(|| root.join("work/bench").join(meta.place()), PathBuf::from);
            let (mut gd, mut rd) = (Vec::new(), Vec::new());
            let mut reqs = Vec::new();
            for n in 1..=runs {
                for (side, image, bench, list) in
                    [("r", &ri, &rb, &mut rd), ("g", &gi, &gb, &mut gd)]
                {
                    let out = work.join(format!("{side}-{n}"));
                    let req =
                        boot_cli::request(&boot_args(args, image, bench, &out), rows, kvm_usable)?;
                    reqs.push((side, n, req));
                    list.push(out);
                }
            }
            if args.has("dry-run") {
                let (_, _, first) = &reqs[0];
                return Ok((
                    format!(
                        "# rb bench: a dry run\n\n{} boots, R then G, in {}. The summary goes to {}. The first boot:\n\n```\n{}\n```\n\nThe rb.conf on the results disk:\n\n```\n{}```\n",
                        reqs.len(),
                        work.display(),
                        top.join(meta.place()).display(),
                        boot::qemu_command(first).join(" "),
                        first.conf.render()
                    ),
                    true,
                ));
            }
            let (ran, note) = boot_all(&reqs, runs, budget)?;
            stopped = note;
            meta.g_image = file_name(&gi);
            meta.r_image = file_name(&ri);
            gd.truncate(ran);
            rd.truncate(ran);
            (gd, rd)
        } else {
            if g_from.is_empty() || r_from.is_empty() {
                return Err("give both --g-from and --r-from".into());
            }
            if let Some(v) = args.get("g-image") {
                meta.g_image = file_name(v);
            }
            if let Some(v) = args.get("r-image") {
                meta.r_image = file_name(v);
            }
            (g_from, r_from)
        };
        let read = |dirs: &[PathBuf]| -> Result<Vec<Option<Vec<Sample>>>, String> {
            dirs.iter().map(|d| read_run(d)).collect()
        };
        let (g, r) = (read(&g_dirs)?, read(&r_dirs)?);
        meta.version = version(&g_dirs);
        let summary = Summary::new(meta, &g, &r);
        let place = top.join(summary.meta.place());
        std::fs::create_dir_all(&place).map_err(|e| format!("{}: {e}", place.display()))?;
        for (name, text) in [("bench.tsv", summary.tsv()), ("runs.tsv", runs_tsv(&g, &r))] {
            let path = place.join(name);
            std::fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))?;
        }
        let mut report = summary.report();
        let _ = write!(report, "{stopped}\nWritten to {}.\n", place.display());
        Ok((report, summary.complete()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(lines: &[(&str, f64, &str)]) -> Option<Vec<Sample>> {
        let mut text = String::new();
        for (n, v, b) in lines {
            let _ = writeln!(text, "g\t{n}\t{v}\t{b}");
        }
        parse_run(&text).ok()
    }

    #[test]
    fn reads_a_run() {
        let s = parse_run("syscalls\tgetpid\t0.5\tlower\n\ncrypto\tsha\t1e9\thigher\n").unwrap();
        assert_eq!(s.len(), 2);
        assert!(s[0].lower && !s[1].lower);
        assert!((s[1].value - 1e9).abs() < 1.0);
        for bad in [
            "a\tb\t1\n",
            "a\tb\tx\tlower\n",
            "a\tb\t1\tfaster\n",
            "a\tb\t-1\tlower\n",
        ] {
            assert!(parse_run(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn coefficient_of_variation() {
        assert_eq!(cv(&[1.0]), None);
        assert!(cv(&[2.0, 2.0, 2.0]).unwrap().abs() < 1e-12);
        // mean 2, sample sd 1
        assert!((cv(&[1.0, 2.0, 3.0]).unwrap() - 0.5).abs() < 1e-12);
    }

    #[test]
    fn speeds_medians_and_gates() {
        let g: Vec<_> = (0..3)
            .map(|i| {
                run(&[
                    ("fast", 1.0, "lower"),
                    ("slow", 2.0 + f64::from(i) * 0.001, "lower"),
                    ("rate", 100.0, "higher"),
                    ("noisy", 1.0 + f64::from(i), "lower"),
                ])
            })
            .collect();
        let mut r: Vec<_> = (0..3)
            .map(|_| {
                run(&[
                    ("fast", 0.5, "lower"),
                    ("slow", 2.2, "lower"),
                    ("rate", 90.0, "higher"),
                    ("noisy", 1.0, "lower"),
                    ("ronly", 1.0, "lower"),
                ])
            })
            .collect();
        let s = Summary::new(Meta::default(), &g, &r);
        let by = |n: &str| s.benches.iter().find(|b| b.name == n).unwrap();
        assert!((by("fast").speed().unwrap() - 2.0).abs() < 1e-12);
        assert!((by("slow").speed().unwrap() - 2.001 / 2.2).abs() < 1e-12);
        assert!((by("rate").speed().unwrap() - 0.9).abs() < 1e-12);
        assert_eq!(by("noisy").not_gated(3).unwrap(), "G CV 50.0%");
        assert_eq!(
            by("ronly").not_gated(3).unwrap(),
            "values in 0 G and 3 R runs of 3"
        );
        assert_eq!(s.gated().count(), 3);
        let want = (2.0 * (2.001 / 2.2) * 0.9_f64).powf(1.0 / 3.0);
        assert!((s.geomean().unwrap() - want).abs() < 1e-12);
        assert!(s.complete());
        assert_eq!(s.benches.last().unwrap().name, "ronly");
        let report = s.report();
        assert!(
            report.contains("## Behind by more than 2%: 2\n"),
            "{report}"
        );
        assert!(report.contains("| slow | g | 0.9095 | yes |"), "{report}");
        assert!(report.contains("| rate | g | 0.9000 | yes |"), "{report}");
        // The summary reads back.
        let rows = parse_summary(&s.tsv()).unwrap();
        assert_eq!(rows.len(), 5);
        assert_eq!(rows[0].name, "fast");
        assert!((rows[0].speed.unwrap() - 2.0).abs() < 1e-9);
        assert_eq!(rows[0].not_gated, None);
        assert_eq!(rows[3].not_gated.as_deref(), Some("G CV 50.0%"));
        assert_eq!(rows[4].speed, None);
        // A boot with no results leaves the run out and the summary incomplete.
        r[1] = None;
        let s = Summary::new(Meta::default(), &g, &r);
        assert!(!s.complete());
        assert_eq!(s.r_runs, 2);
        assert_eq!(s.gated().count(), 0);
        let t = runs_tsv(&g, &r);
        assert_eq!(t.lines().count(), 1 + 3 * 4 + 2 * 5);
        assert!(t.contains("\nr\t3\tg\tronly\t1\tlower\n"), "{t}");
    }

    #[test]
    fn reads_the_runs_of_both_boot_scripts() {
        let top = std::env::temp_dir().join(format!("rb-bench-{}", std::process::id()));
        let line = "syscalls\tgetpid\t0.5\tlower\n";
        for (name, sub) in [("rb", "results/bench"), ("fb", "results/out/bench")] {
            let d = top.join(name).join(sub);
            std::fs::create_dir_all(&d).unwrap();
            std::fs::write(d.join("bench.tsv"), line).unwrap();
            let run = read_run(&top.join(name)).unwrap().unwrap();
            assert_eq!(run[0].name, "getpid");
        }
        assert_eq!(read_run(&top.join("none")).unwrap(), None);
        std::fs::remove_dir_all(&top).unwrap();
    }

    #[test]
    fn places() {
        let m = Meta {
            pin: "netbsd-11".into(),
            system: "netbsd".into(),
            row: "amd64".into(),
            ..Meta::default()
        };
        assert_eq!(m.place(), Path::new("netbsd/amd64/11"));
    }
}
