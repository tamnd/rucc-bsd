//! `rb syntax`: every C unit of a G build through rucc's front end (plan 05.7).
//!
//! Each C unit the reference built is compiled again by rucc with `-fsyntax-only` and the unit's
//! own flags, from the shim's record, in the directory the build ran it in. Each `.S` unit is
//! preprocessed with `-E`. Nothing is linked and nothing boots, so the whole G build is checked
//! in minutes. Three kinds of failure come out:
//! - front end failures;
//! - flags rucc refuses;
//! - warnings that `-Werror` makes errors.
//!
//! A source the build removed after it compiled it, as a `y.tab.c` that is renamed or deleted, is
//! counted as no longer on disk and not as a failure, since there is nothing left to compile.
//!
//! The diagnostics of each unit are then compared with the reference's, as `rb warnings` does: a
//! warning rucc gives that the reference did not counts, even when the unit passes (06.9). The
//! shim keeps only the first bytes of the reference's standard error. When that part is full, a
//! diagnostic of rucc that may come past the cut cannot be judged, so it is listed apart and does
//! not count.

use crate::census::absolute;
use crate::demands;
use crate::warnings::{self, Diagnostic};
use harness_shim::record::CompileRecord;
use harness_shim::run::STDERR_KEEP;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::Path;
use std::process::{Command, Stdio};

/// rucc's arguments for one unit: the persona's, a sysroot when the unit names none, then the
/// build's with `-fsyntax-only`, or `-E` for assembly, in place of `-c`, and without the output
/// and dependency options.
#[must_use]
pub fn args(
    record: &CompileRecord,
    asm: bool,
    persona: &[String],
    sysroot: Option<&str>,
) -> Vec<String> {
    let mut out = persona.to_vec();
    let names_sysroot = record
        .argv
        .iter()
        .any(|a| a == "--sysroot" || a.starts_with("--sysroot="));
    if let Some(dir) = sysroot.filter(|_| !names_sysroot) {
        out.push(format!("--sysroot={dir}"));
    }
    let mode = if asm { "-E" } else { "-fsyntax-only" };
    // replay_args gives the record's added arguments, -E, then the build's; keep the build's.
    let replay = demands::replay_args(record);
    out.push(mode.to_string());
    out.extend(replay.into_iter().skip(record.added.len() + 1));
    out
}

/// What rucc did with one unit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    /// The source, made absolute.
    pub unit: String,
    /// Whether it is assembly.
    pub asm: bool,
    /// Whether the source was no longer on disk, so that rucc did not run.
    pub gone: bool,
    /// The exit status, or `None` when rucc died on a signal or did not start.
    pub exit: Option<i32>,
    /// rucc's standard error, or why it could not run.
    pub stderr: String,
    /// The flag rucc refused, when it said it refused one.
    pub refused: Option<String>,
    /// Diagnostics rucc gave and the reference did not.
    pub only_r: Vec<Diagnostic>,
    /// Diagnostics rucc gave past the part of the reference's standard error the shim kept. They
    /// do not count, since the reference may have given them.
    pub past_cut: Vec<Diagnostic>,
}

impl Outcome {
    /// Whether rucc failed on the unit. A unit whose source is gone did not fail.
    #[must_use]
    pub fn failed(&self) -> bool {
        !self.gone && self.exit != Some(0)
    }

    /// The first error rucc gave, else its first line of standard error.
    #[must_use]
    pub fn first(&self) -> String {
        warnings::parse(&self.stderr)
            .into_iter()
            .find(|d| d.severity == "error")
            .map(|d| d.line())
            .or_else(|| {
                self.stderr
                    .lines()
                    .find(|l| !l.trim().is_empty())
                    .map(|l| l.trim().to_string())
            })
            .unwrap_or_else(|| format!("exit {:?} and nothing on standard error", self.exit))
    }
}

/// The flag a compiler said it does not know, in GCC's words or rucc's.
#[must_use]
pub fn refused(stderr: &str, args: &[String]) -> Option<String> {
    for said in [
        "unrecognized command-line option '",
        "unrecognized command line option '",
        "unknown argument: '",
        "unknown argument '",
        "unsupported option '",
        "unknown option '",
        "unknown option `",
    ] {
        if let Some(at) = stderr.find(said) {
            let flag = stderr[at + said.len()..].split(['\'', '`']).next()?;
            if args.iter().any(|a| a == flag) {
                return Some(flag.to_string());
            }
        }
    }
    None
}

