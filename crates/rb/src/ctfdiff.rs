//! `rb ctf-diff`: compare the CTF of G's and R's kernels and modules (plan 09.6).
//!
//! Each side is one ELF file with a `.SUNW_ctf` section, or a directory. In a directory every
//! 64-bit ELF file with a `.SUNW_ctf` section counts, such as the kernel and the `.kmod` files
//! of the modules set, and the files of the two sides are paired by their path under it.
//!
//! For each pair the comparison has four parts:
//!
//! | part | what must be the same |
//! |---|---|
//! | named types | each named struct, union, enum, typedef, integer and float, with its members, their types and bit offsets, or its values |
//! | all types | the types as a multiset of their kind and how a use of them is written, so anonymous types are matched by where they are used |
//! | data objects | the type of each data symbol, by name |
//! | functions | the return and argument types of each function symbol, by name |
//!
//! The data object and function sections are read in the order of the symbol table, as libctf
//! reads them. When a section is not used up exactly, its symbols are not compared, and the
//! report says so. A name can have many local symbols, so each side of a name is the set of
//! the types its symbols have. A symbol name is in one of these classes:
//!
//! | class | G | R | counted |
//! |---|---|---|---|
//! | not the same | types | other types | yes |
//! | lost | types | the symbol with no type | yes |
//! | gained | no type, no symbol, or fewer symbols | types, with G's among them | no |
//! | not in R | types, with R's among them | no symbol, or fewer, as when rucc inlined a static function | no |
//! | another number | types | the same types, in another number of symbols | no |
//!
//! The losses are the warnings and errors that `ctfconvert` and `ctfmerge` wrote in R's build
//! log and not in G's, given with `--g-log` and `--r-log`. They are lines such as
//! `WARNING: nbctfconvert: ...`.
//!
//! The gate holds when no part has a difference, no file is on one side only and R has no loss.

use crate::ctf::{self, Ctf};
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

/// The numbers of one side of a pair.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Stats {
    /// The bytes of the section.
    pub section_bytes: usize,
    /// The types.
    pub types: usize,
    /// The named types that [`ctf::named`] lists.
    pub named: usize,
    /// The bytes of the data object section.
    pub object_bytes: usize,
    /// The bytes of the function section.
    pub function_bytes: usize,
    /// The data symbols with a type.
    pub objects: usize,
    /// The function symbols with a type.
    pub functions: usize,
}

impl Stats {
    fn of(c: &Ctf, named: usize) -> Self {
        Self {
            section_bytes: c.section_bytes,
            types: c.types.len(),
            named,
            object_bytes: c.object_bytes,
            function_bytes: c.function_bytes,
            objects: c.objects.len(),
            functions: c.functions.len(),
        }
    }
}

/// The comparison of one pair of files.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Pair {
    /// The path of the pair, under the directories, or the file name.
    pub path: String,
    /// G's numbers.
    pub g: Stats,
    /// R's numbers.
    pub r: Stats,
    /// G's named types, each as `name<TAB>full form`.
    pub g_named: Vec<String>,
    /// R's named types.
    pub r_named: Vec<String>,
    /// The named types only G has in that form.
    pub named_g: Vec<String>,
    /// The named types only R has in that form.
    pub named_r: Vec<String>,
    /// The names of the named types that are not the same.
    pub names: Vec<String>,
    /// The types only G has, by kind and form, with how many.
    pub only_g: BTreeMap<(String, String), usize>,
    /// The types only R has.
    pub only_r: BTreeMap<(String, String), usize>,
    /// Whether the symbol sections of both sides were read to the end, so that the symbols could
    /// be compared.
    pub symbols_compared: bool,
    /// The data symbols whose types are not the same.
    pub objects: Symbols,
    /// The function symbols whose types are not the same.
    pub functions: Symbols,
}

/// The symbols of one kind whose types are not the same, by class. Each line is
/// `name<TAB>G's types<TAB>R's types`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Symbols {
    /// Both sides give the name types, and they are not the same. Counted.
    pub differ: Vec<String>,
    /// G gives the name a type, and R has the symbol with no type. Counted.
    pub lost: Vec<String>,
    /// R gives the name a type, and G gives it none or has no such symbol, or R has more
    /// symbols of the name, and the types of G's are among those of R's.
    pub gained: Vec<String>,
    /// G gives the name a type, and R has no such symbol, or fewer symbols of the name, with
    /// types among those of G's.
    pub not_in_r: Vec<String>,
    /// The same types, in another number of symbols.
    pub count: Vec<String>,
}

