//! `rb census`: which compiler made each object of a build (plan 10.4).
//!
//! The input is the `compile.jsonl` that `rb-cc`, `rb-c++`, `rb-cpp` and `rb-as` wrote during one
//! build. Every object file a record wrote gets the record's origin: `ref` in a G build, and
//! `rucc-c`, `rucc-asm`, `ref-cxx`, `ref-c`, `cxx-on-cc` or `other` in an R build. When a file is
//! written twice, the last record counts. With `--objdir`, the census also walks the object tree,
//! and every object there that no record wrote gets the origin `other`: it came from `ld -r`,
//! `objcopy`, `ar` or a compiler the shim did not wrap.
//!
//! The census does not judge a G build. In an R build it reports the two rules of 10.4: `ref-c`
//! must be empty in a graded build, and each `cxx-on-cc` object needs a reason in the project's
//! own Makefile. The share of `.text` bytes per origin needs the link maps of 10.4, which `rb-cc`
//! does not ask for yet, so this census counts objects only.

use harness_shim::record::{CompileRecord, read_log};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::{Component, Path, PathBuf};

/// The suffixes of object files: FreeBSD's `.o`, `.pico`, `.po`, `.nossppico` and `.pieo`, and
/// NetBSD's `.o`, `.pico`, `.po` and `.So`.
pub const OBJECT_SUFFIXES: &[&str] = &[".o", ".pico", ".po", ".So", ".nossppico", ".pieo"];

/// The origin of an object rucc compiled from C.
pub const RUCC_C: &str = "rucc-c";

/// The origin of an object rucc assembled.
pub const RUCC_ASM: &str = "rucc-asm";

/// The origin of an object that no record with an origin wrote.
pub const OTHER: &str = "other";

/// Whether a path names an object file.
#[must_use]
pub fn is_object(path: &str) -> bool {
    OBJECT_SUFFIXES.iter().any(|s| path.ends_with(s))
}

/// One object of the build.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Object {
    /// Its origin.
    pub origin: String,
    /// The slot of the record that wrote it, or empty when no record did.
    pub slot: String,
    /// Whether more than one record wrote it.
    pub rewritten: bool,
    /// Whether it is in the object tree, when one was walked.
    pub on_disk: Option<bool>,
}

/// One standalone preprocessor call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cpp {
    /// The input, absolute.
    pub input: String,
    /// The output, absolute, or `(stdout)`.
    pub output: String,
    /// How many compiler and assembler calls read the output afterwards.
    pub read_by: usize,
}

/// The census of one build.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Census {
    /// Records read.
    pub records: usize,
    /// Lines of the log that did not parse.
    pub skipped_lines: usize,
    /// Records that were probes.
    pub probes: usize,
    /// Calls that were not probes, by slot: (calls, failed).
    pub calls: BTreeMap<String, (usize, usize)>,
    /// Calls that compiled a source and linked it in one step, which leaves no object, by origin.
    pub linked_directly: BTreeMap<String, usize>,
    /// Calls delegated under a bring-up class, which make the run ungraded.
    pub delegated: usize,
    /// The sources of the delegated calls, by class, each with the issue its entry names and
    /// what rucc said first when it tried, if it did.
    pub delegated_units: BTreeMap<String, BTreeMap<String, (String, String)>>,
    /// The sides the records name.
    pub sides: BTreeSet<String>,
    /// Every object, by absolute path.
    pub objects: BTreeMap<String, Object>,
    /// Objects under the skipped parts of the object tree.
    pub skipped_objects: usize,
    /// The standalone preprocessor calls.
    pub cpp: Vec<Cpp>,
}

/// Read `log`, and walk `objdir` when given, leaving out the objects under `skip`, which are
/// paths relative to `objdir`.
pub fn run(log: &Path, objdir: Option<&Path>, skip: &[String]) -> Result<Census, String> {
    let (records, skipped) = read_log(log).map_err(|e| format!("{}: {e}", log.display()))?;
    let mut census = from_records(&records);
    census.skipped_lines = skipped;
    if let Some(objdir) = objdir {
        let root = objdir
            .canonicalize()
            .map_err(|e| format!("{}: {e}", objdir.display()))?;
        let skip: Vec<PathBuf> = skip.iter().map(|s| root.join(s)).collect();
        let mut found = Vec::new();
        walk(&root, &skip, &mut found, &mut census.skipped_objects)?;
        add_tree(&mut census, &found);
    }
    Ok(census)
}