/// Run one unit through rucc and compare its diagnostics with the record's.
#[must_use]
pub fn check(
    rucc: &str,
    record: &CompileRecord,
    source: &str,
    asm: bool,
    persona: &[String],
    sysroot: Option<&str>,
) -> Outcome {
    let args = args(record, asm, persona, sysroot);
    let unit = absolute(&record.cwd, source);
    if !Path::new(&record.cwd).join(source).exists() {
        return Outcome {
            unit,
            asm,
            gone: true,
            exit: None,
            stderr: String::new(),
            refused: None,
            only_r: Vec::new(),
            past_cut: Vec::new(),
        };
    }
    let ran = Command::new(rucc)
        .args(&args)
        .current_dir(&record.cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output();
    let (exit, stderr) = match ran {
        Ok(out) => (
            out.status.code(),
            String::from_utf8_lossy(&out.stderr).into_owned(),
        ),
        Err(e) => (None, format!("cannot run {rucc}: {e}")),
    };
    let (only_r, past_cut) = judge(&record.stderr, &stderr);
    Outcome {
        unit,
        asm,
        gone: false,
        refused: refused(&stderr, &args),
        exit,
        stderr,
        only_r,
        past_cut,
    }
}

/// The diagnostics of `r_err` that the reference's `g_err` does not have: those that count, then
/// those past the cut. When `g_err` filled what the shim keeps, a diagnostic counts only when the
/// reference kept a later line of the same file, and rucc gave it before the last diagnostic it
/// shares with the reference. Both are needed: `#line` can put a later line first, as `byacc` does
/// with the code after its rules. The last line kept may be cut in the middle, so a diagnostic on
/// that line does not count.
#[must_use]
pub fn judge(g_err: &str, r_err: &str) -> (Vec<Diagnostic>, Vec<Diagnostic>) {
    let g = warnings::parse(g_err);
    let r = warnings::parse(r_err);
    let only_r = warnings::compare_unit(&g, &r).only_r;
    if g_err.len() < STDERR_KEEP {
        return (only_r, Vec::new());
    }
    let mut last: BTreeMap<&str, u32> = BTreeMap::new();
    for d in &g {
        let line = last.entry(d.file.as_str()).or_default();
        *line = (*line).max(d.line);
    }
    let shared = r.iter().rposition(|d| d.in_(&g)).unwrap_or(0);
    let before: BTreeSet<_> = r[..shared].iter().map(Diagnostic::key).collect();
    only_r.into_iter().partition(|d| {
        before.contains(&d.key()) && last.get(d.file.as_str()).is_some_and(|&l| d.line < l)
    })
}

/// Run every C and assembly unit of the records through rucc, `jobs` at a time.
#[must_use]
pub fn run(
    records: &[CompileRecord],
    rucc: &str,
    persona: &[String],
    sysroot: Option<&str>,
    jobs: usize,
) -> Vec<Outcome> {
    let units = demands::units(records);
    let tasks: Vec<harness_agent::pool::Task<'_, Outcome>> = units
        .iter()
        .map(|u| {
            let record = &records[u.record];
            Box::new(move || check(rucc, record, &u.source, u.asm, persona, sysroot))
                as harness_agent::pool::Task<'_, _>
        })
        .collect();
    harness_agent::pool::run(jobs, tasks)
}

/// The report, and whether every unit passed with no diagnostic the reference did not give. At
/// most `limit` lines of each list are printed.
#[must_use]
pub fn report(title: &str, outcomes: &[Outcome], limit: usize) -> (String, bool) {
    let mut s = String::new();
    let asm = outcomes.iter().filter(|o| o.asm).count();
    let gone: Vec<&Outcome> = outcomes.iter().filter(|o| o.gone).collect();
    let failed: Vec<&Outcome> = outcomes.iter().filter(|o| o.failed()).collect();
    let mut refused: BTreeMap<&str, usize> = BTreeMap::new();
    for o in &failed {
        if let Some(flag) = &o.refused {
            *refused.entry(flag.as_str()).or_default() += 1;
        }
    }
    let noisy: Vec<&Outcome> = outcomes
        .iter()
        .filter(|o| !o.failed() && !o.only_r.is_empty())
        .collect();
    let cut: Vec<&Outcome> = outcomes
        .iter()
        .filter(|o| !o.failed() && o.only_r.is_empty() && !o.past_cut.is_empty())
        .collect();
    let _ = writeln!(
        s,
        "rb syntax {title}: {} units, {} C and {asm} assembly, {} no longer on disk. Failed: {}, of which {} refused a flag. Passed with a diagnostic the reference did not give: {}.",
        outcomes.len(),
        outcomes.len() - asm,
        gone.len(),
        failed.len(),
        refused.values().sum::<usize>(),
        noisy.len()
    );
    if !cut.is_empty() {
        let _ = writeln!(
            s,
            "Passed with diagnostics past the {STDERR_KEEP} bytes of the reference's standard error the shim keeps, which do not count: {}.",
            cut.len()
        );
    }
    if !refused.is_empty() {
        let _ = writeln!(s, "\nRefused flags:");
        for (flag, n) in &refused {
            let _ = writeln!(s, "  {flag}: {n} units");
        }
    }
    if !failed.is_empty() {
        let _ = writeln!(s, "\nFailed units, each with rucc's first error:");
        for o in failed.iter().take(limit) {
            let _ = writeln!(s, "  {}\n    {}", o.unit, o.first());
        }
        if failed.len() > limit {
            let _ = writeln!(s, "  and {} more", failed.len() - limit);
        }
    }
    if !gone.is_empty() {
        let _ = writeln!(
            s,
            "\nUnits no longer on disk, which the build removed after it compiled them:"
        );
        for o in gone.iter().take(limit) {
            let _ = writeln!(s, "  {}", o.unit);
        }
        if gone.len() > limit {
            let _ = writeln!(s, "  and {} more", gone.len() - limit);
        }
    }
    if !noisy.is_empty() {
        let _ = writeln!(
            s,
            "\nUnits that passed, with diagnostics the reference did not give:"
        );
        for o in noisy.iter().take(limit) {
            let _ = writeln!(s, "  {}", o.unit);
            for d in &o.only_r {
                let _ = writeln!(s, "    {}", d.line());
            }
        }
        if noisy.len() > limit {
            let _ = writeln!(s, "  and {} more", noisy.len() - limit);
        }
    }
    if !cut.is_empty() {
        let _ = writeln!(
            s,
            "\nUnits that passed, with diagnostics past the part of the reference's standard error the shim kept:"
        );
        for o in cut.iter().take(limit) {
            let _ = writeln!(s, "  {}", o.unit);
            for d in &o.past_cut {
                let _ = writeln!(s, "    {}", d.line());
            }
        }
        if cut.len() > limit {
            let _ = writeln!(s, "  and {} more", cut.len() - limit);
        }
    }
    (s, failed.is_empty() && noisy.is_empty())
}

/// Every unit as a line of tab-separated values: the unit, `pass`, `warn`, `cut`, `fail` or
/// `gone`, and the first error or the first diagnostic the reference did not give.
#[must_use]
pub fn tsv(outcomes: &[Outcome]) -> String {
    let mut s = String::from("unit\tresult\tfirst\n");
    for o in outcomes {
        let (result, first) = if o.gone {
            ("gone", String::new())
        } else if o.failed() {
            ("fail", o.first())
        } else if let Some(d) = o.only_r.first() {
            ("warn", d.line())
        } else if let Some(d) = o.past_cut.first() {
            ("cut", d.line())
        } else {
            ("pass", String::new())
        };
        let _ = writeln!(s, "{}\t{result}\t{}", o.unit, first.replace('\t', " "));
    }
    s
}

/// `rb syntax` on the command line.
pub mod cli {
    use super::{report, run, tsv};
    use crate::cli::Args;
    use crate::shims::Rucc;
    use crate::toolchain::cli::Files;
    use harness_shim::record::read_log;
    use std::path::Path;

    /// Run `rb syntax`. Returns the report and whether every unit passed.
    pub fn run_cli(args: &Args, root: &Path) -> Result<(String, bool), String> {
        let files = Files::load(root, args.target.as_deref(), args.get("row"))?;
        let log = args
            .get("log")
            .ok_or("give --log, the compile.jsonl of a G build")?;
        let rucc_pin = files.toolchains.rucc.as_ref();
        let rucc = Rucc::find(
            args.get("rucc"),
            std::env::var("RB_RUCC").ok().as_deref(),
            rucc_pin,
            std::env::var_os("PATH"),
        )?;
        let described = Rucc::describe(&rucc, rucc_pin)?;
        let persona = files.expected()?.rucc_args;
        let number = |key: &str, default: usize| -> Result<usize, String> {
            args.get(key).map_or(Ok(default), |n| {
                n.parse()
                    .map_err(|_| format!("--{key} {n} is not a number"))
            })
        };
        let jobs = number("jobs", harness_agent::pool::default_jobs(1))?;
        let limit = number("limit", 30)?;
        let (records, _) = read_log(Path::new(log)).map_err(|e| format!("{log}: {e}"))?;
        let outcomes = run(
            &records,
            &rucc.display().to_string(),
            &persona,
            args.get("sysroot"),
            jobs,
        );
        if let Some(out) = args.get("out") {
            std::fs::write(out, tsv(&outcomes)).map_err(|e| format!("{out}: {e}"))?;
        }
        let title = format!("{} with {}", files.pin.name, described.line(rucc_pin));
        Ok(report(&title, &outcomes, limit))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use harness_shim::record::parse_log;

    fn record(argv: &str, stderr: &str) -> CompileRecord {
        let words: Vec<String> = argv
            .split_whitespace()
            .map(|w| format!("\"{w}\""))
            .collect();
        let line = format!(
            r#"{{"started":1,"argv":[{}],"compiler":"/gcc","cwd":"/obj/lib","inputs":[{{"path":"a.c","sha256":"x"}}],"outputs":[{{"path":"a.o","sha256":"y"}}],"wall-seconds":0.1,"exit":0,"stderr":"{stderr}","slot":"cc","side":"G","origin":"ref"}}"#,
            words.join(",")
        );
        parse_log(&line).0.remove(0)
    }

    fn words(line: &str) -> Vec<String> {
        line.split_whitespace().map(String::from).collect()
    }

    #[test]
    fn the_replay_checks_syntax_with_the_units_own_flags() {
        let r = record(
            "x86_64--netbsd-gcc -O2 -Werror -MD -MF a.d -c a.c -o a.o",
            "",
        );
        let persona = words("--target=x86_64-unknown-netbsd -fgnuc-version=14.4.0");
        assert_eq!(
            args(&r, false, &persona, Some("/d")),
            words(
                "--target=x86_64-unknown-netbsd -fgnuc-version=14.4.0 --sysroot=/d -fsyntax-only -O2 -Werror a.c"
            )
        );
        let s = record("cc --sysroot=/x -c a.S -o a.o", "");
        assert_eq!(
            args(&s, true, &[], Some("/d")),
            words("-E --sysroot=/x a.S")
        );
    }

    #[test]
    fn a_refused_flag_is_named() {
        let a = words("-O2 -fno-foo a.c");
        assert_eq!(
            refused("rucc: error: unknown argument: '-fno-foo'\n", &a).as_deref(),
            Some("-fno-foo")
        );
        assert_eq!(refused("a.c:1:1: error: x\n", &a), None);
        // rucc quotes the flag with backticks.
        let a = words("-nostdinc++ a.c");
        assert_eq!(
            refused("(command line): error: unknown option `-nostdinc++`\n", &a).as_deref(),
            Some("-nostdinc++")
        );
    }

    #[test]
    fn a_diagnostic_past_the_kept_stderr_does_not_count() {
        let w =
            |file: &str, line: u32, opt: &str| format!("{file}:{line}:1: warning: x [-W{opt}]\n");
        // The reference's standard error is full: a.h to line 30, then a.c to line 9, cut there.
        let mut g = String::new();
        while g.len() < STDERR_KEEP - 20 {
            g += &w("a.h", 30, "a");
        }
        g += &w("a.c", 9, "b");
        g.truncate(STDERR_KEEP);
        let r = [
            w("a.h", 12, "c"),
            w("a.h", 30, "a"),
            w("a.h", 31, "d"),
            w("a.c", 9, "e"),
            w("b.h", 2, "f"),
        ]
        .concat();
        let (count, past) = judge(&g, &r);
        let lines = |v: &[Diagnostic]| {
            v.iter()
                .map(|d| (d.file.clone(), d.line))
                .collect::<Vec<_>>()
        };
        assert_eq!(lines(&count), [("a.h".to_string(), 12)]);
        assert_eq!(
            lines(&past),
            [
                ("a.h".to_string(), 31),
                ("a.c".to_string(), 9),
                ("b.h".to_string(), 2)
            ]
        );
        // rucc gave a.h:12 after the last diagnostic it shares with the reference, as with a
        // `#line` that goes back.
        let back = [w("a.h", 30, "a"), w("a.h", 12, "c")].concat();
        let (count, past) = judge(&g, &back);
        assert!(count.is_empty(), "{count:?}");
        assert_eq!(lines(&past), [("a.h".to_string(), 12)]);
        // When the reference's standard error is not full, every diagnostic counts.
        let (count, past) = judge(&w("a.h", 30, "a"), &r);
        assert_eq!(count.len(), 4);
        assert!(past.is_empty());
    }

    #[test]
    fn a_fake_rucc_gives_each_kind_of_result() {
        let dir = std::env::temp_dir().join(format!("rb-syntax-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let rucc = dir.join("rucc");
        std::fs::write(
            &rucc,
            "#!/bin/sh\ncase \"$*\" in\n*bad.c*) echo 'bad.c:2:3: error: expected expression' >&2; exit 1 ;;\n*noisy.c*) echo 'noisy.c:4:1: warning: q [-Wq]' >&2 ;;\nesac\nexit 0\n",
        )
        .unwrap();
        std::process::Command::new("chmod")
            .arg("+x")
            .arg(&rucc)
            .status()
            .unwrap();
        for src in ["good.c", "bad.c", "noisy.c", "quiet.c", "start.S"] {
            std::fs::write(dir.join(src), "").unwrap();
        }
        let line = |src: &str, err: &str| {
            format!(
                r#"{{"started":1,"argv":["cc","-c","{src}"],"compiler":"/gcc","cwd":"{}","inputs":[{{"path":"{src}","sha256":"x"}}],"outputs":[{{"path":"x.o","sha256":"y"}}],"wall-seconds":0.1,"exit":0,"stderr":"{err}","slot":"cc"}}"#,
                dir.display()
            )
        };
        let log = [
            line("good.c", ""),
            line("bad.c", ""),
            line("noisy.c", ""),
            line("quiet.c", "quiet.c:1:1: warning: v [-Wv]\\n"),
            line("start.S", ""),
            line("y.tab.c", ""),
        ]
        .join("\n");
        let records = parse_log(&log).0;
        let outcomes = run(&records, &rucc.display().to_string(), &[], None, 2);
        std::fs::remove_dir_all(&dir).ok();
        assert_eq!(outcomes.len(), 6);
        let (text, ok) = report("t", &outcomes, 10);
        assert!(!ok);
        // The build removed y.tab.c, so it is counted apart and is not a failure.
        assert!(
            text.contains("6 units, 5 C and 1 assembly, 1 no longer on disk. Failed: 1,"),
            "{text}"
        );
        assert!(text.contains("no longer on disk, which"), "{text}");
        assert!(
            text.contains("bad.c:2: error: expected expression"),
            "{text}"
        );
        assert!(text.contains("noisy.c:4: warning: q [-Wq]"), "{text}");
        assert!(!text.contains("quiet.c\n"), "{text}");
        let table = tsv(&outcomes);
        assert!(
            table.contains("\tfail\t") && table.contains("\twarn\t") && table.contains("\tgone\t"),
            "{table}"
        );
    }
}
