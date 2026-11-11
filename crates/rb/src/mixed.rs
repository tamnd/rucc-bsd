//! `rb mixed`: the object that makes R fail, found by bisection over mixed objects (plan 12.7).
//!
//! R and G compiled the same units with the same command lines (`rb flags-diff`), so each of R's
//! objects can be replaced with G's. The objects are the units that both compile logs have,
//! sorted by path. Step k puts R's objects of the first k units in place and G's objects of the
//! rest, under `--into`. Then it runs the link command and the test command, each with `sh -c`.
//! The test exits 0 when the result behaves as G does (good), 125 when it cannot tell, and with
//! any other status when it fails as R does (bad). A mix that does not link is one the test
//! cannot tell.
//!
//! First the two ends are checked: all of G's objects (k = 0) must be good, and all of R's (k = n)
//! bad. Then a binary search finds the smallest k that is bad, and unit k is the object. When
//! two objects fail only together, the answer is the one that makes the first k - 1 of R's
//! objects fail.
//!
//! A struct layout difference between R and G makes every mix that uses the struct wrong, so the
//! search would stop at whichever object meets it first. So rb mixed refuses to run when the CTF
//! of the two kernels, `--ctf-g` and `--ctf-r` as `rb ctf-diff` reads them, has a named type that
//! differs. `--allow-types` names the types whose layout was checked by hand, such as a type that
//! differs only in which of two same-named types the CTF kept.

use crate::objdiff;
use harness_shim::record::CompileRecord;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

/// One unit that both builds compiled.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Object {
    /// The object's path with the build's root replaced by `<root>`.
    pub key: String,
    /// The source, with the root replaced.
    pub source: String,
    /// R's object.
    pub r: String,
    /// G's object.
    pub g: String,
    /// Where the link takes it from.
    pub place: PathBuf,
    /// R's command line.
    pub argv: Vec<String>,
}

/// The object a link takes: `X.o` in place of `X.o.o` when the build linked `X.o.o` into `X.o`
/// with `ld -r` and removed it, as NetBSD's kernel build does for libkern.
fn linked(path: &str) -> String {
    match path.strip_suffix(".o.o") {
        Some(base) if !Path::new(path).exists() => format!("{base}.o"),
        _ => path.to_string(),
    }
}

/// The objects of two builds, paired.
#[derive(Debug, Clone, Default)]
pub struct Pairing {
    /// The units both logs have and whose objects are on disk on both sides, sorted by key.
    pub objects: Vec<Object>,
    /// How many units only R has.
    pub only_r: usize,
    /// How many units only G has.
    pub only_g: usize,
    /// How many units both have, without the object of one side on disk.
    pub missing: usize,
}

/// Pair the units of two compile logs. `into` replaces `<root>` in the key to give the place of
/// each object.
#[must_use]
pub fn objects(
    g: &[CompileRecord],
    g_root: &str,
    r: &[CompileRecord],
    r_root: &str,
    into: &str,
) -> Pairing {
    let g_units = objdiff::units(g, g_root);
    let mut r_units: BTreeMap<String, (objdiff::Unit, Vec<String>)> = BTreeMap::new();
    for rec in r {
        if let Some((key, unit)) = objdiff::unit(rec, r_root) {
            r_units.insert(key, (unit, rec.argv.clone()));
        }
    }
    let mut out = Pairing::default();
    for (key, (unit, argv)) in &r_units {
        let Some(gu) = g_units.get(key) else {
            out.only_r += 1;
            continue;
        };
        let (r, g) = (linked(&unit.path), linked(&gu.path));
        if !Path::new(&r).is_file() || !Path::new(&g).is_file() {
            out.missing += 1;
            continue;
        }
        let mut place = key.replace("<root>", into);
        if r != unit.path
            && let Some(base) = place.strip_suffix(".o.o")
        {
            place = format!("{base}.o");
        }
        out.objects.push(Object {
            key: key.clone(),
            source: unit.source.clone(),
            r,
            g,
            place: PathBuf::from(place),
            argv: argv.clone(),
        });
    }
    out.only_g = g_units.keys().filter(|k| !r_units.contains_key(*k)).count();
    out
}

