//! `rb frames`: the stack frame of each function in R against G (plan 08.7).
//!
//! NetBSD builds its kernel with `-fstack-usage -Wstack-usage=3584`, and both compilers then write
//! a `.su` file beside each object, with one line for each function: where it is, how many bytes
//! its frame takes, and whether the frame is `static`, `dynamic` or `dynamic,bounded`. The kernel
//! stack on amd64 is three pages, so a frame that grows matters.
//!
//! Objects are paired by path as for `rb sections-diff`, and the `.su` file of `X.o` is `X.su`.
//! Functions are paired by name in each pair of files. These are findings:
//! - R's frame is larger than G's by more than 25% and by more than 128 bytes;
//! - R's frame is over the limit of the unit's `-Wstack-usage=` and G's is not;
//! - R's frame is `dynamic` and G's is `static`;
//! - G wrote a `.su` file for the unit and R did not.
//!
//! A function in one file only is listed and not counted: the compilers inline and clone
//! different functions. The largest differences are listed too, for information.

use crate::objdiff::{Unit, unit};
use harness_shim::record::CompileRecord;
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::Path;

/// One function's frame, as a `.su` line gives it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    /// The bytes of stack the frame takes.
    pub bytes: u64,
    /// `static`, `dynamic` or `dynamic,bounded`.
    pub qualifiers: String,
}

impl Frame {
    /// Whether the frame grows while the function runs.
    #[must_use]
    pub fn dynamic(&self) -> bool {
        self.qualifiers.starts_with("dynamic")
    }
}

/// The frames of a `.su` file by function name. A line is `FILE:LINE:COLUMN:NAME`, a tab, the
/// bytes, a tab and the qualifiers. When a name comes twice, as for a clone, the larger frame
/// is kept.
#[must_use]
pub fn parse(text: &str) -> BTreeMap<String, Frame> {
    let mut out: BTreeMap<String, Frame> = BTreeMap::new();
    for line in text.lines() {
        let mut fields = line.split('\t');
        let (Some(at), Some(bytes), Some(qualifiers)) =
            (fields.next(), fields.next(), fields.next())
        else {
            continue;
        };
        let (Some(name), Ok(bytes)) = (at.splitn(4, ':').nth(3), bytes.trim().parse()) else {
            continue;
        };
        let frame = Frame {
            bytes,
            qualifiers: qualifiers.trim().to_string(),
        };
        match out.get(name) {
            Some(old) if old.bytes >= frame.bytes => {}
            _ => {
                out.insert(name.to_string(), frame);
            }
        }
    }
    out
}

/// The `.su` file of an object: `X.o` has `X.su`, and `X.o.o` has `X.o.su`.
#[must_use]
pub fn su_path(object: &str) -> String {
    Path::new(object)
        .with_extension("su")
        .to_string_lossy()
        .into_owned()
}

/// The limit of the last `-Wstack-usage=` of a command, if it has one.
#[must_use]
pub fn limit(argv: &[String]) -> Option<u64> {
    argv.iter()
        .rev()
        .find_map(|a| a.strip_prefix("-Wstack-usage=")?.parse().ok())
}

/// The objects of a build, keyed as in [`crate::objdiff::units`], each with the limit of its
/// `-Wstack-usage=`. When two calls made the same object, the last one counts.
#[must_use]
pub fn units(records: &[CompileRecord], root: &str) -> BTreeMap<String, (Unit, Option<u64>)> {
    records
        .iter()
        .filter_map(|r| unit(r, root).map(|(k, u)| (k, (u, limit(&r.argv)))))
        .collect()
}

/// A finding, or an allowed difference, about one function of one unit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    /// The unit's source.
    pub source: String,
    /// The function.
    pub function: String,
    /// G's frame.
    pub g: Option<Frame>,
    /// R's frame.
    pub r: Option<Frame>,
}

impl Line {
    fn show(&self) -> String {
        let side = |f: &Option<Frame>| {
            f.as_ref().map_or_else(
                || "none".to_string(),
                |f| format!("{} {}", f.bytes, f.qualifiers),
            )
        };
        format!(
            "{}: {}: G {}, R {}",
            self.source,
            self.function,
            side(&self.g),
            side(&self.r)
        )
    }
}

/// The comparison of two builds' frames.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Frames {
    /// Units both builds made.
    pub paired: usize,
    /// Of those, the units with a `.su` file on both sides.
    pub both: usize,
    /// Units with a `.su` file on neither side.
    pub neither: usize,
    /// Functions in both files of a pair.
    pub functions: usize,
    /// The sum of the frames of those functions, in G and in R.
    pub bytes: (u64, u64),
    /// R's frame is larger by more than 25% and 128 bytes.
    pub larger: Vec<Line>,
    /// R's frame is over the limit and G's is not.
    pub over: Vec<Line>,
    /// R's frame is dynamic and G's is static.
    pub dynamic: Vec<Line>,
    /// The units where G wrote a `.su` file and R did not.
    pub no_su: Vec<String>,
    /// Functions in one file only.
    pub one_side: Vec<Line>,
    /// Every paired function whose frame differs, R's bytes minus G's.
    pub differences: Vec<(i64, Line)>,
}