impl Symbols {
    /// How many names count.
    #[must_use]
    pub fn counted(&self) -> usize {
        self.differ.len() + self.lost.len()
    }

    /// The classes, with their names in the report and in `ctf-symbols.diff`.
    fn classes(&self) -> [(&'static str, &'static str, &Vec<String>); 5] {
        [
            ("not the same", "differ", &self.differ),
            ("lost", "lost", &self.lost),
            ("gained", "gained", &self.gained),
            ("not in R", "not-in-r", &self.not_in_r),
            ("another number", "count", &self.count),
        ]
    }
}

impl Pair {
    /// How many differences count in this pair.
    #[must_use]
    pub fn counted(&self) -> usize {
        self.names.len()
            + self.only_g.values().sum::<usize>()
            + self.only_r.values().sum::<usize>()
            + self.objects.counted()
            + self.functions.counted()
            + usize::from(!self.symbols_compared)
    }
}

/// Compare the CTF of two files.
#[must_use]
pub fn compare(path: &str, g: &Ctf, r: &Ctf) -> Pair {
    let (g_named, r_named) = (ctf::named(g), ctf::named(r));
    let only = |a: &[String], b: &[String]| -> Vec<String> {
        let b: std::collections::BTreeSet<&String> = b.iter().collect();
        a.iter().filter(|l| !b.contains(l)).cloned().collect()
    };
    let named_g = only(&g_named, &r_named);
    let named_r = only(&r_named, &g_named);
    let mut names: Vec<String> = named_g
        .iter()
        .chain(&named_r)
        .map(|l| l.split('\t').next().unwrap_or_default().to_string())
        .collect();
    names.sort();
    names.dedup();
    let (ga, ra) = (ctf::all(g), ctf::all(r));
    let minus = |a: &BTreeMap<(String, String), usize>, b: &BTreeMap<(String, String), usize>| {
        a.iter()
            .filter_map(|(k, &n)| {
                let m = b.get(k).copied().unwrap_or(0);
                (n > m).then(|| (k.clone(), n - m))
            })
            .collect::<BTreeMap<_, _>>()
    };
    let symbols_compared = g.symbols_match && r.symbols_match;
    let (objects, functions) = if symbols_compared {
        let objects = |c: &Ctf| {
            sides(
                c.objects
                    .iter()
                    .map(|(n, t)| (n.as_str(), ctf::reference(c, *t, 0))),
                &c.untyped_objects,
            )
        };
        let functions = |c: &Ctf| {
            sides(
                c.functions
                    .iter()
                    .map(|(n, f)| (n.as_str(), ctf::function_text(c, f))),
                &c.untyped_functions,
            )
        };
        (
            classify(&objects(g), &objects(r)),
            classify(&functions(g), &functions(r)),
        )
    } else {
        (Symbols::default(), Symbols::default())
    };
    Pair {
        path: path.to_string(),
        g: Stats::of(g, g_named.len()),
        r: Stats::of(r, r_named.len()),
        only_g: minus(&ga, &ra),
        only_r: minus(&ra, &ga),
        g_named,
        r_named,
        named_g,
        named_r,
        names,
        symbols_compared,
        objects,
        functions,
    }
}

/// What one side has under one symbol name: the types of its symbols, with how many symbols
/// have each, and how many symbols have no type.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Side {
    typed: BTreeMap<String, usize>,
    untyped: usize,
}

fn sides<'a>(
    typed: impl Iterator<Item = (&'a str, String)>,
    untyped: &[String],
) -> BTreeMap<String, Side> {
    let mut out: BTreeMap<String, Side> = BTreeMap::new();
    for (name, text) in typed {
        *out.entry(name.to_string())
            .or_default()
            .typed
            .entry(text)
            .or_insert(0) += 1;
    }
    for name in untyped {
        out.entry(name.clone()).or_default().untyped += 1;
    }
    out
}

/// One side of a name as the report writes it: its types, each with `xN` when N symbols have
/// it, `no type`, or `-` when there is no such symbol.
fn side_text(side: Option<&Side>) -> String {
    match side {
        None => "-".into(),
        Some(s) if s.typed.is_empty() => "no type".into(),
        Some(s) => s
            .typed
            .iter()
            .map(|(t, n)| {
                if *n > 1 {
                    format!("{t} x{n}")
                } else {
                    t.clone()
                }
            })
            .collect::<Vec<_>>()
            .join(" | "),
    }
}