/// What the test said of one step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// It behaves as G does.
    Good,
    /// It fails as R does.
    Bad,
    /// The test could not tell.
    Skip,
}

impl Verdict {
    /// The verdict of a test's exit status: 0 good, 125 cannot tell, anything else bad.
    #[must_use]
    pub fn of(code: Option<i32>) -> Self {
        match code {
            Some(0) => Self::Good,
            Some(125) => Self::Skip,
            _ => Self::Bad,
        }
    }

    fn word(self) -> &'static str {
        match self {
            Self::Good => "good",
            Self::Bad => "bad",
            Self::Skip => "cannot tell",
        }
    }
}

/// How the search ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Found {
    /// Unit k, from 1, is the object.
    Object(usize),
    /// All of G's objects are not good, or all of R's not bad, so there is nothing to search.
    Ends(String),
    /// The test could not tell at any step between `lo` R objects, good, and `hi`, bad.
    Untestable(usize, usize),
}

/// Bisect over `n` units. `probe(k)` gives the verdict with R's objects of the first k units.
/// Returns each step and how the search ended.
pub fn bisect(
    n: usize,
    mut probe: impl FnMut(usize) -> Result<Verdict, String>,
) -> Result<(Vec<(usize, Verdict)>, Found), String> {
    let mut steps = Vec::new();
    let mut ask = |k: usize, steps: &mut Vec<(usize, Verdict)>| -> Result<Verdict, String> {
        let v = probe(k)?;
        steps.push((k, v));
        Ok(v)
    };
    let all_g = ask(0, &mut steps)?;
    if all_g != Verdict::Good {
        let why = format!(
            "with all of G's objects the test says {}, so the link or the test is wrong",
            all_g.word()
        );
        return Ok((steps, Found::Ends(why)));
    }
    let all_r = ask(n, &mut steps)?;
    if all_r != Verdict::Bad {
        let why = format!(
            "with all of R's objects the test says {}, so there is nothing to find",
            all_r.word()
        );
        return Ok((steps, Found::Ends(why)));
    }
    let (mut lo, mut hi) = (0, n);
    let mut skipped = BTreeSet::new();
    while hi - lo > 1 {
        let mid = lo + (hi - lo) / 2;
        // The untried step nearest the middle.
        let Some(k) = (0..hi - lo)
            .flat_map(|d| [mid + d, mid.wrapping_sub(d)])
            .find(|k| *k > lo && *k < hi && !skipped.contains(k))
        else {
            return Ok((steps, Found::Untestable(lo, hi)));
        };
        match ask(k, &mut steps)? {
            Verdict::Good => lo = k,
            Verdict::Bad => hi = k,
            Verdict::Skip => {
                skipped.insert(k);
            }
        }
    }
    Ok((steps, Found::Object(hi)))
}

/// The objects in place for one step, copied only where they change.
pub struct Mix<'a> {
    objects: &'a [Object],
    r_in_place: Vec<Option<bool>>,
}

impl<'a> Mix<'a> {
    /// A mix that knows nothing of what is in place.
    #[must_use]
    pub fn new(objects: &'a [Object]) -> Self {
        Self {
            objects,
            r_in_place: vec![None; objects.len()],
        }
    }

    /// Put R's objects of the first k units in place and G's of the rest. Returns how many
    /// were copied. A copy is newer than what the link made before, so make links again.
    pub fn set(&mut self, k: usize) -> Result<usize, String> {
        let mut copied = 0;
        for (i, o) in self.objects.iter().enumerate() {
            let want = i < k;
            if self.r_in_place[i] == Some(want) {
                continue;
            }
            let from = if want { &o.r } else { &o.g };
            if let Some(dir) = o.place.parent() {
                std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
            }
            std::fs::copy(from, &o.place)
                .map_err(|e| format!("{from} to {}: {e}", o.place.display()))?;
            self.r_in_place[i] = Some(want);
            copied += 1;
        }
        Ok(copied)
    }
}