/// The census of a list of records.
#[must_use]
pub fn from_records(records: &[CompileRecord]) -> Census {
    let mut census = Census {
        records: records.len(),
        ..Census::default()
    };
    let mut cpp_outputs: Vec<(String, String)> = Vec::new();
    let mut read: BTreeMap<String, usize> = BTreeMap::new();
    for r in records {
        if let Some(side) = &r.side {
            census.sides.insert(side.clone());
        }
        if r.probe {
            census.probes += 1;
            continue;
        }
        let slot = r.slot.clone().unwrap_or_else(|| "cc".to_string());
        let calls = census.calls.entry(slot.clone()).or_default();
        calls.0 += 1;
        if !r.succeeded() {
            calls.1 += 1;
        }
        if let Some(delegated) = &r.delegated {
            census.delegated += 1;
            // The shim writes the class, and for a named unit the issue after it, as in
            // `cxx-c tamnd/rucc#4097`.
            let (class, issue) = delegated
                .split_once(' ')
                .map_or((delegated.as_str(), ""), |(c, i)| (c, i.trim()));
            let source = r
                .inputs
                .first()
                .map_or_else(|| "(no input)".to_string(), |i| absolute(&r.cwd, &i.path));
            let said = r
                .stderr
                .lines()
                .find(|l| !l.trim().is_empty())
                .unwrap_or_default()
                .trim()
                .to_string();
            census
                .delegated_units
                .entry(class.to_string())
                .or_default()
                .insert(source, (issue.to_string(), said));
        }
        if slot == "cpp" {
            let input = r
                .inputs
                .first()
                .map_or_else(|| "(stdin)".to_string(), |i| absolute(&r.cwd, &i.path));
            let output = r
                .outputs
                .first()
                .map_or_else(|| "(stdout)".to_string(), |o| absolute(&r.cwd, &o.path));
            cpp_outputs.push((input, output));
            continue;
        }
        for input in &r.inputs {
            *read.entry(absolute(&r.cwd, &input.path)).or_default() += 1;
        }
        if !r.succeeded() {
            continue;
        }
        let objects: Vec<String> = r
            .outputs
            .iter()
            .filter(|o| is_object(&o.path))
            .map(|o| absolute(&r.cwd, &o.path))
            .collect();
        let origin = r.origin.clone();
        if objects.is_empty() {
            if let Some(origin) = origin.filter(|_| !r.argv.iter().skip(1).any(|a| a == "-c")) {
                *census.linked_directly.entry(origin).or_default() += 1;
            }
            continue;
        }
        let origin = origin.unwrap_or_else(|| OTHER.to_string());
        for path in objects {
            let rewritten = census.objects.contains_key(&path);
            census.objects.insert(
                path,
                Object {
                    origin: origin.clone(),
                    slot: slot.clone(),
                    rewritten,
                    on_disk: None,
                },
            );
        }
    }
    census.cpp = cpp_outputs
        .into_iter()
        .map(|(input, output)| Cpp {
            read_by: read.get(&output).copied().unwrap_or(0),
            input,
            output,
        })
        .collect();
    census
}

/// Add what a walk of the object tree found: objects no record wrote become `other`, and the
/// records' objects are marked as there or not.
pub fn add_tree(census: &mut Census, found: &[String]) {
    for object in census.objects.values_mut() {
        object.on_disk = Some(false);
    }
    for path in found {
        census
            .objects
            .entry(path.clone())
            .and_modify(|o| o.on_disk = Some(true))
            .or_insert_with(|| Object {
                origin: OTHER.to_string(),
                slot: String::new(),
                rewritten: false,
                on_disk: Some(true),
            });
    }
}

fn walk(
    dir: &Path,
    skip: &[PathBuf],
    found: &mut Vec<String>,
    skipped: &mut usize,
) -> Result<(), String> {
    let entries = std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    for entry in entries {
        let entry = entry.map_err(|e| format!("{}: {e}", dir.display()))?;
        let path = entry.path();
        let kind = entry
            .file_type()
            .map_err(|e| format!("{}: {e}", path.display()))?;
        if kind.is_dir() {
            if skip.iter().any(|s| path.starts_with(s)) {
                let mut inside = Vec::new();
                walk(&path, &[], &mut inside, skipped)?;
                *skipped += inside.len();
            } else {
                walk(&path, skip, found, skipped)?;
            }
        } else if kind.is_file() && is_object(&path.to_string_lossy()) {
            if skip.iter().any(|s| path.starts_with(s)) {
                *skipped += 1;
            } else {
                found.push(path.to_string_lossy().into_owned());
            }
        }
    }
    Ok(())
}