/// Put each symbol name in its class.
fn classify(g: &BTreeMap<String, Side>, r: &BTreeMap<String, Side>) -> Symbols {
    let mut out = Symbols::default();
    let mut names: Vec<&String> = g.keys().chain(r.keys()).collect();
    names.sort();
    names.dedup();
    for name in names {
        let (gs, rs) = (g.get(name), r.get(name));
        let typed = |s: Option<&Side>| s.is_some_and(|s| !s.typed.is_empty());
        let class = match (typed(gs), typed(rs)) {
            (false, false) => continue,
            (true, true) => {
                let empty = BTreeMap::new();
                let gt = gs.map_or(&empty, |s| &s.typed);
                let rt = rs.map_or(&empty, |s| &s.typed);
                let within = |a: &BTreeMap<String, usize>, b: &BTreeMap<String, usize>| {
                    a.iter().all(|(t, n)| b.get(t).is_some_and(|m| m >= n))
                };
                if gt == rt {
                    continue;
                } else if within(gt, rt) {
                    &mut out.gained
                } else if within(rt, gt) {
                    &mut out.not_in_r
                } else if gt.keys().eq(rt.keys()) {
                    &mut out.count
                } else {
                    &mut out.differ
                }
            }
            (true, false) if rs.is_some() => &mut out.lost,
            (true, false) => &mut out.not_in_r,
            (false, true) => &mut out.gained,
        };
        class.push(format!("{name}\t{}\t{}", side_text(gs), side_text(rs)));
    }
    out
}

/// The lines of a build log that are a warning or an error of `ctfconvert` or `ctfmerge`.
#[must_use]
pub fn ctf_messages(log: &str) -> Vec<String> {
    log.lines()
        .filter(|line| is_ctf_message(line))
        .map(|l| l.trim().to_string())
        .collect()
}

/// Whether a line is a message of the CTF tools: `WARNING: PROG: ...` or `ERROR: PROG: ...`, as
/// their `whine` writes it, or `PROG: ...`, where PROG ends in `ctfconvert` or `ctfmerge`.
fn is_ctf_message(line: &str) -> bool {
    let tool = |word: &str| {
        let word = word.rsplit('/').next().unwrap_or(word);
        word.ends_with("ctfconvert") || word.ends_with("ctfmerge")
    };
    let line = line.trim();
    for level in ["WARNING: ", "ERROR: "] {
        if let Some(rest) = line.strip_prefix(level) {
            return rest.split(':').next().is_some_and(tool);
        }
    }
    line.split_once(": ")
        .is_some_and(|(prog, _)| !prog.contains(' ') && tool(prog))
}

/// The lines of R's log that G's does not have, each as often as R has it more.
#[must_use]
pub fn losses(g: &[String], r: &[String]) -> Vec<String> {
    let mut left: BTreeMap<&str, usize> = BTreeMap::new();
    for l in g {
        *left.entry(l).or_insert(0) += 1;
    }
    r.iter()
        .filter(|l| match left.get_mut(l.as_str()) {
            Some(n) if *n > 0 => {
                *n -= 1;
                false
            }
            _ => true,
        })
        .cloned()
        .collect()
}

/// The whole comparison.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Diff {
    /// The pairs, by path.
    pub pairs: Vec<Pair>,
    /// The files only G has.
    pub only_g: Vec<String>,
    /// The files only R has.
    pub only_r: Vec<String>,
    /// The CTF messages of G's log, when one was given.
    pub g_messages: Option<Vec<String>>,
    /// The CTF messages of R's log, when one was given.
    pub r_messages: Option<Vec<String>>,
}

impl Diff {
    /// R's messages that G does not have.
    #[must_use]
    pub fn losses(&self) -> Vec<String> {
        losses(
            self.g_messages.as_deref().unwrap_or_default(),
            self.r_messages.as_deref().unwrap_or_default(),
        )
    }

    /// Whether the gate holds.
    #[must_use]
    pub fn same(&self) -> bool {
        !self.pairs.is_empty()
            && self.pairs.iter().all(|p| p.counted() == 0)
            && self.only_g.is_empty()
            && self.only_r.is_empty()
            && self.losses().is_empty()
    }

