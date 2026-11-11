//! `rb warnings`: the warnings rucc gives that the reference does not (plan 06.9).
//!
//! Both systems build almost everything with `-Werror`, so a warning rucc gives and the reference
//! does not stops the build. The rule is that, for each unit, rucc's warnings are a subset of the
//! reference's. Warnings are compared by option name (`[-Wfoo]`) and by line. A warning only the
//! reference gives is listed but not counted: rucc may be quieter, not noisier.
//!
//! rucc puts its own code in the brackets, as in `[E0770]`, where GCC puts the option name, and
//! its words are not always GCC's. So a diagnostic with a code of rucc is the same as a diagnostic
//! of the reference on the same line of the same file.
//!
//! The diagnostics come from the standard error the shim kept on each record. The shim keeps the
//! first KiB of it, so a unit with more is compared on that part, and the report says how many
//! units that was.

use crate::census::{absolute, is_object};
use harness_shim::record::CompileRecord;
use harness_shim::run::STDERR_KEEP;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::Path;

/// One diagnostic, as a compiler printed it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Diagnostic {
    /// The file name, without its directory, or `(command line)` when the line names no file.
    pub file: String,
    /// The line, or 0 when there is none.
    pub line: u32,
    /// The option, as in `-Wformat`, with `-Werror=` read as `-W`. Empty when none is named.
    pub option: String,
    /// rucc's code, as in `E0770`, which rucc prints where GCC prints the option. Empty when
    /// there is none.
    pub code: String,
    /// `warning` or `error`.
    pub severity: String,
    /// The message, without the option.
    pub message: String,
}

impl Diagnostic {
    /// What two diagnostics must share to be the same: the file, the line and the option, or the
    /// message when no option is named.
    #[must_use]
    pub fn key(&self) -> (String, u32, String) {
        let what = if self.option.is_empty() {
            self.message.clone()
        } else {
            self.option.clone()
        };
        (self.file.clone(), self.line, what)
    }

    /// Whether this is the same diagnostic as one of `other`: the same key, or, when either one
    /// has a code of rucc, the same file and line.
    #[must_use]
    pub fn in_(&self, other: &[Diagnostic]) -> bool {
        let key = self.key();
        other.iter().any(|o| {
            o.key() == key
                || ((!self.code.is_empty() || !o.code.is_empty())
                    && o.file == self.file
                    && o.line == self.line)
        })
    }

    /// The diagnostic as one line of a report.
    #[must_use]
    pub fn line(&self) -> String {
        let place = if self.line == 0 {
            self.file.clone()
        } else {
            format!("{}:{}", self.file, self.line)
        };
        let bracket = if self.option.is_empty() {
            &self.code
        } else {
            &self.option
        };
        if bracket.is_empty() {
            format!("{place}: {}: {}", self.severity, self.message)
        } else {
            format!("{place}: {}: {} [{bracket}]", self.severity, self.message)
        }
    }
}

/// The message without the code of rucc at its end, and the code: `E0770` in
/// `'f' is deprecated [E0770]`. A code is `E` or `W` and four digits.
fn rucc_code(message: &str) -> Option<(&str, &str)> {
    let (head, code) = message.strip_suffix(']')?.rsplit_once(" [")?;
    let digits = code.strip_prefix('E').or_else(|| code.strip_prefix('W'))?;
    (digits.len() == 4 && digits.bytes().all(|b| b.is_ascii_digit()))
        .then(|| (head.trim_end(), code))
}

/// Read the warnings and errors of a compiler's standard error. Notes and the `In file included
/// from` lines are left out.
#[must_use]
pub fn parse(stderr: &str) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    for line in stderr.lines() {
        let Some((severity, at)) = ["warning", "error", "fatal error"]
            .iter()
            .filter_map(|s| line.find(&format!(": {s}: ")).map(|at| (*s, at)))
            .min_by_key(|(_, at)| *at)
        else {
            continue;
        };
        let place = &line[..at];
        let rest = &line[at + severity.len() + 4..];
        let (message, option) = match rest.rfind(" [-W") {
            Some(i) if rest.ends_with(']') => (
                rest[..i].trim(),
                rest[i + 2..rest.len() - 1]
                    .split(',')
                    .next()
                    .unwrap_or_default()
                    .replacen("-Werror=", "-W", 1),
            ),
            _ => (rest.trim(), String::new()),
        };
        let (message, code) = match rucc_code(message) {
            Some((message, code)) => (message, code.to_string()),
            None => (message, String::new()),
        };
        let mut parts = place.split(':');
        let file = parts.next().unwrap_or_default();
        let line_no = parts.next().and_then(|n| n.trim().parse().ok());
        let file = match line_no {
            Some(_) => Path::new(file)
                .file_name()
                .map_or_else(|| file.to_string(), |n| n.to_string_lossy().into_owned()),
            None => "(command line)".to_string(),
        };
        out.push(Diagnostic {
            file,
            line: line_no.unwrap_or(0),
            option,
            code,
            severity: if severity == "fatal error" {
                "error".to_string()
            } else {
                severity.to_string()
            },
            message: message.to_string(),
        });
    }
    out
}