/// A path the record spelled relative to its working directory, made absolute without asking
/// the file system, so that a file deleted since still has a name.
#[must_use]
pub fn absolute(cwd: &str, path: &str) -> String {
    let joined = Path::new(cwd).join(path);
    let mut out = PathBuf::new();
    for part in joined.components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out.to_string_lossy().into_owned()
}

impl Census {
    /// Objects by origin.
    #[must_use]
    pub fn by_origin(&self) -> BTreeMap<&str, usize> {
        let mut out = BTreeMap::new();
        for o in self.objects.values() {
            *out.entry(o.origin.as_str()).or_default() += 1;
        }
        out
    }

    /// Objects rucc built: the origins `rucc-c` and `rucc-asm`.
    #[must_use]
    pub fn rucc_objects(&self) -> usize {
        self.objects
            .values()
            .filter(|o| o.origin == RUCC_C || o.origin == RUCC_ASM)
            .count()
    }

    /// Objects whose origin is `other` or `ref-c`, which 10.4 lists by file.
    #[must_use]
    pub fn foreign(&self) -> Vec<(&str, &Object)> {
        self.objects
            .iter()
            .filter(|(_, o)| o.origin == OTHER || o.origin == "ref-c")
            .map(|(p, o)| (p.as_str(), o))
            .collect()
    }

    /// Whether the build is a G build: every record that names a side names G.
    #[must_use]
    pub fn is_reference(&self) -> bool {
        !self.sides.is_empty() && self.sides.iter().all(|s| s == "G")
    }

    /// The census as markdown. At most `limit` lines of each list are printed.
    #[must_use]
    pub fn report(&self, title: &str, limit: usize) -> String {
        let mut s = String::new();
        if self.delegated > 0 {
            let classes: Vec<String> = self
                .delegated_units
                .iter()
                .map(|(class, units)| format!("{class} {}", units.len()))
                .collect();
            let _ = writeln!(
                s,
                "# Census of {title}: ungraded. Calls delegated under bring-up: {} ({})\n",
                self.delegated,
                classes.join(", ")
            );
        } else {
            let _ = writeln!(s, "# Census of {title}\n");
        }
        let units: usize = self.calls.values().map(|c| c.0).sum();
        let failed: usize = self.calls.values().map(|c| c.1).sum();
        let sides: Vec<&str> = self.sides.iter().map(String::as_str).collect();
        let _ = writeln!(
            s,
            "Records: {}. Probes: {}. Other calls: {units}, of which {failed} failed. Sides: {}.",
            self.records,
            self.probes,
            if sides.is_empty() {
                "none named".to_string()
            } else {
                sides.join(", ")
            }
        );
        if self.skipped_lines > 0 {
            let _ = writeln!(
                s,
                "\nLines of the log that did not parse, which are not counted: {}.",
                self.skipped_lines
            );
        }
        self.objects_part(&mut s);
        self.rules_part(&mut s);
        self.delegated_part(&mut s, limit);
        self.cpp_part(&mut s, limit);
        self.foreign_part(&mut s, limit);
        s
    }

    fn objects_part(&self, s: &mut String) {
        let total = self.objects.len();
        let _ = writeln!(
            s,
            "\n## Objects by origin\n\n| origin | objects | share |\n|---|---|---|"
        );
        for (origin, n) in self.by_origin() {
            let _ = writeln!(s, "| {origin} | {n} | {} |", percent(n, total));
        }
        let _ = writeln!(s, "| all | {total} | |");
        if !self.is_reference() {
            let rucc = self.rucc_objects();
            let _ = writeln!(
                s,
                "\nObjects rucc built (`rucc-c` and `rucc-asm`): {rucc} of {total}, {}.",
                percent(rucc, total)
            );
        }
        let rewritten = self.objects.values().filter(|o| o.rewritten).count();
        let missing = self
            .objects
            .values()
            .filter(|o| o.on_disk == Some(false))
            .count();
        if rewritten > 0 {
            let _ = writeln!(
                s,
                "\nObjects that more than one call wrote: {rewritten}. The last call counts."
            );
        }
        if missing > 0 {
            let _ = writeln!(
                s,
                "\nObjects that a call wrote and that are not in the object tree now: {missing}."
            );
        }
        if self.skipped_objects > 0 {
            let _ = writeln!(
                s,
                "\nObjects in the skipped parts of the object tree, which are not counted: {}.",
                self.skipped_objects
            );
        }
        if !self.linked_directly.is_empty() {
            let list: Vec<String> = self
                .linked_directly
                .iter()
                .map(|(o, n)| format!("{n} {o}"))
                .collect();
            let _ = writeln!(
                s,
                "\nCalls that compiled and linked in one step, and so left no object: {}.",
                list.join(", ")
            );
        }
    }

