//! `rb cross-modules`: the kernel modules of one build in the kernel of another (plan 09.3).
//!
//! Two builds give four cells. The first letter names the kernel and the second the modules, as
//! in the 2x2 of `rb test`:
//!
//! | | G modules | R modules |
//! |---|---|---|
//! | G kernel | GG, the baseline | GR |
//! | R kernel | RG | RR |
//!
//! The command has two forms.
//!
//! With `--image` and `--out`, it runs one cell. It boots the image with the modules suite of
//! rb-rc, which loads, lists and unloads each module, one at a time. `--modules DIR` gives the
//! modules of the other build. When a boot ends in a panic or a timeout while a module was
//! loading, that module gets the boot's result, and the next boot skips it. The cell ends when a
//! boot passes, or after `--attempts` boots. `OUT/modules.tsv` gets the result of each module.
//!
//! With `--gg DIR` and one or more of `--rg`, `--gr` and `--rr`, it compares the cells. Each DIR is
//! the `--out` of the first form. A module that passes in GG and does not pass in another cell is
//! a finding: an ABI difference between the module and the kernel. The other cells say which side
//! has it.

use crate::boot::{Outcome, Request};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::Path;

/// The cells other than GG, in the order the report gives them.
pub const CELLS: [&str; 3] = ["RG", "GR", "RR"];

/// The result of one module in one cell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Module {
    /// A result of the modules suite (`pass`, `present`, `skip`, `load`, `list` or `unload`), or
    /// the result of the boot that stopped while the module loaded, such as `panic`.
    pub result: String,
    /// The boot that gave the result, from 1.
    pub boot: u32,
    /// The first line of the error, or the panic message.
    pub error: String,
}

impl Module {
    /// Whether the module works in the kernel: it loaded and unloaded, or the kernel has it.
    #[must_use]
    pub fn ok(&self) -> bool {
        self.result == "pass" || self.result == "present"
    }
}

/// The modules of one cell, by name.
pub type Cell = BTreeMap<String, Module>;

/// The lines of the suite's `modules.tsv`: name, result and error.
#[must_use]
pub fn guest_rows(text: &str) -> Vec<(String, String, String)> {
    text.lines()
        .filter_map(|l| {
            let mut f = l.splitn(3, '\t');
            let name = f.next()?.trim();
            let result = f.next()?.trim();
            if name.is_empty() || result.is_empty() {
                return None;
            }
            Some((
                name.to_string(),
                result.to_string(),
                f.next().unwrap_or("").trim().to_string(),
            ))
        })
        .collect()
}

/// Add the rows of one boot to the cell. A later result takes the place of an earlier one, but a
/// skip does not: a module is skipped because an earlier boot stopped in it.
pub fn merge(cell: &mut Cell, boot: u32, rows: &[(String, String, String)]) {
    for (name, result, error) in rows {
        if result == "skip" && cell.contains_key(name) {
            continue;
        }
        cell.insert(
            name.clone(),
            Module {
                result: result.clone(),
                boot,
                error: error.clone(),
            },
        );
    }
}

/// The module a boot that did not pass stopped in: the last `RB-CASE modules/NAME`, when the
/// suite did not write a line for that module.
#[must_use]
pub fn stopped_in(o: &Outcome, rows: &[(String, String, String)]) -> Option<String> {
    if o.passed() {
        return None;
    }
    let name = o.seen.case.as_deref()?.strip_prefix("modules/")?;
    if name.is_empty() || rows.iter().any(|(n, _, _)| n == name) {
        return None;
    }
    Some(name.to_string())
}

/// The cell as `modules.tsv`: name, result, boot and error, with a tab between them.
#[must_use]
pub fn render(cell: &Cell) -> String {
    let mut s = String::new();
    for (name, m) in cell {
        let _ = writeln!(s, "{name}\t{}\t{}\t{}", m.result, m.boot, m.error);
    }
    s
}