/// The standard error of each unit of a log, keyed by its object with `root` replaced by
/// `<root>`. The last call that wrote an object counts.
#[must_use]
pub fn units(records: &[CompileRecord], root: &str) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for r in records {
        let slot = r.slot.as_deref().unwrap_or("cc");
        if !r.is_unit() || slot != "cc" {
            continue;
        }
        let Some(output) = r
            .outputs
            .iter()
            .map(|o| o.path.as_str())
            .find(|p| is_object(p))
        else {
            continue;
        };
        let mut key = absolute(&r.cwd, output);
        if !root.is_empty() {
            key = key.replace(root, "<root>");
        }
        out.insert(key, r.stderr.clone());
    }
    out
}

/// What one unit gave on each side.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UnitDiff {
    /// Diagnostics only rucc gave, which count.
    pub only_r: Vec<Diagnostic>,
    /// Diagnostics only the reference gave, which do not.
    pub only_g: Vec<Diagnostic>,
}

/// The comparison of two sets of units.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Warnings {
    /// Units on both sides.
    pub paired: usize,
    /// Units whose diagnostics were the same.
    pub same: usize,
    /// Units whose standard error filled what the shim keeps, so that some may be lost.
    pub cut: usize,
    /// The units that differ, by unit.
    pub units: BTreeMap<String, UnitDiff>,
}

/// Compare one unit's diagnostics: `r` must be a subset of `g`.
#[must_use]
pub fn compare_unit(g: &[Diagnostic], r: &[Diagnostic]) -> UnitDiff {
    let mut seen = BTreeSet::new();
    let only_r = r
        .iter()
        .filter(|d| !d.in_(g) && seen.insert(d.key()))
        .cloned()
        .collect();
    let only_g = g
        .iter()
        .filter(|d| !d.in_(r) && seen.insert(d.key()))
        .cloned()
        .collect();
    UnitDiff { only_r, only_g }
}

/// Compare the standard error of the units both sides built.
#[must_use]
pub fn compare(g: &BTreeMap<String, String>, r: &BTreeMap<String, String>) -> Warnings {
    let mut out = Warnings::default();
    for (unit, g_err) in g {
        let Some(r_err) = r.get(unit) else { continue };
        out.paired += 1;
        if g_err.len() >= STDERR_KEEP || r_err.len() >= STDERR_KEEP {
            out.cut += 1;
        }
        let diff = compare_unit(&parse(g_err), &parse(r_err));
        if diff.only_r.is_empty() && diff.only_g.is_empty() {
            out.same += 1;
        } else {
            out.units.insert(unit.clone(), diff);
        }
    }
    out
}

impl Warnings {
    /// The diagnostics that count: those only rucc gave.
    #[must_use]
    pub fn counted(&self) -> usize {
        self.units.values().map(|u| u.only_r.len()).sum()
    }

    /// The report. At most `limit` units are listed in each part.
    #[must_use]
    pub fn report(&self, limit: usize) -> String {
        let mut s = String::new();
        let only_g: usize = self.units.values().map(|u| u.only_g.len()).sum();
        let _ = writeln!(
            s,
            "rb warnings: {} paired units, {} the same. Diagnostics only rucc gave, which count: {}. Diagnostics only the reference gave, which do not: {only_g}.",
            self.paired,
            self.same,
            self.counted()
        );
        if self.cut > 0 {
            let _ = writeln!(
                s,
                "Units whose standard error filled the {STDERR_KEEP} bytes the shim keeps, and so may have more: {}.",
                self.cut
            );
        }
        part(
            &mut s,
            "Only rucc",
            limit,
            self.units
                .iter()
                .filter(|(_, u)| !u.only_r.is_empty())
                .map(|(n, u)| (n, &u.only_r)),
        );
        part(
            &mut s,
            "Only the reference (not counted)",
            limit,
            self.units
                .iter()
                .filter(|(_, u)| !u.only_g.is_empty())
                .map(|(n, u)| (n, &u.only_g)),
        );
        s
    }
}

fn part<'a>(
    s: &mut String,
    title: &str,
    limit: usize,
    units: impl Iterator<Item = (&'a String, &'a Vec<Diagnostic>)>,
) {
    let units: Vec<_> = units.collect();
    if units.is_empty() {
        return;
    }
    let _ = writeln!(s, "\n{title}: {} units", units.len());
    for (unit, diagnostics) in units.iter().take(limit) {
        let _ = writeln!(s, "  {unit}");
        for d in *diagnostics {
            let _ = writeln!(s, "    {}", d.line());
        }
    }
    if units.len() > limit {
        let _ = writeln!(s, "  and {} more", units.len() - limit);
    }
}

/// `rb warnings` on the command line.
pub mod cli {
    use super::{compare, units};
    use crate::cli::Args;
    use harness_shim::record::read_log;
    use std::path::Path;