    fn rules_part(&self, s: &mut String) {
        let _ = writeln!(
            s,
            "\n## Calls by slot\n\n| slot | calls | failed |\n|---|---|---|"
        );
        for (slot, (calls, failed)) in &self.calls {
            let _ = writeln!(s, "| {slot} | {calls} | {failed} |");
        }

        let _ = writeln!(s, "\n## Rules\n");
        let origins = self.by_origin();
        let ref_c = origins.get("ref-c").copied().unwrap_or(0);
        let cxx_on_cc = origins.get("cxx-on-cc").copied().unwrap_or(0);
        if self.is_reference() {
            let _ = writeln!(
                s,
                "- This is a G build, so the `ref-c` and `cxx-on-cc` rules of an R build do not apply."
            );
        } else {
            let _ = writeln!(
                s,
                "- `ref-c` objects: {ref_c}. A graded build must have none."
            );
            let _ = writeln!(
                s,
                "- `cxx-on-cc` objects: {cxx_on_cc}. Each one needs a reason in the project's Makefile."
            );
        }
        let _ = writeln!(
            s,
            "- Delegated calls: {}. A run with any is ungraded.",
            self.delegated
        );
    }

    fn delegated_part(&self, s: &mut String, limit: usize) {
        if self.delegated_units.is_empty() {
            return;
        }
        let _ = writeln!(
            s,
            "\n## Delegated units\n\nEach unit here needs an issue before the run can be graded.\n\n| class | source | issue | what rucc said |\n|---|---|---|---|"
        );
        let rows: Vec<(&String, &String, &(String, String))> = self
            .delegated_units
            .iter()
            .flat_map(|(class, units)| units.iter().map(move |(u, said)| (class, u, said)))
            .collect();
        for (class, unit, (issue, said)) in rows.iter().take(limit) {
            let said = said.replace('|', "\\|");
            let issue = if issue.is_empty() {
                "none"
            } else {
                issue.as_str()
            };
            let _ = writeln!(s, "| {class} | `{unit}` | {issue} | {said} |");
        }
        more(s, rows.len(), limit);
    }

    fn cpp_part(&self, s: &mut String, limit: usize) {
        let _ = writeln!(s, "\n## Preprocessor\n");
        if self.cpp.is_empty() {
            let _ = writeln!(s, "No call went through `rb-cpp`.");
        } else {
            let feeds = self.cpp.iter().filter(|c| c.read_by > 0).count();
            let _ = writeln!(
                s,
                "Calls through `rb-cpp`: {}. Calls whose output a later compiler or assembler call read: {feeds}.\n\n| input | output | read by |\n|---|---|---|",
                self.cpp.len()
            );
            for c in self.cpp.iter().take(limit) {
                let _ = writeln!(s, "| `{}` | `{}` | {} |", c.input, c.output, c.read_by);
            }
            more(s, self.cpp.len(), limit);
        }
    }

    fn foreign_part(&self, s: &mut String, limit: usize) {
        let foreign = self.foreign();
        let _ = writeln!(s, "\n## Foreign objects\n");
        if foreign.is_empty() {
            let _ = writeln!(s, "None.");
        } else {
            let _ = writeln!(
                s,
                "Objects with the origin `other` or `ref-c`: {}.\n\n| object | origin | slot |\n|---|---|---|",
                foreign.len()
            );
            for (path, o) in foreign.iter().take(limit) {
                let slot = if o.slot.is_empty() {
                    "no record"
                } else {
                    &o.slot
                };
                let _ = writeln!(s, "| `{path}` | {} | {slot} |", o.origin);
            }
            more(s, foreign.len(), limit);
        }
    }