/// Read a cell's `modules.tsv` back.
pub fn parse(text: &str) -> Result<Cell, String> {
    let mut cell = Cell::new();
    for (i, line) in text.lines().enumerate() {
        let f: Vec<&str> = line.splitn(4, '\t').collect();
        if f.len() < 3 {
            return Err(format!("line {}: not NAME RESULT BOOT ERROR", i + 1));
        }
        let boot = f[2]
            .parse()
            .map_err(|_| format!("line {}: the boot {} is not a number", i + 1, f[2]))?;
        cell.insert(
            f[0].to_string(),
            Module {
                result: f[1].to_string(),
                boot,
                error: f.get(3).unwrap_or(&"").to_string(),
            },
        );
    }
    Ok(cell)
}

/// One cell after its boots.
#[derive(Debug, Clone, Default)]
pub struct Run {
    /// The modules.
    pub cell: Cell,
    /// The result and seconds of each boot.
    pub boots: Vec<(String, f64)>,
    /// Whether the last boot passed, so every module has a result.
    pub complete: bool,
}

/// Boot `base` until a boot passes, at most `attempts` times, each in `out/boot-N`, and write
/// `out/modules.tsv`. `boot` runs one boot; it is [`crate::boot::run`] outside the tests.
pub fn run_cell(
    base: &Request,
    out: &Path,
    attempts: u32,
    boot: impl Fn(&Request) -> Result<Outcome, String>,
) -> Result<Run, String> {
    let mut run = Run::default();
    let mut skip = base.conf.modules_skip.clone();
    for n in 1..=attempts {
        let mut req = base.clone();
        req.out = out.join(format!("boot-{n}"));
        req.conf.suites = vec!["modules".into()];
        req.conf.modules_skip.clone_from(&skip);
        eprintln!("rb cross-modules: boot {n} in {}", req.out.display());
        let o = boot(&req)?;
        eprintln!(
            "rb cross-modules: boot {n}: {} in {:.0} s",
            o.result, o.seconds
        );
        run.boots.push((o.result.clone(), o.seconds));
        let tsv = req.out.join("results/modules/modules.tsv");
        let rows = guest_rows(&std::fs::read_to_string(tsv).unwrap_or_default());
        merge(&mut run.cell, n, &rows);
        if o.passed() {
            run.complete = true;
            break;
        }
        let Some(name) = stopped_in(&o, &rows) else {
            break;
        };
        run.cell.insert(
            name.clone(),
            Module {
                result: o.result.clone(),
                boot: n,
                error: o.seen.panic.clone().unwrap_or_default(),
            },
        );
        skip.push(name);
    }
    std::fs::create_dir_all(out).map_err(|e| format!("{}: {e}", out.display()))?;
    let file = out.join("modules.tsv");
    std::fs::write(&file, render(&run.cell)).map_err(|e| format!("{}: {e}", file.display()))?;
    Ok(run)
}

/// How many modules of a cell have each result.
fn tally(cell: &Cell) -> BTreeMap<&str, usize> {
    let mut t = BTreeMap::new();
    for m in cell.values() {
        *t.entry(m.result.as_str()).or_default() += 1;
    }
    t
}

/// One cell as markdown.
#[must_use]
pub fn run_report(run: &Run, out: &Path) -> String {
    let mut s = format!(
        "# rb cross-modules: one cell\n\n{} modules in {} boots. The last boot {}.\n\n",
        run.cell.len(),
        run.boots.len(),
        if run.complete {
            "passed"
        } else {
            "did not pass, so some modules have no result"
        }
    );
    s.push_str("| boot | result | seconds |\n|---|---|---|\n");
    for (i, (result, secs)) in run.boots.iter().enumerate() {
        let _ = writeln!(s, "| {} | {result} | {secs:.0} |", i + 1);
    }
    s.push_str("\n| result | modules |\n|---|---|\n");
    for (result, n) in tally(&run.cell) {
        let _ = writeln!(s, "| {result} | {n} |");
    }
    let bad: Vec<_> = run.cell.iter().filter(|(_, m)| !m.ok()).collect();
    if !bad.is_empty() {
        s.push_str("\n| module | result | boot | error |\n|---|---|---|---|\n");
        for (name, m) in bad {
            let _ = writeln!(s, "| {name} | {} | {} | {} |", m.result, m.boot, m.error);
        }
    }
    let _ = writeln!(
        s,
        "\nThe results are in {}.",
        out.join("modules.tsv").display()
    );
    s
}