impl Frames {
    /// What counts.
    #[must_use]
    pub fn counted(&self) -> usize {
        self.larger.len() + self.over.len() + self.dynamic.len() + self.no_su.len()
    }

    /// Put in the frames of one unit.
    pub fn add(
        &mut self,
        source: &str,
        g: &BTreeMap<String, Frame>,
        r: &BTreeMap<String, Frame>,
        limit: Option<u64>,
    ) {
        let line = |name: &str| Line {
            source: source.to_string(),
            function: name.to_string(),
            g: g.get(name).cloned(),
            r: r.get(name).cloned(),
        };
        for (name, theirs) in g {
            let Some(ours) = r.get(name) else {
                self.one_side.push(line(name));
                continue;
            };
            self.functions += 1;
            self.bytes.0 += theirs.bytes;
            self.bytes.1 += ours.bytes;
            if ours.bytes > theirs.bytes + 128 && ours.bytes * 4 > theirs.bytes * 5 {
                self.larger.push(line(name));
            }
            if limit.is_some_and(|l| ours.bytes > l && theirs.bytes <= l) {
                self.over.push(line(name));
            }
            if ours.dynamic() && !theirs.dynamic() {
                self.dynamic.push(line(name));
            }
            if ours.bytes != theirs.bytes {
                let d = i64::try_from(ours.bytes).unwrap_or(i64::MAX)
                    - i64::try_from(theirs.bytes).unwrap_or(i64::MAX);
                self.differences.push((d, line(name)));
            }
        }
        for name in r.keys().filter(|n| !g.contains_key(*n)) {
            self.one_side.push(line(name));
        }
    }

    /// The report. Each list stops after `limit` lines, and the largest differences after `top`.
    #[must_use]
    pub fn report(&self, limit: usize, top: usize) -> String {
        let mut s = String::new();
        let _ = writeln!(
            s,
            "rb frames: units paired: {}, with a .su file on both sides: {}, on neither: {}. Functions paired: {}, frames G {} bytes, R {} bytes. Findings: {}. Functions in one file only, not counted: {}.",
            self.paired,
            self.both,
            self.neither,
            self.functions,
            self.bytes.0,
            self.bytes.1,
            self.counted(),
            self.one_side.len(),
        );
        let list = |s: &mut String, title: &str, lines: Vec<String>, limit: usize| {
            if lines.is_empty() {
                return;
            }
            let _ = writeln!(s, "\n{title} ({}):", lines.len());
            for l in lines.iter().take(limit) {
                let _ = writeln!(s, "  {l}");
            }
            if lines.len() > limit {
                let _ = writeln!(s, "  and {} more", lines.len() - limit);
            }
        };
        let shown = |v: &[Line]| v.iter().map(Line::show).collect::<Vec<_>>();
        list(
            &mut s,
            "R's frame is larger than G's by more than 25% and 128 bytes",
            shown(&self.larger),
            limit,
        );
        list(
            &mut s,
            "R's frame is over the -Wstack-usage limit and G's is not",
            shown(&self.over),
            limit,
        );
        list(
            &mut s,
            "R's frame is dynamic and G's is static",
            shown(&self.dynamic),
            limit,
        );
        list(
            &mut s,
            "G wrote a .su file and R did not",
            self.no_su.clone(),
            limit,
        );
        let mut differences: Vec<&(i64, Line)> = self.differences.iter().collect();
        differences.sort_by(|a, b| {
            b.0.abs()
                .cmp(&a.0.abs())
                .then_with(|| a.1.source.cmp(&b.1.source))
                .then_with(|| a.1.function.cmp(&b.1.function))
        });
        list(
            &mut s,
            "The largest differences, R minus G, not counted",
            differences
                .iter()
                .map(|(d, l)| format!("{d:+} {}", l.show()))
                .collect(),
            top,
        );
        list(
            &mut s,
            "Functions in one file only, not counted",
            shown(&self.one_side),
            limit,
        );
        s
    }
}

/// Read a `.su` file, or `None` when there is none.
fn read_su(object: &str) -> Result<Option<BTreeMap<String, Frame>>, String> {
    let path = su_path(object);
    match std::fs::read_to_string(&path) {
        Ok(text) => Ok(Some(parse(&text))),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("{path}: {e}")),
    }
}