    /// The markdown report. Each list stops after `limit` lines.
    #[must_use]
    pub fn report(&self, limit: usize) -> String {
        let mut s = String::new();
        let differ: Vec<&Pair> = self.pairs.iter().filter(|p| p.counted() > 0).collect();
        if self.pairs.len() > 1 {
            let _ = writeln!(
                s,
                "Files compared: {}. Files with differences: {}.\n",
                self.pairs.len(),
                differ.len()
            );
            if !differ.is_empty() {
                s.push_str("| file | named types | types only in G | types only in R | data objects | functions |\n|---|---|---|---|---|---|\n");
                for p in &differ {
                    let _ = writeln!(
                        s,
                        "| {} | {} | {} | {} | {} | {} |",
                        p.path,
                        p.names.len(),
                        p.only_g.values().sum::<usize>(),
                        p.only_r.values().sum::<usize>(),
                        symbols_cell(p, p.objects.counted()),
                        symbols_cell(p, p.functions.counted()),
                    );
                }
                s.push('\n');
            }
        }
        let shown: Vec<&Pair> = if self.pairs.len() == 1 {
            self.pairs.iter().collect()
        } else {
            differ.iter().take(limit).copied().collect()
        };
        for p in shown {
            if self.pairs.len() > 1 {
                let _ = writeln!(s, "### {}\n", p.path);
            }
            pair_report(&mut s, p, limit);
        }
        for (label, files) in [("G", &self.only_g), ("R", &self.only_r)] {
            if !files.is_empty() {
                let _ = writeln!(s, "Files with CTF only in {label}: {}.\n", files.len());
                list(&mut s, files, limit);
            }
        }
        if self.g_messages.is_some() || self.r_messages.is_some() {
            let losses = self.losses();
            let _ = writeln!(
                s,
                "ctfconvert and ctfmerge messages: G {}, R {}. Losses, the messages only R gives: {}.\n",
                self.g_messages.as_ref().map_or(0, Vec::len),
                self.r_messages.as_ref().map_or(0, Vec::len),
                losses.len()
            );
            list(&mut s, &losses, limit);
        }
        let _ = writeln!(
            s,
            "Gate: {}.",
            if self.same() {
                "the CTF is the same"
            } else {
                "the CTF is not the same"
            }
        );
        s
    }

    /// Write the named types of each side and the differences of each pair into `dir`. With one
    /// pair the files go in `dir`, else in `dir/<path>`.
    pub fn write(&self, dir: &Path) -> Result<(), String> {
        for p in &self.pairs {
            let d: PathBuf = if self.pairs.len() == 1 {
                dir.to_path_buf()
            } else {
                dir.join(&p.path)
            };
            std::fs::create_dir_all(&d).map_err(|e| format!("{}: {e}", d.display()))?;
            let lines = |v: &[String], prefix: &str| -> String {
                v.iter().fold(String::new(), |mut s, l| {
                    let _ = writeln!(s, "{prefix}{l}");
                    s
                })
            };
            let types = |m: &BTreeMap<(String, String), usize>, prefix: &str| -> String {
                m.iter().fold(String::new(), |mut s, ((k, r), n)| {
                    let _ = writeln!(s, "{prefix}{n}\t{k}\t{r}");
                    s
                })
            };
            let symbols = |kind: &str, v: &Symbols| -> String {
                let mut s = String::new();
                for (_, class, lines) in v.classes() {
                    for l in lines {
                        let _ = writeln!(s, "{class}\t{kind}\t{l}");
                    }
                }
                s
            };
            let files = [
                ("ctf-g.txt", lines(&p.g_named, "")),
                ("ctf-r.txt", lines(&p.r_named, "")),
                (
                    "ctf-named.diff",
                    lines(&p.named_g, "< ") + &lines(&p.named_r, "> "),
                ),
                (
                    "ctf-all.diff",
                    types(&p.only_g, "< ") + &types(&p.only_r, "> "),
                ),
                (
                    "ctf-symbols.diff",
                    symbols("object", &p.objects) + &symbols("function", &p.functions),
                ),
            ];
            for (name, text) in files {
                let f = d.join(name);
                std::fs::write(&f, text).map_err(|e| format!("{}: {e}", f.display()))?;
            }
        }
        let losses = self.losses();
        if !losses.is_empty() {
            let f = dir.join("ctf-losses.txt");
            std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
            std::fs::write(&f, losses.join("\n") + "\n")
                .map_err(|e| format!("{}: {e}", f.display()))?;
        }
        Ok(())
    }
}