    /// Every object as a line of tab-separated values: path, origin and slot.
    #[must_use]
    pub fn objects_tsv(&self) -> String {
        let mut s = String::from("object\torigin\tslot\n");
        for (path, o) in &self.objects {
            let _ = writeln!(s, "{path}\t{}\t{}", o.origin, o.slot);
        }
        s
    }
}

fn more(s: &mut String, total: usize, limit: usize) {
    if total > limit {
        let _ = writeln!(s, "\nNot listed: {} more.", total - limit);
    }
}

fn percent(n: usize, total: usize) -> String {
    if total == 0 {
        return String::new();
    }
    #[allow(clippy::cast_precision_loss)]
    let p = n as f64 * 100.0 / total as f64;
    format!("{p:.1}%")
}

#[cfg(test)]
mod tests {
    use super::*;
    use harness_shim::record::parse_log;

    const G: &str = r#"{"started":1,"argv":["rb-cc","-c","a.c","-o","a.o"],"compiler":"/gcc","cwd":"/obj/lib","inputs":[{"path":"a.c","sha256":"x"}],"outputs":[{"path":"a.o","sha256":"y"}],"wall-seconds":0.1,"exit":0,"slot":"cc","side":"G","origin":"ref"}
{"started":1,"argv":["rb-cc","-c","a.c","-o","a.pico"],"compiler":"/gcc","cwd":"/obj/lib","inputs":[{"path":"a.c","sha256":"x"}],"outputs":[{"path":"a.pico","sha256":"y"}],"wall-seconds":0.1,"exit":0,"slot":"cc","side":"G","origin":"ref"}
{"started":1,"argv":["rb-cc","--version"],"compiler":"/gcc","cwd":"/obj","wall-seconds":0.1,"exit":0,"probe":true,"slot":"cc","side":"G"}
{"started":1,"argv":["rb-c++","-c","b.cc","-o","../bin/b.o"],"compiler":"/g++","cwd":"/obj/lib","inputs":[{"path":"b.cc","sha256":"x"}],"outputs":[{"path":"../bin/b.o","sha256":"y"}],"wall-seconds":0.1,"exit":0,"slot":"c++","side":"G","origin":"ref-cxx"}
{"started":1,"argv":["rb-cpp","x.in","x.h"],"compiler":"/cpp","cwd":"/obj/inc","inputs":[{"path":"x.in","sha256":"x"}],"outputs":[{"path":"x.h","sha256":"y"}],"wall-seconds":0.1,"exit":0,"slot":"cpp","side":"G"}
{"started":1,"argv":["rb-cc","-c","/obj/inc/x.h.c"],"compiler":"/gcc","cwd":"/obj/inc","inputs":[{"path":"x.h","sha256":"x"}],"outputs":[{"path":"c.o","sha256":"y"}],"wall-seconds":0.1,"exit":0,"slot":"cc","side":"G","origin":"ref"}
{"started":1,"argv":["rb-cc","-c","bad.c"],"compiler":"/gcc","cwd":"/obj","inputs":[{"path":"bad.c","sha256":"x"}],"wall-seconds":0.1,"exit":1,"slot":"cc","side":"G","origin":"ref"}
{"started":1,"argv":["rb-cc","-o","tool","tool.c"],"compiler":"/gcc","cwd":"/obj","inputs":[{"path":"tool.c","sha256":"x"}],"outputs":[{"path":"tool","sha256":"y"}],"wall-seconds":0.1,"exit":0,"slot":"cc","side":"G","origin":"ref"}
{"started":1,"argv":["rb-as","-o","s.o","s.s"],"compiler":"/as","cwd":"/obj","inputs":[{"path":"s.s","sha256":"x"}],"outputs":[{"path":"s.o","sha256":"y"}],"wall-seconds":0.1,"exit":0,"slot":"as","side":"G","origin":"other"}
not json
"#;

    #[test]
    fn objects_get_the_origin_of_the_call_that_wrote_them() {
        let (records, skipped) = parse_log(G);
        assert_eq!(skipped, 1);
        let c = from_records(&records);
        assert_eq!(c.records, 9);
        assert_eq!(c.probes, 1);
        assert_eq!(c.calls["cc"], (5, 1));
        assert_eq!(c.objects["/obj/lib/a.o"].origin, "ref");
        assert_eq!(c.objects["/obj/lib/a.pico"].origin, "ref");
        assert_eq!(c.objects["/obj/bin/b.o"].origin, "ref-cxx");
        assert_eq!(c.objects["/obj/s.o"].origin, "other");
        let origins = c.by_origin();
        assert_eq!(origins["ref"], 3);
        assert_eq!(c.linked_directly["ref"], 1);
        assert!(c.is_reference());
        assert_eq!(c.cpp.len(), 1);
        assert_eq!(c.cpp[0].output, "/obj/inc/x.h");
        assert_eq!(c.cpp[0].read_by, 1);
    }