/// Compare the frames of the units both builds made.
pub fn compare(
    g: &BTreeMap<String, (Unit, Option<u64>)>,
    r: &BTreeMap<String, (Unit, Option<u64>)>,
) -> Result<Frames, String> {
    let mut out = Frames::default();
    for (key, (ours, limit)) in r {
        let Some((theirs, _)) = g.get(key) else {
            continue;
        };
        out.paired += 1;
        let source = if ours.source.is_empty() {
            key.as_str()
        } else {
            ours.source.as_str()
        };
        match (read_su(&theirs.path)?, read_su(&ours.path)?) {
            (Some(a), Some(b)) => {
                out.both += 1;
                out.add(source, &a, &b, *limit);
            }
            (Some(_), None) => out.no_su.push(source.to_string()),
            (None, None) => out.neither += 1,
            (None, Some(_)) => {}
        }
    }
    Ok(out)
}

/// `rb frames` on the command line.
pub mod cli {
    use super::{compare, units};
    use crate::cli::Args;
    use harness_shim::record::read_log;
    use std::path::Path;

    /// Run `rb frames`. Returns the report and whether functions were paired and nothing
    /// counted.
    pub fn run(args: &Args) -> Result<(String, bool), String> {
        let (Some(g), Some(r)) = (args.get("g"), args.get("r")) else {
            return Err("give --g and --r, the compile.jsonl of each build".into());
        };
        let number = |key: &str, default: usize| -> Result<usize, String> {
            args.get(key).map_or(Ok(default), |n| {
                n.parse()
                    .map_err(|_| format!("--{key} {n} is not a number"))
            })
        };
        let read = |log: &str, root: &str| {
            let (records, _) = read_log(Path::new(log)).map_err(|e| format!("{log}: {e}"))?;
            Ok::<_, String>(units(&records, root))
        };
        let found = compare(
            &read(g, args.get("g-root").unwrap_or_default())?,
            &read(r, args.get("r-root").unwrap_or_default())?,
        )?;
        Ok((
            found.report(number("limit", 20)?, number("top", 100)?),
            found.functions > 0 && found.counted() == 0,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frames(text: &str) -> BTreeMap<String, Frame> {
        parse(text)
    }

    #[test]
    fn su_lines_are_read_by_function_name() {
        let f = frames(
            "/s/a.c:10:1:f\t48\tstatic\n/s/a.c:20:5:g\t64\tdynamic,bounded\n/s/a.c:30:1:f\t96\tstatic\nnot a line\n",
        );
        assert_eq!(f.len(), 2);
        assert_eq!(f["f"].bytes, 96);
        assert!(f["g"].dynamic());
        let cxx = frames("/s/a.cc:3:5:int ns::f(int)\t16\tstatic\n");
        assert!(cxx.contains_key("int ns::f(int)"));
    }

    #[test]
    fn the_su_file_is_named_after_the_object() {
        assert_eq!(su_path("/o/a.o"), "/o/a.su");
        assert_eq!(su_path("/o/lib/kern/memcpy.o.o"), "/o/lib/kern/memcpy.o.su");
        let argv: Vec<String> = ["cc", "-Wstack-usage=1024", "-Wstack-usage=3584", "-c"]
            .map(String::from)
            .to_vec();
        assert_eq!(limit(&argv), Some(3584));
        assert_eq!(limit(&argv[..1]), None);
    }

    #[test]
    fn larger_over_the_limit_and_dynamic_frames_are_findings() {
        let g = frames(
            "a.c:1:1:same\t100\tstatic\na.c:2:1:big\t400\tstatic\na.c:3:1:small\t100\tstatic\na.c:4:1:over\t3500\tstatic\na.c:5:1:vla\t32\tstatic\na.c:6:1:inlined\t16\tstatic\n",
        );
        let r = frames(
            "a.c:1:1:same\t100\tstatic\na.c:2:1:big\t560\tstatic\na.c:3:1:small\t220\tstatic\na.c:4:1:over\t3600\tstatic\na.c:5:1:vla\t32\tdynamic\n",
        );
        let mut found = Frames::default();
        found.add("/s/a.c", &g, &r, Some(3584));
        let names = |v: &[Line]| v.iter().map(|l| l.function.clone()).collect::<Vec<_>>();
        // big grew 160 bytes and 40%; small grew 120%, but only 120 bytes; over grew 100 bytes.
        assert_eq!(names(&found.larger), ["big"]);
        assert_eq!(names(&found.over), ["over"]);
        assert_eq!(names(&found.dynamic), ["vla"]);
        assert_eq!(names(&found.one_side), ["inlined"]);
        assert_eq!(found.counted(), 3);
        assert_eq!(found.functions, 5);
        let text = found.report(20, 2);
        assert!(
            text.contains("  /s/a.c: big: G 400 static, R 560 static\n"),
            "{text}"
        );
        assert!(text.contains("  +160 /s/a.c: big: "), "{text}");
        assert!(text.contains("  +120 /s/a.c: small: "), "{text}");
        assert!(!text.contains("  +100 /s/a.c: over: "), "{text}");

        let mut same = Frames::default();
        same.add("/s/a.c", &g, &g, Some(3584));
        assert_eq!(same.counted(), 0);
    }
}