/// A module that passes in GG and not in another cell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    /// The module.
    pub module: String,
    /// The cell.
    pub cell: &'static str,
    /// Its result there, or `absent` when the cell has no such module.
    pub result: String,
    /// The error.
    pub error: String,
}

/// The four cells compared.
#[derive(Debug, Clone, Default)]
pub struct Compare {
    /// The cells given, GG first.
    pub cells: Vec<(&'static str, Cell)>,
    /// The modules GG passes, which are graded.
    pub graded: usize,
    /// The modules GG has and does not pass, which are not graded.
    pub gg_failed: Vec<(String, String)>,
    /// The findings, by module and then cell.
    pub findings: Vec<Finding>,
}

/// Compare the cells with GG. `others` holds each other cell given.
#[must_use]
pub fn compare(gg: Cell, others: Vec<(&'static str, Cell)>) -> Compare {
    let mut c = Compare::default();
    for (name, m) in &gg {
        if !m.ok() {
            c.gg_failed.push((name.clone(), m.result.clone()));
            continue;
        }
        c.graded += 1;
        for (cell, modules) in &others {
            let (result, error) = modules
                .get(name)
                .map_or(("absent".to_string(), String::new()), |m| {
                    (m.result.clone(), m.error.clone())
                });
            if result != "pass" && result != "present" {
                c.findings.push(Finding {
                    module: name.clone(),
                    cell,
                    result,
                    error,
                });
            }
        }
    }
    c.cells.push(("GG", gg));
    c.cells.extend(others);
    c
}

impl Compare {
    /// The comparison as markdown, with at most `limit` findings listed.
    #[must_use]
    pub fn report(&self, limit: usize) -> String {
        let modules: BTreeSet<&String> = self.cells.iter().flat_map(|(_, c)| c.keys()).collect();
        let mut s = format!(
            "# rb cross-modules\n\nrb cross-modules: {} modules, {} graded as GG passes them. Findings: {}.\n\n",
            modules.len(),
            self.graded,
            self.findings.len()
        );
        s.push_str("The first letter of a cell names the kernel, and the second the modules.\n\n");
        s.push_str(
            "| cell | modules | pass | present | other | findings |\n|---|---|---|---|---|---|\n",
        );
        for (name, cell) in &self.cells {
            let t = tally(cell);
            let get = |r: &str| t.get(r).copied().unwrap_or(0);
            let found = self.findings.iter().filter(|f| f.cell == *name).count();
            let _ = writeln!(
                s,
                "| {name} | {} | {} | {} | {} | {found} |",
                cell.len(),
                get("pass"),
                get("present"),
                cell.len() - get("pass") - get("present"),
            );
        }
        if !self.findings.is_empty() {
            s.push_str("\n| module | cell | result | error |\n|---|---|---|---|\n");
            for f in self.findings.iter().take(limit) {
                let _ = writeln!(
                    s,
                    "| {} | {} | {} | {} |",
                    f.module, f.cell, f.result, f.error
                );
            }
            if self.findings.len() > limit {
                let _ = writeln!(s, "\nand {} more.", self.findings.len() - limit);
            }
        }
        if !self.gg_failed.is_empty() {
            let _ = write!(
                s,
                "\nGG does not pass {} modules, which are not graded:",
                self.gg_failed.len()
            );
            for (name, result) in self.gg_failed.iter().take(limit) {
                let _ = write!(s, " {name} ({result})");
            }
            s.push('\n');
        }
        s
    }
}

/// The command line of `rb cross-modules`.
pub mod cli {
    use super::{CELLS, compare, parse, run_cell, run_report};
    use crate::boot::{self, cli as boot_cli, kvm_usable};
    use crate::cli::Args;
    use rb_pins::places::Rows;
    use std::path::{Path, PathBuf};

    fn number(args: &Args, name: &str, default: u32) -> Result<u32, String> {
        args.get(name).map_or(Ok(default), |v| {
            v.parse()
                .map_err(|_| format!("--{name} {v} is not a number"))
        })
    }

    fn read(dir: &str) -> Result<super::Cell, String> {
        let file = Path::new(dir).join("modules.tsv");
        let text =
            std::fs::read_to_string(&file).map_err(|e| format!("{}: {e}", file.display()))?;
        parse(&text).map_err(|e| format!("{}: {e}", file.display()))
    }

    /// Run `rb cross-modules`, and return the report and whether it held: every module of the
    /// cell passed, or no cell has a finding. `rows` is needed to boot.
    pub fn run(args: &Args, rows: Option<&Rows>) -> Result<(String, bool), String> {
        if let Some(gg) = args.get("gg") {
            if args.get("image").is_some() {
                return Err("give --image to run a cell, or --gg to compare cells".into());
            }
            let mut others = Vec::new();
            for cell in CELLS {
                if let Some(dir) = args.get(&cell.to_lowercase()) {
                    others.push((cell, read(dir)?));
                }
            }
            if others.is_empty() {
                return Err("give --rg, --gr or --rr with --gg".into());
            }
            let c = compare(read(gg)?, others);
            let limit = number(args, "limit", 200)? as usize;
            return Ok((c.report(limit), c.findings.is_empty()));
        }
        let rows = rows.ok_or("booting needs the rows.toml of the repository")?;
        let req = boot_cli::request(args, rows, kvm_usable)?;
        let attempts = number(args, "attempts", 8)?;
        if attempts == 0 {
            return Err("--attempts must be 1 or more".into());
        }
        let out = PathBuf::from(&req.out);
        let run = run_cell(&req, &out, attempts, boot::run)?;
        let ok = run.complete && run.cell.values().all(super::Module::ok);
        Ok((run_report(&run, &out), ok))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cell(rows: &[(&str, &str)]) -> Cell {
        rows.iter()
            .map(|(n, r)| {
                (
                    (*n).to_string(),
                    Module {
                        result: (*r).to_string(),
                        boot: 1,
                        error: String::new(),
                    },
                )
            })
            .collect()
    }

    fn rows(text: &str) -> Vec<(String, String, String)> {
        guest_rows(text)
    }

    #[test]
    fn a_skip_keeps_the_earlier_result() {
        let mut c = Cell::new();
        merge(&mut c, 1, &rows("a\tpass\t\nb\tload\tmodload: b: no\n"));
        c.insert(
            "x".into(),
            Module {
                result: "panic".into(),
                boot: 1,
                error: "panic: trap".into(),
            },
        );
        merge(
            &mut c,
            2,
            &rows("a\tpass\t\nb\tpass\t\nx\tskip\t\ny\tunload\tbusy\n"),
        );
        assert_eq!(
            render(&c),
            "a\tpass\t2\t\nb\tpass\t2\t\nx\tpanic\t1\tpanic: trap\ny\tunload\t2\tbusy\n"
        );
        assert_eq!(parse(&render(&c)).unwrap(), c);
    }

    #[test]
    fn the_module_a_boot_stopped_in() {
        let mut o = Outcome {
            result: "panic".into(),
            ..Outcome::default()
        };
        o.seen.case = Some("modules/x".into());
        assert_eq!(stopped_in(&o, &rows("a\tpass\t\n")).as_deref(), Some("x"));
        // The suite wrote x's line, so the boot stopped after it.
        assert_eq!(stopped_in(&o, &rows("x\tpass\t\n")), None);
        o.seen.case = Some("lib/libc/t_x:y".into());
        assert_eq!(stopped_in(&o, &[]), None);
        o.seen.case = Some("modules/x".into());
        o.result = "pass".into();
        assert_eq!(stopped_in(&o, &[]), None);
    }

    #[test]
    fn a_cell_boots_again_past_a_panic() {
        let top = std::env::temp_dir().join(format!("rb-cross-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&top);
        let base = crate::boot::tests::request();
        let boot = |req: &Request| -> Result<Outcome, String> {
            // x panics. The suite saved a's line before it.
            let skip = &req.conf.modules_skip;
            let dir = req.out.join("results/modules");
            std::fs::create_dir_all(&dir).unwrap();
            let mut o = Outcome::default();
            if skip.iter().any(|s| s == "x") {
                std::fs::write(
                    dir.join("modules.tsv"),
                    "a\tpass\t\nx\tskip\t\nz\tload\tno\n",
                )
                .unwrap();
                o.result = "pass".into();
            } else {
                std::fs::write(dir.join("modules.tsv"), "a\tpass\t\n").unwrap();
                o.result = "panic".into();
                o.seen.case = Some("modules/x".into());
                o.seen.panic = Some("panic: kernel diagnostic assertion".into());
            }
            assert_eq!(req.conf.suites, ["modules"]);
            Ok(o)
        };
        let run = run_cell(&base, &top, 3, boot).unwrap();
        assert!(run.complete);
        assert_eq!(run.boots.len(), 2);
        let text = std::fs::read_to_string(top.join("modules.tsv")).unwrap();
        assert_eq!(
            text,
            "a\tpass\t2\t\nx\tpanic\t1\tpanic: kernel diagnostic assertion\nz\tload\t2\tno\n"
        );
        let report = run_report(&run, &top);
        assert!(report.contains("| x | panic | 1 |"), "{report}");
        // A boot that stops outside a module ends the cell.
        let stuck = |_: &Request| -> Result<Outcome, String> {
            Ok(Outcome {
                result: "boot-timeout".into(),
                ..Outcome::default()
            })
        };
        let run = run_cell(&base, &top.join("stuck"), 3, stuck).unwrap();
        assert!(!run.complete && run.boots.len() == 1 && run.cell.is_empty());
        std::fs::remove_dir_all(&top).unwrap();
    }

    #[test]
    fn findings_are_what_gg_passes_and_another_cell_does_not() {
        let gg = cell(&[
            ("a", "pass"),
            ("b", "pass"),
            ("c", "load"),
            ("k", "present"),
        ]);
        let rg = cell(&[
            ("a", "pass"),
            ("b", "panic"),
            ("c", "load"),
            ("k", "present"),
        ]);
        let gr = cell(&[("a", "pass"), ("c", "pass"), ("k", "present")]);
        let c = compare(gg, vec![("RG", rg), ("GR", gr)]);
        assert_eq!(c.graded, 3);
        assert_eq!(c.gg_failed, [("c".to_string(), "load".to_string())]);
        let found: Vec<_> = c
            .findings
            .iter()
            .map(|f| (f.module.as_str(), f.cell, f.result.as_str()))
            .collect();
        assert_eq!(found, [("b", "RG", "panic"), ("b", "GR", "absent")]);
        let report = c.report(10);
        assert!(
            report
                .contains("rb cross-modules: 4 modules, 3 graded as GG passes them. Findings: 2."),
            "{report}"
        );
        assert!(report.contains("| RG | 4 | 1 | 1 | 2 | 1 |"), "{report}");
        assert!(
            report.contains("GG does not pass 1 modules, which are not graded: c (load)"),
            "{report}"
        );
    }
}
