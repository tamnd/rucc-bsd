//! `rb flags-diff`: every unit's command line in R's build against the same unit's in G's (plan
//! 05.6 and 02.2).
//!
//! Units are paired by their output. Three things are taken out of each command line first: the
//! compiler's path, the persona flag (`-fgnuc-version=`) and the target flag rucc's shim adds
//! (`--target=` or `-target`). A build's own directory is replaced by `<root>` when `--g-root` and
//! `--r-root` name it. What still differs means that a make decision went another way, such as a
//! per-directory `CFLAGS.gcc` or a `.if ${COMPILER_VERSION}` in one `Makefile`.

use crate::census::{absolute, is_object};
use crate::divergences::Divergences;
use harness_shim::record::CompileRecord;
use std::collections::BTreeMap;
use std::fmt::Write as _;

/// One unit: its output, as the key, and its command line without the compiler's path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unit {
    /// The output, absolute, with the root replaced.
    pub output: String,
    /// The arguments after the compiler, normalized.
    pub args: Vec<String>,
}

/// Whether an argument is one of the three normalized away. `-target` also drops the next word.
fn dropped(arg: &str) -> bool {
    arg.starts_with("-fgnuc-version=") || arg.starts_with("--target=")
}

/// The normalized arguments of a command line.
#[must_use]
pub fn normalize(argv: &[String], root: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut i = 1;
    while i < argv.len() {
        let a = argv[i].as_str();
        if a == "-target" {
            i += 2;
            continue;
        }
        if !dropped(a) {
            let mut word = a
                .replace("rb-cc-rucc", "rb-cc")
                .replace("rb-cc-ref", "rb-cc");
            if !root.is_empty() {
                word = word.replace(root, "<root>");
            }
            out.push(word);
        }
        i += 1;
    }
    out
}

/// The units of a build: the C and C++ calls with `-c` that made an object. When two calls made
/// the same object, the last one counts.
#[must_use]
pub fn units(records: &[CompileRecord], root: &str) -> BTreeMap<String, Vec<String>> {
    let mut out = BTreeMap::new();
    for r in records {
        let slot = r.slot.as_deref().unwrap_or("cc");
        if !r.is_unit() || !matches!(slot, "cc" | "c++") {
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
        out.insert(key, normalize(&r.argv, root));
    }
    out
}

/// What one side has that the other does not, as a multiset difference that keeps the order.
fn minus(a: &[String], b: &[String]) -> Vec<String> {
    let mut left: Vec<&String> = b.iter().collect();
    let mut out = Vec::new();
    for x in a {
        if let Some(at) = left.iter().position(|y| *y == x) {
            left.remove(at);
        } else {
            out.push(x.clone());
        }
    }
    out
}

/// Units that differ in the same way.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Group {
    /// Flags only G's command line has.
    pub only_g: Vec<String>,
    /// Flags only R's has.
    pub only_r: Vec<String>,
    /// The units, by output.
    pub units: Vec<String>,
}

/// The comparison of two builds.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FlagsDiff {
    /// Units both builds made.
    pub paired: usize,
    /// Units whose command lines are the same after normalizing, or differ in allowed flags only.
    pub same: usize,
    /// Units whose flags are the same in another order.
    pub reordered: Vec<String>,
    /// The groups of units whose flags differ.
    pub groups: Vec<Group>,
    /// Flags left out of the comparison by an entry of the divergence file, with the reason and
    /// the number of units.
    pub allowed: BTreeMap<String, (String, usize)>,
    /// Units only G made.
    pub only_g: Vec<String>,
    /// Units only R made.
    pub only_r: Vec<String>,
}