    /// Run `rb warnings`. Returns the report and whether nothing counted.
    pub fn run(args: &Args) -> Result<(String, bool), String> {
        let (Some(g), Some(r)) = (args.get("g"), args.get("r")) else {
            return Err("give --g and --r, the compile.jsonl of each build".into());
        };
        let read = |log: &str, root: &str| {
            let (records, _) = read_log(Path::new(log)).map_err(|e| format!("{log}: {e}"))?;
            Ok::<_, String>(units(&records, root))
        };
        let g_units = read(g, args.get("g-root").unwrap_or_default())?;
        let r_units = read(r, args.get("r-root").unwrap_or_default())?;
        let limit = args
            .get("limit")
            .map(str::parse)
            .transpose()
            .map_err(|e| format!("--limit: {e}"))?
            .unwrap_or(20);
        let found = compare(&g_units, &r_units);
        let text = format!(
            "G: {g}: {} units\nR: {r}: {} units\n{}",
            g_units.len(),
            r_units.len(),
            found.report(limit)
        );
        Ok((text, found.counted() == 0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use harness_shim::record::parse_log;

    #[test]
    fn diagnostics_are_read_with_their_option_and_line() {
        let text = "\
In file included from ../inc/x.h:3,
/src/lib/a.c:12:5: warning: unused variable 'x' [-Wunused-variable]
/src/lib/a.c:20:1: error: control reaches end of non-void function [-Werror=return-type]
/src/lib/a.c:20:1: note: something
cc1: warning: command-line option '-Wfoo' is valid for C++ but not for C
a.c:7: error: expected ';'
";
        let d = parse(text);
        assert_eq!(d.len(), 4);
        assert_eq!(d[0].key(), ("a.c".into(), 12, "-Wunused-variable".into()));
        assert_eq!(d[1].option, "-Wreturn-type");
        assert_eq!(d[1].severity, "error");
        assert_eq!(d[2].file, "(command line)");
        assert_eq!(d[3].key(), ("a.c".into(), 7, "expected ';'".into()));
    }

    #[test]
    fn only_what_rucc_adds_counts() {
        let g = parse("a.c:1:1: warning: w [-Wa]\na.c:2:1: warning: v [-Wmaybe-uninitialized]\n");
        let r = parse("x/a.c:1:9: error: w2 [-Werror=a]\na.c:3:1: warning: z [-Wz]\n");
        let d = compare_unit(&g, &r);
        assert_eq!(d.only_r.len(), 1);
        assert_eq!(d.only_r[0].option, "-Wz");
        assert_eq!(d.only_g[0].option, "-Wmaybe-uninitialized");
    }

    #[test]
    fn a_code_of_rucc_is_the_same_as_the_reference_on_the_same_line() {
        let g = parse(
            "/src/a.c:123:9: warning: 'ntohll' is deprecated [-Wdeprecated-declarations]\n\
             /src/a.c:83:9: warning: 'f' redefined\n\
             /src/a.c:9:1: warning: passing argument 1 of 'g' discards 'const' qualifier [-Wdiscarded-qualifiers]\n",
        );
        let r = parse(
            "a.c:123: warning: 'ntohll' is deprecated [E0770]\n\
             a.c:83: warning: `f` redefined [W0301]\n\
             a.c:9: warning: passing argument 1 discards 'const' qualifier [E0514]\n\
             a.c:40: warning: cast to pointer from integer of different size [E0568]\n",
        );
        assert_eq!(r[0].code, "E0770");
        assert_eq!(r[0].message, "'ntohll' is deprecated");
        assert_eq!(
            r[0].line(),
            "a.c:123: warning: 'ntohll' is deprecated [E0770]"
        );
        let d = compare_unit(&g, &r);
        assert_eq!(d.only_r.len(), 1, "{:?}", d.only_r);
        assert_eq!(d.only_r[0].line, 40);
        assert!(d.only_g.is_empty(), "{:?}", d.only_g);
        // A word in brackets that is not a code stays in the message.
        assert_eq!(parse("a.c:1: warning: w [x]\n")[0].message, "w [x]");
    }

    #[test]
    fn units_are_paired_by_object() {
        let rec = |side: &str, err: &str| {
            format!(
                r#"{{"started":1,"argv":["cc","-c","a.c","-o","a.o"],"compiler":"/cc","cwd":"/{side}/lib","outputs":[{{"path":"a.o","sha256":"y"}}],"wall-seconds":0.1,"exit":0,"stderr":"{err}","slot":"cc"}}"#
            )
        };
        let g = units(
            &parse_log(&rec("g", "a.c:1:1: warning: w [-Wa]\\n")).0,
            "/g",
        );
        let r = units(
            &parse_log(&rec(
                "r",
                "a.c:1:1: warning: w [-Wa]\\na.c:4:1: warning: q [-Wq]\\n",
            ))
            .0,
            "/r",
        );
        let w = compare(&g, &r);
        assert_eq!((w.paired, w.same, w.counted()), (1, 0, 1));
        let report = w.report(5);
        assert!(report.contains("<root>/lib/a.o"), "{report}");
        assert!(report.contains("a.c:4: warning: q [-Wq]"), "{report}");
    }
}