/// Run a command with `sh -c`, with its output on standard error, and return its exit status.
fn shell(command: &str, k: usize, n: usize) -> Result<Option<i32>, String> {
    let status = std::process::Command::new("sh")
        .arg("-c")
        .arg(command)
        .env("RB_MIXED_R", k.to_string())
        .env("RB_MIXED_UNITS", n.to_string())
        .stdout(std::io::stderr())
        .status()
        .map_err(|e| format!("sh -c {command}: {e}"))?;
    Ok(status.code())
}

/// The named types that differ in the CTF of the two kernels and are not allowed.
pub fn layout_gate(g: &Path, r: &Path, allowed: &[String]) -> Result<Vec<String>, String> {
    let diff = crate::ctfdiff::run(g, r)?;
    if diff.pairs.is_empty() {
        return Err(format!(
            "{} and {} have no CTF to compare",
            g.display(),
            r.display()
        ));
    }
    let names: BTreeSet<String> = diff
        .pairs
        .iter()
        .flat_map(|p| p.names.iter().cloned())
        .filter(|n| !allowed.contains(n))
        .collect();
    Ok(names.into_iter().collect())
}

/// The counts of a pairing, as one sentence.
fn counts(p: &Pairing) -> String {
    format!(
        "rb mixed: {} objects paired, R only: {}, G only: {}, not on disk: {}",
        p.objects.len(),
        p.only_r,
        p.only_g,
        p.missing
    )
}

/// The search as markdown.
#[must_use]
pub fn report(pairing: &Pairing, steps: &[(usize, Verdict, f64)], found: &Found) -> String {
    let objects = &pairing.objects;
    let mut s = format!(
        "# rb mixed\n\n{}. Steps: {}.\n\n",
        counts(pairing),
        steps.len()
    );
    match found {
        Found::Object(k) => {
            let o = &objects[k - 1];
            let _ = writeln!(
                s,
                "The object is unit {k}: {}. With R's objects of the first {} units the test is good, and with unit {k} too it is bad.\n",
                o.key,
                k - 1
            );
            let _ = writeln!(
                s,
                "- source: {}\n- R: {}\n- G: {}\n\nR's command line:\n\n```\n{}\n```\n",
                o.source,
                o.r,
                o.g,
                o.argv.join(" ")
            );
        }
        Found::Ends(why) => {
            let _ = writeln!(s, "No search: {why}.\n");
        }
        Found::Untestable(lo, hi) => {
            let _ = writeln!(
                s,
                "The test could not tell at any step between {lo} R objects, good, and {hi}, bad. The object is one of units {} to {hi}.\n",
                lo + 1
            );
        }
    }
    s.push_str("| step | R objects | verdict | seconds |\n|---|---|---|---|\n");
    for (i, (k, v, secs)) in steps.iter().enumerate() {
        let _ = writeln!(s, "| {} | {k} | {} | {secs:.0} |", i + 1, v.word());
    }
    s
}

/// The command line of `rb mixed`.
pub mod cli {
    use super::{Mix, Verdict, bisect, counts, layout_gate, objects, report, shell};
    use crate::cli::Args;
    use harness_shim::record::read_log;
    use std::fmt::Write as _;
    use std::path::Path;
    use std::time::Instant;

