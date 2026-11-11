//! `rb speed`: the compile time and size gates of 02.5, from the records of two builds (plan 13.4).
//!
//! The shim writes one record per compiler call to `compile.jsonl`, with the user and system CPU
//! time of the compiler and its children and the peak resident memory of the largest process. G's
//! records come from the same shim around the reference GCC, so both sides count the driver, the
//! compiler proper and the assembler.
//!
//! Only C units and `.S` files given to the C compiler slot count. Probes, links, C++ units and
//! calls that failed are left out. Units are paired by object path, with the roots replaced as for
//! `rb flags-diff`. A unit in one build only makes the comparison invalid until it is explained.
//!
//! The gates:
//! - the sum of R's CPU time over the paired units is at most half of G's;
//! - no rucc process has a peak resident memory above 1.5 GiB;
//! - R's text, the size of the executable sections of the kernel and of `libc.so`, is within 5%
//!   of G's.
//!
//! The per unit ratio, its percentiles and the worst units use only units where G took at least
//! [`MIN_SECONDS`], because the clock of `rusage` counts in ticks and a 2 ms unit can show 0.

use crate::objdiff::unit;
use harness_shim::record::CompileRecord;
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::Path;

/// The most R's CPU time can be, as a fraction of G's (02.5).
pub const CPU_LIMIT: f64 = 0.5;

/// The most peak resident memory a rucc process can have, in KiB: 1.5 GiB (02.5).
pub const RSS_LIMIT_KB: u64 = 1536 * 1024;

/// How far R's text can be from G's, in percent of G's (02.5).
pub const TEXT_PERCENT: u64 = 5;

/// The least CPU time of G, in seconds, for a unit to count in the per unit ratios.
pub const MIN_SECONDS: f64 = 0.01;

/// A probe that takes longer than this, in seconds of wall time, is a bug (05.8).
pub const PROBE_LIMIT: f64 = 0.02;

/// One unit of one build.
#[derive(Debug, Clone, PartialEq)]
pub struct Timed {
    /// The source, absolute, with the root replaced.
    pub source: String,
    /// User and system CPU time, or `None` when the record has no `rusage`.
    pub cpu: Option<f64>,
    /// Peak resident memory in KiB, when the record has it.
    pub rss_kb: Option<u64>,
}

/// Whether the source is a C unit or a `.S` file.
fn counted_source(source: &str) -> bool {
    Path::new(source)
        .extension()
        .is_some_and(|e| e == "c" || e == "S")
}

/// The units of a build that count, keyed by object path with `root` replaced by `<root>`. When
/// two calls made the same object, the last one counts.
#[must_use]
pub fn timed_units(records: &[CompileRecord], root: &str) -> BTreeMap<String, Timed> {
    records
        .iter()
        .filter(|r| r.slot.as_deref().unwrap_or("cc") == "cc")
        .filter_map(|r| {
            let (key, u) = unit(r, root)?;
            counted_source(&u.source).then(|| {
                let cpu = match (r.user_seconds, r.system_seconds) {
                    (Some(user), Some(system)) => Some(user + system),
                    _ => None,
                };
                (
                    key,
                    Timed {
                        source: u.source,
                        cpu,
                        rss_kb: r.peak_rss_kb,
                    },
                )
            })
        })
        .collect()
}

/// One unit in both builds.
#[derive(Debug, Clone, PartialEq)]
pub struct Paired {
    /// The source, from R's record.
    pub source: String,
    /// G's CPU time in seconds.
    pub g: f64,
    /// R's CPU time in seconds.
    pub r: f64,
    /// R's peak resident memory in KiB, when known.
    pub rss_kb: Option<u64>,
}

impl Paired {
    /// R's time over G's, or `None` when G took less than [`MIN_SECONDS`].
    #[must_use]
    pub fn ratio(&self) -> Option<f64> {
        (self.g >= MIN_SECONDS).then(|| self.r / self.g)
    }
}

/// What the compile records of two builds gave.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Compile {
    /// The units in both builds with CPU times on both sides, by key.
    pub paired: BTreeMap<String, Paired>,
    /// The units of G only.
    pub only_g: Vec<String>,
    /// The units of R only.
    pub only_r: Vec<String>,
    /// The units in both builds with no CPU time on one side.
    pub untimed: usize,
    /// The largest peak resident memory of any call of R's C compiler, in KiB, and its first
    /// input. The C++ slot is G's `g++` in both builds, so it does not count.
    pub max_rss: Option<(u64, String)>,
    /// The time from the first call of G to the end of its last, in seconds.
    pub wall_g: f64,
    /// The same for R.
    pub wall_r: f64,
    /// R's probes slower than [`PROBE_LIMIT`]: wall time, directory and first input.
    pub slow_probes: Vec<(f64, String, String)>,
}