/// Compare two builds' units.
#[must_use]
pub fn compare(
    g: &BTreeMap<String, Vec<String>>,
    r: &BTreeMap<String, Vec<String>>,
    system: &str,
    divergences: &Divergences,
) -> FlagsDiff {
    let mut d = FlagsDiff::default();
    let mut groups: BTreeMap<(Vec<String>, Vec<String>), Vec<String>> = BTreeMap::new();
    for (output, ga) in g {
        let Some(ra) = r.get(output) else {
            d.only_g.push(output.clone());
            continue;
        };
        d.paired += 1;
        if ga == ra {
            d.same += 1;
            continue;
        }
        let mut keep = |flags: Vec<String>| -> Vec<String> {
            flags
                .into_iter()
                .filter(|f| match divergences.flag(system, f) {
                    Some(reason) => {
                        d.allowed
                            .entry(f.clone())
                            .or_insert_with(|| (reason.to_string(), 0))
                            .1 += 1;
                        false
                    }
                    None => true,
                })
                .collect()
        };
        let only_g = keep(minus(ga, ra));
        let only_r = keep(minus(ra, ga));
        if !only_g.is_empty() || !only_r.is_empty() {
            groups
                .entry((only_g, only_r))
                .or_default()
                .push(output.clone());
        } else if minus(ga, ra).is_empty() && minus(ra, ga).is_empty() {
            d.reordered.push(output.clone());
        } else {
            d.same += 1;
        }
    }
    d.only_r = r.keys().filter(|k| !g.contains_key(*k)).cloned().collect();
    let mut groups: Vec<Group> = groups
        .into_iter()
        .map(|((only_g, only_r), units)| Group {
            only_g,
            only_r,
            units,
        })
        .collect();
    groups.sort_by_key(|g| std::cmp::Reverse(g.units.len()));
    d.groups = groups;
    d
}

impl FlagsDiff {
    /// How many differences count: each unit in a group, each reordered unit, and, unless
    /// `paired_only`, each unit only one build made.
    #[must_use]
    pub fn counted(&self, paired_only: bool) -> usize {
        let mut n = self.groups.iter().map(|g| g.units.len()).sum::<usize>() + self.reordered.len();
        if !paired_only {
            n += self.only_g.len() + self.only_r.len();
        }
        n
    }

    /// The report. Each list stops after `limit` lines.
    #[must_use]
    pub fn report(&self, paired_only: bool, limit: usize) -> String {
        let differ: usize = self.groups.iter().map(|g| g.units.len()).sum();
        let mut s = format!(
            "{} units in both builds: {} the same, {differ} with other flags in {} groups, {} with the same flags in another order\n{} units only in G, {} only in R{}\n",
            self.paired,
            self.same,
            self.groups.len(),
            self.reordered.len(),
            self.only_g.len(),
            self.only_r.len(),
            if paired_only { " (not counted)" } else { "" }
        );
        for g in self.groups.iter().take(limit) {
            let _ = writeln!(
                s,
                "\n{} units: G has {}; R has {}",
                g.units.len(),
                words(&g.only_g),
                words(&g.only_r)
            );
            for u in g.units.iter().take(3) {
                let _ = writeln!(s, "  {u}");
            }
            if g.units.len() > 3 {
                let _ = writeln!(s, "  and {} more", g.units.len() - 3);
            }
        }
        if self.groups.len() > limit {
            let _ = writeln!(s, "\nand {} more groups", self.groups.len() - limit);
        }
        list(
            &mut s,
            "same flags in another order",
            &self.reordered,
            limit,
        );
        list(&mut s, "only in G", &self.only_g, limit);
        list(&mut s, "only in R", &self.only_r, limit);
        if !self.allowed.is_empty() {
            s.push_str("\nallowed by mk-divergences.toml:\n");
            for (flag, (reason, n)) in &self.allowed {
                let _ = writeln!(s, "  {flag} in {n} units: {reason}");
            }
        }
        s
    }
}

fn words(flags: &[String]) -> String {
    if flags.is_empty() {
        "nothing more".to_string()
    } else {
        flags.join(" ")
    }
}

fn list(s: &mut String, title: &str, items: &[String], limit: usize) {
    if items.is_empty() {
        return;
    }
    let _ = writeln!(s, "\n{title}:");
    for i in items.iter().take(limit) {
        let _ = writeln!(s, "  {i}");
    }
    if items.len() > limit {
        let _ = writeln!(s, "  and {} more", items.len() - limit);
    }
}

/// The command line half.
pub mod cli {
    use super::{compare, units};
    use crate::cli::Args;
    use crate::divergences::Divergences;
    use crate::toolchain::cli::Files;
    use harness_shim::record::read_log;
    use std::path::Path;