fn symbols_cell(p: &Pair, n: usize) -> String {
    if p.symbols_compared {
        n.to_string()
    } else {
        "not compared".into()
    }
}

fn list(s: &mut String, lines: &[String], limit: usize) {
    if lines.is_empty() {
        return;
    }
    s.push_str("```\n");
    for l in lines.iter().take(limit) {
        let _ = writeln!(s, "{l}");
    }
    if lines.len() > limit {
        let _ = writeln!(s, "... and {} more", lines.len() - limit);
    }
    s.push_str("```\n\n");
}

fn pair_report(s: &mut String, p: &Pair, limit: usize) {
    let rows = [
        ("section bytes", p.g.section_bytes, p.r.section_bytes),
        ("types", p.g.types, p.r.types),
        ("named types", p.g.named, p.r.named),
        ("data object bytes", p.g.object_bytes, p.r.object_bytes),
        ("function bytes", p.g.function_bytes, p.r.function_bytes),
        ("data objects with a type", p.g.objects, p.r.objects),
        ("functions with a type", p.g.functions, p.r.functions),
    ];
    s.push_str("| | G | R |\n|---|---|---|\n");
    for (label, g, r) in rows {
        let _ = writeln!(s, "| {label} | {g} | {r} |");
    }
    let _ = writeln!(
        s,
        "\nNamed types that are not the same: {}.\n",
        p.names.len()
    );
    list(s, &p.names, limit);
    for (label, only) in [("G", &p.only_g), ("R", &p.only_r)] {
        let mut kinds: BTreeMap<&str, usize> = BTreeMap::new();
        for ((kind, _), n) in only {
            *kinds.entry(kind).or_insert(0) += n;
        }
        let mut kinds: Vec<(&str, usize)> = kinds.into_iter().collect();
        kinds.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
        let detail = if kinds.is_empty() {
            "none".to_string()
        } else {
            kinds
                .iter()
                .map(|(k, n)| format!("{n} {k}"))
                .collect::<Vec<_>>()
                .join(", ")
        };
        let _ = writeln!(
            s,
            "Types only in {label}: {} ({detail}).\n",
            only.values().sum::<usize>()
        );
    }
    if p.symbols_compared {
        s.push_str("| symbols | not the same | lost | gained | not in R | another number |\n|---|---|---|---|---|---|\n");
        for (label, v) in [("data objects", &p.objects), ("functions", &p.functions)] {
            let n = v.classes().map(|(_, _, l)| l.len());
            let _ = writeln!(
                s,
                "| {label} | {} | {} | {} | {} | {} |",
                n[0], n[1], n[2], n[3], n[4]
            );
        }
        s.push_str(
            "\nNot the same and lost count. Each line is the name, then G's types, then R's.\n\n",
        );
        let lines: Vec<String> = [("object", &p.objects), ("function", &p.functions)]
            .iter()
            .flat_map(|(kind, v)| {
                v.classes()
                    .into_iter()
                    .take(2)
                    .flat_map(move |(_, class, l)| {
                        l.iter().map(move |l| format!("{class} {kind} {l}"))
                    })
            })
            .collect();
        list(s, &lines, limit);
    } else {
        s.push_str("The data object and function sections do not follow the symbol table on one side, so the symbols are not compared.\n\n");
    }
}

/// The files with CTF under `root`, by their path under it. A file is itself, by its name.
pub fn files(root: &Path) -> Result<BTreeMap<String, PathBuf>, String> {
    let mut out = BTreeMap::new();
    if root.is_file() {
        let name = root
            .file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        out.insert(name, root.to_path_buf());
        return Ok(out);
    }
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries = std::fs::read_dir(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        for entry in entries {
            let entry = entry.map_err(|e| format!("{}: {e}", dir.display()))?;
            let path = entry.path();
            let kind = entry.file_type().map_err(|e| e.to_string())?;
            if kind.is_dir() {
                stack.push(path);
            } else if kind.is_file() && is_elf(&path) {
                let data = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
                if ctf::has_ctf(&data) {
                    let rel = path
                        .strip_prefix(root)
                        .unwrap_or(&path)
                        .to_string_lossy()
                        .into_owned();
                    out.insert(rel, path);
                }
            }
        }
    }
    Ok(out)
}