/// The time from the start of the first call to the end of the last.
fn span(records: &[CompileRecord]) -> f64 {
    let start = records
        .iter()
        .map(|r| r.started)
        .fold(f64::INFINITY, f64::min);
    let end = records
        .iter()
        .map(|r| r.started + r.wall_seconds)
        .fold(f64::NEG_INFINITY, f64::max);
    if end > start { end - start } else { 0.0 }
}

/// The first input of a call, or `(stdin)`.
fn first_input(r: &CompileRecord) -> String {
    r.inputs
        .first()
        .map_or_else(|| "(stdin)".to_string(), |i| i.path.clone())
}

/// Compare the records of G and R.
#[must_use]
pub fn compare(g: &[CompileRecord], g_root: &str, r: &[CompileRecord], r_root: &str) -> Compile {
    let g_units = timed_units(g, g_root);
    let r_units = timed_units(r, r_root);
    let mut out = Compile {
        wall_g: span(g),
        wall_r: span(r),
        ..Compile::default()
    };
    for (key, gu) in &g_units {
        let Some(ru) = r_units.get(key) else {
            out.only_g.push(key.clone());
            continue;
        };
        match (gu.cpu, ru.cpu) {
            (Some(g), Some(r)) => {
                out.paired.insert(
                    key.clone(),
                    Paired {
                        source: ru.source.clone(),
                        g,
                        r,
                        rss_kb: ru.rss_kb,
                    },
                );
            }
            _ => out.untimed += 1,
        }
    }
    out.only_r = r_units
        .keys()
        .filter(|k| !g_units.contains_key(*k))
        .cloned()
        .collect();
    out.max_rss = r
        .iter()
        .filter(|c| c.slot.as_deref().unwrap_or("cc") == "cc")
        .filter_map(|c| c.peak_rss_kb.map(|kb| (kb, first_input(c))))
        .max_by_key(|(kb, _)| *kb);
    out.slow_probes = r
        .iter()
        .filter(|c| c.probe && c.wall_seconds > PROBE_LIMIT)
        .map(|c| (c.wall_seconds, c.cwd.clone(), first_input(c)))
        .collect();
    out.slow_probes.sort_by(|a, b| b.0.total_cmp(&a.0));
    out
}

impl Compile {
    /// The summed CPU time of G and of R over the paired units.
    #[must_use]
    pub fn sums(&self) -> (f64, f64) {
        self.paired
            .values()
            .fold((0.0, 0.0), |(g, r), p| (g + p.g, r + p.r))
    }

    /// R's summed CPU time over G's, or `None` when G's is zero.
    #[must_use]
    pub fn ratio(&self) -> Option<f64> {
        let (g, r) = self.sums();
        (g > 0.0).then(|| r / g)
    }

    /// The per unit ratios, in ascending order.
    #[must_use]
    pub fn ratios(&self) -> Vec<f64> {
        let mut out: Vec<f64> = self.paired.values().filter_map(Paired::ratio).collect();
        out.sort_by(f64::total_cmp);
        out
    }

    /// The units with the highest ratio, worst first.
    #[must_use]
    pub fn worst(&self, limit: usize) -> Vec<(&String, &Paired, f64)> {
        let mut out: Vec<_> = self
            .paired
            .iter()
            .filter_map(|(k, p)| p.ratio().map(|x| (k, p, x)))
            .collect();
        out.sort_by(|a, b| b.2.total_cmp(&a.2).then_with(|| a.0.cmp(b.0)));
        out.truncate(limit);
        out
    }

    /// Whether the CPU gate passed.
    #[must_use]
    pub fn cpu_passed(&self) -> bool {
        self.ratio().is_some_and(|r| r <= CPU_LIMIT)
    }

    /// Whether the memory gate passed. With no memory in the records it did not.
    #[must_use]
    pub fn rss_passed(&self) -> bool {
        self.max_rss
            .as_ref()
            .is_some_and(|(kb, _)| *kb <= RSS_LIMIT_KB)
    }

    /// Whether the units of the two builds are the same.
    #[must_use]
    pub fn matched(&self) -> bool {
        self.only_g.is_empty() && self.only_r.is_empty() && self.untimed == 0
    }