    #[test]
    fn the_tree_adds_objects_no_call_wrote() {
        let mut c = from_records(&parse_log(G).0);
        add_tree(
            &mut c,
            &["/obj/lib/a.o".to_string(), "/obj/sys/kern.o".to_string()],
        );
        assert_eq!(c.objects["/obj/sys/kern.o"].origin, OTHER);
        assert_eq!(c.objects["/obj/lib/a.o"].on_disk, Some(true));
        assert_eq!(c.objects["/obj/lib/a.pico"].on_disk, Some(false));
        let foreign: Vec<&str> = c.foreign().iter().map(|(p, _)| *p).collect();
        assert_eq!(foreign, ["/obj/s.o", "/obj/sys/kern.o"]);
        let report = c.report("test", 1);
        assert!(report.contains("| ref | 3 | 50.0% |"), "{report}");
        assert!(report.contains("This is a G build"), "{report}");
        assert!(report.contains("Not listed: 1 more."), "{report}");
    }

    #[test]
    fn an_r_build_reports_the_rules() {
        let r = G
            .replace("\"side\":\"G\"", "\"side\":\"R\"")
            .replace(
                "\"origin\":\"ref-cxx\"",
                "\"origin\":\"cxx-on-cc\",\"delegated\":\"cxx-c tamnd/rucc#4097\"",
            )
            .replacen("\"origin\":\"ref\"", "\"origin\":\"rucc-c\"", 2);
        let c = from_records(&parse_log(&r).0);
        assert_eq!(c.rucc_objects(), 2);
        let report = c.report("r", 10);
        assert!(report.contains("Objects rucc built"), "{report}");
        assert!(report.contains("`cxx-on-cc` objects: 1."), "{report}");
        assert!(report.contains("Delegated calls: 1."), "{report}");
        assert!(
            report.starts_with(
                "# Census of r: ungraded. Calls delegated under bring-up: 1 (cxx-c 1)"
            ),
            "{report}"
        );
        assert!(report.contains("## Delegated units"), "{report}");
        assert!(report.contains(" | tamnd/rucc#4097 | "), "{report}");
        assert_eq!(c.delegated_units["cxx-c"].len(), 1);
    }

    #[test]
    fn the_walk_finds_objects_and_leaves_out_the_skipped_parts() {
        let root = std::env::temp_dir().join(format!("rb-census-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for dir in ["lib", "tmp/tools"] {
            std::fs::create_dir_all(root.join(dir)).unwrap();
        }
        for file in [
            "lib/a.o",
            "lib/a.pico",
            "lib/a.c",
            "tmp/tools/t.o",
            "tmp/x.o",
        ] {
            std::fs::write(root.join(file), "").unwrap();
        }
        let log = root.join("compile.jsonl");
        let real = root.canonicalize().unwrap();
        let line = format!(
            r#"{{"started":1,"argv":["rb-cc","-c","a.c"],"compiler":"/gcc","cwd":"{}","inputs":[{{"path":"a.c","sha256":"x"}}],"outputs":[{{"path":"a.o","sha256":"y"}}],"wall-seconds":0.1,"exit":0,"slot":"cc","side":"G","origin":"ref"}}"#,
            real.join("lib").display()
        );
        std::fs::write(&log, line + "\n").unwrap();
        let c = run(&log, Some(&root), &["tmp".to_string()]).unwrap();
        let _ = std::fs::remove_dir_all(&root);
        assert_eq!(c.skipped_objects, 2);
        assert_eq!(c.objects.len(), 2);
        let origins = c.by_origin();
        assert_eq!((origins["ref"], origins[OTHER]), (1, 1));
        assert!(c.objects_tsv().contains("lib/a.pico\tother\t\n"));
    }

    #[test]
    fn paths_are_made_absolute_without_the_file_system() {
        assert_eq!(absolute("/a/b", "../c/./d.o"), "/a/c/d.o");
        assert_eq!(absolute("/a/b", "/x/y.o"), "/x/y.o");
        assert!(is_object("x.pico") && is_object("y.So") && !is_object("z.c"));
    }
}