fn is_elf(path: &Path) -> bool {
    use std::io::Read as _;
    let mut magic = [0u8; 4];
    std::fs::File::open(path)
        .and_then(|mut f| f.read_exact(&mut magic))
        .is_ok()
        && magic == *b"\x7fELF"
}

/// Compare two sides, each a file or a directory.
pub fn run(g: &Path, r: &Path) -> Result<Diff, String> {
    let (gf, rf) = (files(g)?, files(r)?);
    let read = |p: &Path| -> Result<Ctf, String> {
        let data = std::fs::read(p).map_err(|e| format!("{}: {e}", p.display()))?;
        ctf::read(&data).map_err(|e| format!("{}: {e}", p.display()))
    };
    let mut diff = Diff::default();
    if g.is_file() && r.is_file() {
        let name = gf.keys().next().cloned().unwrap_or_default();
        diff.pairs.push(compare(&name, &read(g)?, &read(r)?));
        return Ok(diff);
    }
    for (rel, gp) in &gf {
        match rf.get(rel) {
            Some(rp) => diff.pairs.push(compare(rel, &read(gp)?, &read(rp)?)),
            None => diff.only_g.push(rel.clone()),
        }
    }
    diff.only_r = rf
        .keys()
        .filter(|k| !gf.contains_key(*k))
        .cloned()
        .collect();
    Ok(diff)
}

/// The command line of `rb ctf-diff`.
pub mod cli {
    use super::{ctf_messages, run};
    use crate::cli::Args;
    use std::path::Path;