    /// The report, with at most `limit` lines in each list.
    #[must_use]
    pub fn report(&self, limit: usize) -> String {
        let mut out = String::new();
        let _ = writeln!(
            out,
            "units: {} paired, {} only in G, {} only in R, {} without CPU time",
            self.paired.len(),
            self.only_g.len(),
            self.only_r.len(),
            self.untimed
        );
        for (side, keys) in [("G", &self.only_g), ("R", &self.only_r)] {
            for key in keys.iter().take(limit) {
                let _ = writeln!(out, "  only in {side}: {key}");
            }
        }
        let (g, r) = self.sums();
        match self.ratio() {
            Some(ratio) => {
                let _ = writeln!(
                    out,
                    "CPU: G {g:.1} s, R {r:.1} s, ratio {ratio:.3} (gate <= {CPU_LIMIT}): {}",
                    verdict(self.cpu_passed())
                );
            }
            None => {
                let _ = writeln!(out, "CPU: no CPU time in G's records: fail");
            }
        }
        let ratios = self.ratios();
        let pct =
            |p| harness_stats::percentile(&ratios, p).map_or("-".into(), |x| format!("{x:.3}"));
        let _ = writeln!(
            out,
            "per unit ratio over {} units with G >= {MIN_SECONDS} s: p50 {} p90 {} p99 {}",
            ratios.len(),
            pct(50.0),
            pct(90.0),
            pct(99.0)
        );
        match &self.max_rss {
            Some((kb, input)) => {
                let _ = writeln!(
                    out,
                    "largest peak RSS of R: {} MiB (gate <= {} MiB): {}, {input}",
                    kb / 1024,
                    RSS_LIMIT_KB / 1024,
                    verdict(self.rss_passed())
                );
            }
            None => {
                let _ = writeln!(out, "largest peak RSS of R: none in the records: fail");
            }
        }
        let _ = writeln!(
            out,
            "wall time of the compile records: G {:.0} s, R {:.0} s (reported, not gated)",
            self.wall_g, self.wall_r
        );
        let _ = writeln!(
            out,
            "probes of R slower than {} ms: {}",
            PROBE_LIMIT * 1000.0,
            self.slow_probes.len()
        );
        for (wall, cwd, input) in self.slow_probes.iter().take(limit) {
            let _ = writeln!(out, "  {:.0} ms  {cwd}  {input}", wall * 1000.0);
        }
        let worst = self.worst(limit);
        let _ = writeln!(out, "the {} units with the worst ratio:", worst.len());
        let _ = writeln!(out, "  ratio    G s      R s      RSS MiB  source");
        for (_, p, ratio) in worst {
            let _ = writeln!(
                out,
                "  {ratio:<8.3} {:<8.3} {:<8.3} {:<8} {}",
                p.g,
                p.r,
                p.rss_kb.map_or("-".into(), |kb| (kb / 1024).to_string()),
                p.source
            );
        }
        out
    }

    /// One line per paired unit as tab-separated values: key, source, G, R, ratio, R's RSS.
    #[must_use]
    pub fn tsv(&self) -> String {
        let mut out = String::from("unit\tsource\tg_seconds\tr_seconds\tratio\tr_rss_kb\n");
        for (key, p) in &self.paired {
            let _ = writeln!(
                out,
                "{key}\t{}\t{:.6}\t{:.6}\t{}\t{}",
                p.source,
                p.g,
                p.r,
                p.ratio().map_or(String::new(), |x| format!("{x:.4}")),
                p.rss_kb.map_or(String::new(), |kb| kb.to_string())
            );
        }
        out
    }
}

/// `pass` or `fail`.
fn verdict(passed: bool) -> &'static str {
    if passed { "pass" } else { "fail" }
}

/// The size of the executable sections of an object, in bytes.
pub fn text_size(path: &Path) -> Result<u64, String> {
    use object::{Object as _, ObjectSection as _, SectionKind};
    let data = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let file = object::File::parse(&*data).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(file
        .sections()
        .filter(|s| s.kind() == SectionKind::Text)
        .map(|s| s.size())
        .sum())
}

/// One pair of files compared by text size.
#[derive(Debug, Clone, PartialEq)]
pub struct Text {
    /// G's file.
    pub g: String,
    /// R's file.
    pub r: String,
    /// G's text in bytes.
    pub g_bytes: u64,
    /// R's text in bytes.
    pub r_bytes: u64,
}

impl Text {
    /// R's text over G's.
    #[must_use]
    #[allow(clippy::cast_precision_loss)]
    pub fn ratio(&self) -> f64 {
        if self.g_bytes == 0 {
            f64::INFINITY
        } else {
            self.r_bytes as f64 / self.g_bytes as f64
        }
    }