    /// Run `rb flags-diff`. Returns the report and whether no difference counted.
    pub fn run(args: &Args, root: &Path) -> Result<(String, bool), String> {
        let files = Files::load(root, args.target.as_deref(), args.get("row"))?;
        let (Some(g), Some(r)) = (args.get("g"), args.get("r")) else {
            return Err("give --g and --r, the compile.jsonl of each build".into());
        };
        let read = |log: &str, root: &str| {
            let (records, bad) = read_log(Path::new(log)).map_err(|e| format!("{log}: {e}"))?;
            Ok::<_, String>((units(&records, root), records.len(), bad))
        };
        let (g_units, g_records, g_bad) = read(g, args.get("g-root").unwrap_or_default())?;
        let (r_units, r_records, r_bad) = read(r, args.get("r-root").unwrap_or_default())?;
        let divergences = Divergences::load(root, args.get("divergences").map(Path::new))?;
        let diff = compare(&g_units, &r_units, &files.pin.system, &divergences);
        let limit = args
            .get("limit")
            .map(str::parse)
            .transpose()
            .map_err(|e| format!("--limit: {e}"))?
            .unwrap_or(20);
        let paired_only = args.has("paired-only");
        let text = format!(
            "G: {g}: {g_records} records, {} units, {g_bad} unreadable lines\nR: {r}: {r_records} records, {} units, {r_bad} unreadable lines\n{}",
            g_units.len(),
            r_units.len(),
            diff.report(paired_only, limit)
        );
        Ok((text, diff.counted(paired_only) == 0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use harness_shim::record::parse_log;

    fn argv(line: &str) -> Vec<String> {
        line.split_whitespace().map(String::from).collect()
    }

    #[test]
    fn the_compiler_persona_and_target_go() {
        let g = argv("/t/bin/x-rb-cc-ref -O2 -c a.c -o /g/obj/a.o");
        let r = argv(
            "/u/bin/x-rb-cc-rucc --target=x86_64-unknown-netbsd -fgnuc-version=12.5.0 -target x -O2 -c a.c -o /r/obj/a.o",
        );
        assert_eq!(normalize(&g, "/g"), normalize(&r, "/r"));
        assert_eq!(normalize(&g, "/g"), argv("-O2 -c a.c -o <root>/obj/a.o"));
    }

    #[test]
    fn units_are_paired_by_output_and_grouped_by_difference() {
        let rec = |side: &str, cwd: &str, out: &str, flags: &str| {
            format!(
                r#"{{"started":1,"argv":["cc","-c","x.c",{flags}"-o","{out}"],"compiler":"/cc","cwd":"/{side}{cwd}","outputs":[{{"path":"{out}","sha256":"y"}}],"wall-seconds":0.1,"exit":0,"slot":"cc"}}"#
            )
        };
        let g = [
            rec("g", "/lib", "a.o", r#""-O2","#),
            rec("g", "/lib", "b.o", r#""-O2","-Wno-x","#),
            rec("g", "/lib", "c.o", r#""-O2","-Wno-x","#),
            rec("g", "/lib", "d.o", r#""-O2","-g","#),
            rec("g", "/lib", "e.o", r#""-O2","-frucc-y","#),
            rec("g", "/lib", "only.o", ""),
        ]
        .join("\n");
        let r = [
            rec("r", "/lib", "a.o", r#""-O2","#),
            rec("r", "/lib", "b.o", r#""-O2","#),
            rec("r", "/lib", "c.o", r#""-O2","#),
            rec("r", "/lib", "d.o", r#""-g","-O2","#),
            rec("r", "/lib", "e.o", r#""-O2","#),
            rec("r", "/lib", "../bin/sh.o", ""),
        ]
        .join("\n");
        let gu = units(&parse_log(&g).0, "/g");
        let ru = units(&parse_log(&r).0, "/r");
        assert!(gu.contains_key("<root>/lib/a.o"));
        assert!(ru.contains_key("<root>/bin/sh.o"));
        let div =
            Divergences::parse("[[divergence]]\nflag = \"-frucc-*\"\nreason = \"z\"\n").unwrap();
        let d = compare(&gu, &ru, "netbsd", &div);
        assert_eq!(d.paired, 5);
        assert_eq!(d.same, 2);
        assert_eq!(d.reordered, ["<root>/lib/d.o"]);
        assert_eq!(d.groups.len(), 1);
        assert_eq!(d.groups[0].only_g, ["-Wno-x"]);
        assert!(d.groups[0].only_r.is_empty());
        assert_eq!(d.groups[0].units.len(), 2);
        assert_eq!(d.allowed["-frucc-y"].1, 1);
        assert_eq!(d.only_g, ["<root>/lib/only.o"]);
        assert_eq!(d.only_r, ["<root>/bin/sh.o"]);
        assert_eq!(d.counted(false), 5);
        assert_eq!(d.counted(true), 3);
        let text = d.report(false, 10);
        assert!(
            text.contains("2 units: G has -Wno-x; R has nothing more"),
            "{text}"
        );
        assert!(text.contains("-frucc-y in 1 units: z"), "{text}");
    }
}