    /// Run `rb ctf-diff`. Returns the report and whether the gate holds.
    pub fn run_cli(args: &Args) -> Result<(String, bool), String> {
        let (Some(g), Some(r)) = (args.get("g"), args.get("r")) else {
            return Err(
                "give --g and --r, each a kernel or module with CTF, or a directory of them".into(),
            );
        };
        let limit = args.get("limit").map_or(Ok(40), |n| {
            n.parse()
                .map_err(|_| format!("--limit {n} is not a number"))
        })?;
        let mut diff = run(Path::new(g), Path::new(r))?;
        let log = |key: &str| -> Result<Option<Vec<String>>, String> {
            args.get(key)
                .map(|f| {
                    std::fs::read(f)
                        .map(|b| ctf_messages(&String::from_utf8_lossy(&b)))
                        .map_err(|e| format!("{f}: {e}"))
                })
                .transpose()
        };
        diff.g_messages = log("g-log")?;
        diff.r_messages = log("r-log")?;
        if let Some(out) = args.get("out") {
            diff.write(Path::new(out))?;
        }
        Ok((diff.report(limit), diff.same()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ctf::{Detail, Function, Type};

    fn int(name: &str) -> Type {
        Type {
            name: name.into(),
            kind: 1,
            size: 4,
            detail: Detail::Encoding(1, 0, 32),
        }
    }

    fn strukt(name: &str, member_type: u32, size: u64) -> Type {
        Type {
            name: name.into(),
            kind: 6,
            size,
            detail: Detail::Members(vec![("a".into(), member_type, 0)]),
        }
    }

    fn ctf(types: Vec<Type>) -> Ctf {
        Ctf {
            types: types
                .into_iter()
                .enumerate()
                .map(|(i, t)| (u32::try_from(i).unwrap() + 1, t))
                .collect(),
            symbols_match: true,
            ..Ctf::default()
        }
    }

    #[test]
    fn the_same_types_with_other_ids() {
        let g = ctf(vec![int("int"), strukt("s", 1, 4)]);
        let mut r = ctf(vec![strukt("s", 2, 4), int("int")]);
        r.objects.push(("x".into(), 1));
        let mut g2 = g.clone();
        g2.objects.push(("x".into(), 2));
        let p = compare("netbsd", &g2, &r);
        assert_eq!(p.counted(), 0, "{p:?}");
    }

    #[test]
    fn a_struct_with_another_member_type() {
        let g = ctf(vec![int("int"), int("short"), strukt("s", 1, 4)]);
        let r = ctf(vec![int("int"), int("short"), strukt("s", 2, 4)]);
        let p = compare("netbsd", &g, &r);
        assert_eq!(p.names, vec!["s".to_string()]);
        assert_eq!(p.named_g.len(), 1);
        assert_eq!(
            p.only_g.values().sum::<usize>(),
            0,
            "the use of s is written by name"
        );
        let d = Diff {
            pairs: vec![p],
            ..Diff::default()
        };
        assert!(!d.same());
        let text = d.report(10);
        assert!(
            text.contains("Named types that are not the same: 1."),
            "{text}"
        );
        assert!(text.contains("Gate: the CTF is not the same."), "{text}");
    }

    #[test]
    fn an_anonymous_type_is_matched_by_its_use() {
        let mut g = ctf(vec![int("int"), strukt("", 1, 4)]);
        g.types.insert(
            3,
            Type {
                name: "t".into(),
                kind: 10,
                size: 2,
                detail: Detail::None,
            },
        );
        let mut r = g.clone();
        r.types.insert(2, strukt("", 1, 8));
        let p = compare("netbsd", &g, &r);
        assert_eq!(p.names, vec!["t".to_string()]);
        assert_eq!(
            p.only_g.values().sum::<usize>(),
            1,
            "the typedef is written by name"
        );
        assert_eq!(p.only_r.values().sum::<usize>(), 1);
    }

    #[test]
    fn function_types_by_name() {
        let mut g = ctf(vec![int("int"), int("long")]);
        let mut r = g.clone();
        g.functions.push((
            "f".into(),
            Function {
                ret: 1,
                args: vec![1, 0],
            },
        ));
        r.functions.push((
            "f".into(),
            Function {
                ret: 1,
                args: vec![2, 0],
            },
        ));
        r.functions.push((
            "g".into(),
            Function {
                ret: 0,
                args: vec![],
            },
        ));
        let p = compare("netbsd", &g, &r);
        assert_eq!(
            p.functions.differ,
            vec!["f\tint (int, ...)\tint (long, ...)".to_string()]
        );
        assert_eq!(p.functions.gained, vec!["g\t-\tvoid ()".to_string()]);
        assert_eq!(p.counted(), 1);
    }

    #[test]
    fn symbol_classes() {
        let mut g = ctf(vec![int("int"), int("long")]);
        let mut r = g.clone();
        g.objects.extend([
            ("lost".into(), 1),
            ("gone".into(), 1),
            ("n".into(), 1),
            ("n".into(), 1),
            ("n".into(), 2),
            ("more".into(), 1),
        ]);
        g.untyped_objects.push("gained".into());
        r.untyped_objects.push("lost".into());
        r.objects.extend([
            ("gained".into(), 2),
            ("n".into(), 1),
            ("n".into(), 2),
            ("n".into(), 2),
            ("more".into(), 1),
            ("more".into(), 2),
        ]);
        let p = compare("netbsd", &g, &r);
        let o = &p.objects;
        assert_eq!(o.lost, vec!["lost\tint\tno type".to_string()]);
        assert_eq!(o.not_in_r, vec!["gone\tint\t-".to_string()]);
        assert_eq!(
            o.gained,
            vec![
                "gained\tno type\tlong".to_string(),
                "more\tint\tint | long".to_string()
            ]
        );
        assert_eq!(o.count, vec!["n\tint x2 | long\tint | long x2".to_string()]);
        assert_eq!(p.counted(), 1);
    }

    #[test]
    fn symbols_that_are_not_read_to_the_end_count_once() {
        let g = ctf(vec![int("int")]);
        let mut r = g.clone();
        r.symbols_match = false;
        let p = compare("netbsd", &g, &r);
        assert_eq!(p.counted(), 1);
        let d = Diff {
            pairs: vec![p],
            ..Diff::default()
        };
        assert!(d.report(5).contains("not compared"));
    }

    #[test]
    fn ctf_tool_messages_and_losses() {
        let log = "\
#   compile  GENERIC/vers.o
/t/bin/nbctfconvert -g -L VERSION -g vers.o
WARNING: nbctfconvert: file vers.o: die 52: unknown base type encoding 0x10
ERROR: nbctfmerge: no CTF data in a.o
nbctfconvert: x.o: cannot read DWARF
cc: error: something
";
        let m = ctf_messages(log);
        assert_eq!(m.len(), 3, "{m:?}");
        let g = vec![m[2].clone()];
        assert_eq!(losses(&g, &m), vec![m[0].clone(), m[1].clone()]);
        let d = Diff {
            pairs: vec![compare("netbsd", &ctf(vec![]), &ctf(vec![]))],
            g_messages: Some(g),
            r_messages: Some(m),
            ..Diff::default()
        };
        assert!(!d.same());
        assert!(
            d.report(5)
                .contains("Losses, the messages only R gives: 2.")
        );
    }
}