    /// Whether R's text is within [`TEXT_PERCENT`] of G's, counted in whole bytes.
    #[must_use]
    pub fn passed(&self) -> bool {
        u128::from(self.r_bytes.abs_diff(self.g_bytes)) * 100
            <= u128::from(self.g_bytes) * u128::from(TEXT_PERCENT)
    }

    /// The report line.
    #[must_use]
    pub fn line(&self) -> String {
        format!(
            "text of {}: G {} bytes, R {} bytes, ratio {:.4} (gate within {}%): {}",
            Path::new(&self.r)
                .file_name()
                .map_or_else(|| self.r.clone(), |n| n.to_string_lossy().into_owned()),
            self.g_bytes,
            self.r_bytes,
            self.ratio(),
            TEXT_PERCENT,
            verdict(self.passed())
        )
    }
}

/// The command line half.
pub mod cli {
    use super::{Text, compare, text_size};
    use crate::cli::Args;
    use harness_shim::record::read_log;
    use std::fmt::Write as _;
    use std::path::Path;

    /// The comma separated words of an option.
    fn list(args: &Args, name: &str) -> Vec<String> {
        args.get(name)
            .map(|s| {
                s.split(',')
                    .filter(|p| !p.is_empty())
                    .map(String::from)
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Run `rb speed`. Returns the report and whether every gate passed.
    pub fn run(args: &Args) -> Result<(String, bool), String> {
        let g_text = list(args, "g-text");
        let r_text = list(args, "r-text");
        if g_text.len() != r_text.len() {
            return Err("give as many files to --g-text as to --r-text".into());
        }
        let logs = (args.get("g"), args.get("r"));
        if logs.0.is_some() != logs.1.is_some() {
            return Err("give both --g and --r, the compile.jsonl of each build".into());
        }
        if logs.0.is_none() && g_text.is_empty() {
            return Err("give --g and --r, or --g-text and --r-text".into());
        }
        let limit = args
            .get("limit")
            .map(str::parse)
            .transpose()
            .map_err(|e| format!("--limit: {e}"))?
            .unwrap_or(50);
        let mut out = String::new();
        let mut ok = true;
        if let Some(build) = args.get("build") {
            let _ = writeln!(out, "build: {build}");
        }
        if let (Some(g), Some(r)) = logs {
            let read = |log: &str| {
                let (records, skipped) =
                    read_log(Path::new(log)).map_err(|e| format!("{log}: {e}"))?;
                Ok::<_, String>((records, skipped))
            };
            let (g_records, g_skipped) = read(g)?;
            let (r_records, r_skipped) = read(r)?;
            let _ = writeln!(
                out,
                "G: {g}: {} records, {g_skipped} unreadable lines\nR: {r}: {} records, {r_skipped} unreadable lines",
                g_records.len(),
                r_records.len()
            );
            let found = compare(
                &g_records,
                args.get("g-root").unwrap_or_default(),
                &r_records,
                args.get("r-root").unwrap_or_default(),
            );
            out.push_str(&found.report(limit));
            ok &= found.cpu_passed() && found.rss_passed() && found.matched();
            if let Some(path) = args.get("units") {
                std::fs::write(path, found.tsv()).map_err(|e| format!("{path}: {e}"))?;
            }
        }
        for (g, r) in g_text.iter().zip(&r_text) {
            let text = Text {
                g_bytes: text_size(Path::new(g))?,
                r_bytes: text_size(Path::new(r))?,
                g: g.clone(),
                r: r.clone(),
            };
            let _ = writeln!(out, "{}", text.line());
            ok &= text.passed();
        }
        let _ = writeln!(out, "gates: {}", super::verdict(ok));
        Ok((out, ok))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use harness_shim::record::parse_log;

    fn record(out: &str, src: &str, user: f64, rss: u64, extra: &str) -> String {
        format!(
            r#"{{"started":100,"argv":["cc","-c","{src}","-o","{out}"],"compiler":"cc","added":[],"cwd":"/w","env":{{}},"inputs":[{{"path":"{src}","sha256":""}}],"outputs":[{{"path":"{out}","sha256":""}}],"wall-seconds":{user},"user-seconds":{user},"system-seconds":0.0,"peak-rss-kb":{rss},"exit":0,"stderr":"","rucc":[]{extra}}}"#
        )
    }

    fn log(lines: &[String]) -> Vec<CompileRecord> {
        parse_log(&lines.join("\n")).0
    }

    #[test]
    fn units_pair_by_object_and_sum_their_cpu_time() {
        let g = log(&[
            record("/g/a.o", "a.c", 1.0, 1000, ""),
            record("/g/b.o", "b.S", 2.0, 1000, ""),
            record("/g/c.o", "c.cc", 5.0, 1000, ""),
            record("/g/p.o", "p.c", 5.0, 1000, r#","probe":true"#),
            record("/g/x.o", "x.c", 1.0, 1000, ""),
        ]);
        let r = log(&[
            record("/r/a.o", "a.c", 0.25, 2048, ""),
            record("/r/b.o", "b.S", 1.75, 4096, ""),
            record("/r/c.o", "c.cc", 1.0, 9_000_000, r#","slot":"c++""#),
            record("/r/p.o", "p.c", 0.03, 1000, r#","probe":true"#),
            record("/r/y.o", "y.c", 1.0, 1000, ""),
        ]);
        let found = compare(&g, "/g", &r, "/r");
        assert_eq!(found.paired.len(), 2);
        assert_eq!(found.only_g, vec!["<root>/x.o".to_string()]);
        assert_eq!(found.only_r, vec!["<root>/y.o".to_string()]);
        assert_eq!(found.sums(), (3.0, 2.0));
        assert!(!found.cpu_passed());
        assert!(!found.matched());
        assert_eq!(found.max_rss.as_ref().unwrap().0, 4096);
        assert!(found.rss_passed());
        assert_eq!(found.slow_probes.len(), 1);
        let worst = found.worst(1);
        assert_eq!(worst[0].0, "<root>/b.o");
        assert!((worst[0].2 - 0.875).abs() < 1e-9);
        let report = found.report(5);
        assert!(report.contains("ratio 0.667"), "{report}");
        assert!(report.contains("only in G: <root>/x.o"), "{report}");
    }

    #[test]
    fn a_small_unit_has_no_ratio_and_a_fast_build_passes() {
        let g = log(&[
            record("/g/a.o", "a.c", 4.0, 1000, ""),
            record("/g/t.o", "t.c", 0.001, 1000, ""),
        ]);
        let r = log(&[
            record("/r/a.o", "a.c", 1.0, 1000, ""),
            record("/r/t.o", "t.c", 0.004, 1000, ""),
        ]);
        let found = compare(&g, "/g", &r, "/r");
        assert_eq!(found.ratios(), vec![0.25]);
        assert!(found.cpu_passed() && found.rss_passed() && found.matched());
        assert_eq!(found.tsv().lines().count(), 3);
    }

    #[test]
    fn a_record_without_rusage_is_untimed() {
        let g = log(&[record("/g/a.o", "a.c", 1.0, 1000, "")]);
        let r = parse_log(
            r#"{"started":1,"argv":["cc","-c","a.c","-o","/r/a.o"],"compiler":"cc","added":[],"cwd":"/w","env":{},"inputs":[{"path":"a.c","sha256":""}],"outputs":[{"path":"/r/a.o","sha256":""}],"wall-seconds":1,"exit":0,"stderr":"","rucc":[]}"#,
        )
        .0;
        let found = compare(&g, "/g", &r, "/r");
        assert_eq!(found.untimed, 1);
        assert!(found.ratio().is_none());
        assert!(!found.rss_passed());
    }

    #[test]
    fn text_is_within_five_percent_either_way() {
        let text = |g, r| Text {
            g: "g/libc.so".into(),
            r: "r/libc.so".into(),
            g_bytes: g,
            r_bytes: r,
        };
        assert!(text(1000, 1050).passed());
        assert!(text(1000, 950).passed());
        assert!(!text(1000, 1051).passed());
        assert!(!text(1000, 949).passed());
        assert!(text(1000, 1000).line().contains("libc.so"));
    }

    #[test]
    fn the_text_of_an_object_is_its_executable_sections() {
        use object::write::Object as Writer;
        use object::{Architecture, BinaryFormat, Endianness, SectionKind};
        let mut w = Writer::new(BinaryFormat::Elf, Architecture::X86_64, Endianness::Little);
        let t = w.add_section(Vec::new(), b".text".to_vec(), SectionKind::Text);
        w.append_section_data(t, &[0x90; 40], 16);
        let d = w.add_section(Vec::new(), b".data".to_vec(), SectionKind::Data);
        w.append_section_data(d, &[0; 100], 8);
        let dir = std::env::temp_dir().join(format!("rb-speed-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("t.o");
        std::fs::write(&path, w.write().unwrap()).unwrap();
        assert_eq!(text_size(&path).unwrap(), 40);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