    /// Run `rb mixed`, and return the report and whether it found the object.
    pub fn run(args: &Args) -> Result<(String, bool), String> {
        let need = |key: &str| {
            args.get(key)
                .ok_or_else(|| format!("give --{key}; see rb help"))
        };
        let read = |log: &str| {
            read_log(Path::new(log))
                .map(|(records, _)| records)
                .map_err(|e| format!("{log}: {e}"))
        };
        let into = need("into")?;
        let pairing = objects(
            &read(need("g")?)?,
            args.get("g-root").unwrap_or_default(),
            &read(need("r")?)?,
            args.get("r-root").unwrap_or_default(),
            into,
        );
        if pairing.objects.is_empty() {
            return Err(format!("{}, so there is nothing to mix", counts(&pairing)));
        }
        let allowed: Vec<String> = args
            .get("allow-types")
            .map(|v| {
                v.split(',')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(String::from)
                    .collect()
            })
            .unwrap_or_default();
        let differ = layout_gate(
            Path::new(need("ctf-g")?),
            Path::new(need("ctf-r")?),
            &allowed,
        )?;
        if !differ.is_empty() {
            return Err(format!(
                "rb mixed does not run, because R and G differ in these named types, so a mix of their objects can be wrong: {}. Check each layout, and name the ones that agree with --allow-types.",
                differ.join(", ")
            ));
        }
        if args.has("dry-run") {
            let mut s = format!(
                "# rb mixed: a dry run\n\n{}. No named type differs in the CTF, other than the {} that --allow-types names.\n\n",
                counts(&pairing),
                allowed.len()
            );
            for o in pairing.objects.iter().take(5) {
                let _ = writeln!(s, "- {} from {} or {}", o.place.display(), o.r, o.g);
            }
            return Ok((s, true));
        }
        let (link, test) = (need("link")?, need("test")?);
        let n = pairing.objects.len();
        let mut mix = Mix::new(&pairing.objects);
        let mut timed = Vec::new();
        let (steps, found) = bisect(n, |k| {
            let start = Instant::now();
            let copied = mix.set(k)?;
            eprintln!("rb mixed: R's objects of {k} of {n} units, {copied} copied");
            // A mix that does not link cannot be tested.
            let code = shell(link, k, n)?;
            let v = if code == Some(0) {
                Verdict::of(shell(test, k, n)?)
            } else {
                eprintln!("rb mixed: the link exited {code:?}");
                Verdict::Skip
            };
            eprintln!("rb mixed: {k} of {n}: {v:?}");
            timed.push(start.elapsed().as_secs_f64());
            Ok(v)
        })?;
        let steps: Vec<_> = steps
            .iter()
            .zip(timed)
            .map(|((k, v), t)| (*k, *v, t))
            .collect();
        let ok = matches!(found, super::Found::Object(_));
        Ok((report(&pairing, &steps, &found), ok))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn search(n: usize, bad_from: usize) -> (Vec<usize>, Found) {
        let (steps, found) = bisect(n, |k| {
            Ok(if k >= bad_from {
                Verdict::Bad
            } else {
                Verdict::Good
            })
        })
        .unwrap();
        (steps.iter().map(|(k, _)| *k).collect(), found)
    }

    #[test]
    fn bisection_finds_the_first_bad_unit() {
        for n in 1..40 {
            for bad in 1..=n {
                let (steps, found) = search(n, bad);
                assert_eq!(found, Found::Object(bad), "n {n} bad {bad}");
                assert!(steps.len() <= 2 + (usize::BITS - n.leading_zeros()) as usize);
            }
        }
        let (steps, _) = search(3386, 1234);
        assert_eq!(steps[..2], [0, 3386]);
        assert!(steps.len() <= 14, "{steps:?}");
    }

    #[test]
    fn the_ends_are_checked_first() {
        let (steps, found) = search(10, 0);
        assert_eq!(steps, [0]);
        assert!(matches!(found, Found::Ends(w) if w.contains("link or the test is wrong")));
        let (steps, found) = search(10, 11);
        assert_eq!(steps, [0, 10]);
        assert!(matches!(found, Found::Ends(w) if w.contains("nothing to find")));
    }

    #[test]
    fn a_step_the_test_cannot_tell_is_passed_over() {
        // 4 cannot be told; the first bad unit is 7.
        let probe = |bad: usize| {
            move |k: usize| {
                Ok(match k {
                    4 => Verdict::Skip,
                    k if k >= bad => Verdict::Bad,
                    _ => Verdict::Good,
                })
            }
        };
        let (steps, found) = bisect(8, probe(7)).unwrap();
        assert_eq!(found, Found::Object(7));
        assert!(steps.contains(&(4, Verdict::Skip)));
        // When the first bad unit is 5, unit 4 or 5 is the object, as 4 cannot be told.
        let (_, found) = bisect(8, probe(5)).unwrap();
        assert_eq!(found, Found::Untestable(3, 5));
        // When no step between the ends can be told, the range is the answer.
        let (_, found) = bisect(3, |k| {
            Ok(match k {
                0 => Verdict::Good,
                3 => Verdict::Bad,
                _ => Verdict::Skip,
            })
        })
        .unwrap();
        assert_eq!(found, Found::Untestable(0, 3));
        assert_eq!(Verdict::of(Some(0)), Verdict::Good);
        assert_eq!(Verdict::of(Some(125)), Verdict::Skip);
        assert_eq!(Verdict::of(None), Verdict::Bad);
    }

    fn record(out: &str, src: &str) -> CompileRecord {
        let line = format!(
            r#"{{"started":0,"argv":["cc","-c","{src}","-o","{out}"],"compiler":"cc","cwd":"/","inputs":[{{"path":"{src}","sha256":"x"}}],"outputs":[{{"path":"{out}","sha256":"y"}}],"wall-seconds":0,"exit":0,"slot":"cc"}}"#
        );
        harness_shim::record::parse_log(&line).0.remove(0)
    }

    #[test]
    fn objects_are_paired_and_mixed_in_place() {
        let top = std::env::temp_dir().join(format!("rb-mixed-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&top);
        for (leg, text) in [("g", "G"), ("r", "R")] {
            let dir = top.join(leg).join("obj");
            std::fs::create_dir_all(&dir).unwrap();
            for name in ["a.o", "b.o", "k.o"] {
                std::fs::write(dir.join(name), format!("{text} {name}")).unwrap();
            }
        }
        // m.o is in both logs, and only G's is on disk.
        std::fs::write(top.join("g/obj/m.o"), "G m.o").unwrap();
        let (g, r) = (top.join("g"), top.join("r"));
        let rec = |root: &Path, name: &str| {
            record(
                &root.join("obj").join(name).display().to_string(),
                &format!("/src/{name}.c"),
            )
        };
        let g_log = vec![
            rec(&g, "a.o"),
            rec(&g, "b.o"),
            rec(&g, "k.o.o"),
            rec(&g, "g.o"),
            rec(&g, "m.o"),
        ];
        let r_log = vec![
            rec(&r, "b.o"),
            rec(&r, "a.o"),
            rec(&r, "k.o.o"),
            rec(&r, "r.o"),
            rec(&r, "m.o"),
        ];
        let into = top.join("link");
        let pairing = objects(
            &g_log,
            &g.display().to_string(),
            &r_log,
            &r.display().to_string(),
            &into.display().to_string(),
        );
        assert_eq!((pairing.only_r, pairing.only_g, pairing.missing), (1, 1, 1));
        let objects = &pairing.objects;
        let keys: Vec<_> = objects.iter().map(|o| o.key.as_str()).collect();
        assert_eq!(
            keys,
            ["<root>/obj/a.o", "<root>/obj/b.o", "<root>/obj/k.o.o"]
        );
        // k.o.o was linked into k.o and removed, so k.o is the object.
        assert_eq!(objects[2].place, into.join("obj/k.o"));
        assert!(objects[2].r.ends_with("/r/obj/k.o"));
        let mut mix = Mix::new(objects);
        assert_eq!(mix.set(1).unwrap(), 3);
        let read = |name: &str| std::fs::read_to_string(into.join("obj").join(name)).unwrap();
        assert_eq!(
            [read("a.o"), read("b.o"), read("k.o")],
            ["R a.o", "G b.o", "G k.o"]
        );
        assert_eq!(mix.set(2).unwrap(), 1);
        assert_eq!(read("b.o"), "R b.o");
        let steps = [
            (0, Verdict::Good, 1.0),
            (3, Verdict::Bad, 1.0),
            (1, Verdict::Good, 1.0),
            (2, Verdict::Bad, 1.0),
        ];
        let text = report(&pairing, &steps, &Found::Object(2));
        assert!(
            text.contains("The object is unit 2: <root>/obj/b.o."),
            "{text}"
        );
        assert!(text.contains("cc -c /src/b.o.c"), "{text}");
        std::fs::remove_dir_all(&top).unwrap();
    }
}
