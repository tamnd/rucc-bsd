//! `rb probes`: ask R's and G's compilers the questions a build asks, without building, and say
//! for each whether they answer the same (plan 05.3, 03.4, 03.7 and 03.13).
//!
//! Each probe is compared by what the build reads from it, not by its whole text. FreeBSD's
//! `bsd.compiler.mk` reads a version number from `--version`, so two banners that give the same
//! number are the same answer. A probe that the build only asks to see whether it fails is
//! compared by its exit status.
//!
//! The probes are a fixed list, and with `--from` also every call a G build's `compile.jsonl`
//! marks as a probe. A macro that differs and that an entry of `mk-divergences.toml` names is
//! listed with the entry's reason and does not count.

use crate::divergences::Divergences;
use harness_shim::record::CompileRecord;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Instant;

/// What the build reads from a probe, so what is compared.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reads {
    /// The compiler type and version that `bsd.compiler.mk` reads from `--version`.
    Identity,
    /// The GCC version in the `version` line of `-v`, which `newvers.sh` records.
    VersionLine,
    /// The macros of `-dM -E`.
    Macros,
    /// The exit status and the lines of standard output that are not line markers.
    Output,
    /// The exit status only.
    Status,
}

/// One question.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Probe {
    /// What it is called in the report.
    pub name: String,
    /// The arguments after the compiler's name.
    pub args: Vec<String>,
    /// Standard input.
    pub stdin: String,
    /// The directory it runs in, or the work directory.
    pub cwd: Option<PathBuf>,
    /// What is compared.
    pub reads: Reads,
}

impl Probe {
    fn new(name: &str, args: &[&str], stdin: &str, reads: Reads) -> Self {
        Self {
            name: name.to_string(),
            args: args.iter().map(ToString::to_string).collect(),
            stdin: stdin.to_string(),
            cwd: None,
            reads,
        }
    }
}

/// The `__has_feature` and `__has_extension` queries the systems' headers make (03.13).
const FEATURE_TESTS: &str = "\
#ifdef __has_extension
ext_c_static_assert=__has_extension(c_static_assert)
ext_c_alignas=__has_extension(c_alignas)
ext_c_atomic=__has_extension(c_atomic)
ext_c_generic_selections=__has_extension(c_generic_selections)
ext_c_thread_local=__has_extension(c_thread_local)
#else
no_has_extension
#endif
#ifdef __has_feature
feature_c_static_assert=__has_feature(c_static_assert)
feature_c_generic_selections=__has_feature(c_generic_selections)
feature_address_sanitizer=__has_feature(address_sanitizer)
feature_nullability=__has_feature(nullability)
#else
no_has_feature
#endif
builtin_align_up=__has_builtin(__builtin_align_up)
attribute_nonstring=__has_attribute(__nonstring__)
attribute_no_stack_protector=__has_attribute(no_stack_protector)
";

/// The flags of the kernels' and boot code's command lines that rucc refused or may refuse
/// (03.4). Each is asked on its own, on a one-line unit.
const FLAGS: &[&[&str]] = &[
    &["-finline-limit=8000"],
    &["-fdiagnostics-show-option"],
    &["--param", "inline-unit-growth=100"],
    &["--param", "large-function-growth=1000"],
    &["--param", "min-pagesize=1024"],
    &["-mrtd"],
    &["-mregparm=3"],
    &["-fstack-usage"],
    &["-ftrivial-auto-var-init=pattern"],
    &["-mindirect-branch=thunk", "-mindirect-branch-register"],
    &["-mcmodel=kernel"],
    &[
        "-mno-red-zone",
        "-mno-mmx",
        "-mno-sse",
        "-mno-avx",
        "-msoft-float",
    ],
    &["-fno-delete-null-pointer-checks"],
    &["-fstack-protector-strong"],
    &["-ffreestanding", "-nostdinc"],
];

/// The unit the flag probes compile.
pub const UNIT: &str = "int rb_probe(int x) { return x + 1; }\n";

/// The files of the small tree that the `-B` and `--sysroot` probes search, under the work
/// directory. `buildworld` gives `-B` the directory of the cross tools and, for lib32,
/// `WORLDTMP/usr/lib32`, and gives `--sysroot` `WORLDTMP` (plan 05.1). The `ld` is executable,
/// because GCC finds a program only if it can run it.
pub const TREE: &[&str] = &[
    "rb-b/ld",
    "rb-b/crt1.o",
    "rb-sysroot/usr/lib/crt1.o",
    "rb-sysroot/usr/lib/libc.so",
    "rb-sysroot/usr/lib32/crt1.o",
];

/// The `-B` and `--sysroot` probes, as `buildworld` uses the two flags. Each reads the tree of
/// [`TREE`].
const SEARCH: &[(&str, &[&str])] = &[
    (
        "-B the tools -print-prog-name=ld",
        &["-Brb-b/", "-print-prog-name=ld"],
    ),
    (
        "-B the tools -print-file-name=crt1.o",
        &["-Brb-b/", "-print-file-name=crt1.o"],
    ),
    (
        "--sysroot -print-sysroot",
        &["--sysroot=rb-sysroot", "-print-sysroot"],
    ),
    (
        "--sysroot -print-file-name=crt1.o",
        &["--sysroot=rb-sysroot", "-print-file-name=crt1.o"],
    ),
    (
        "--sysroot -print-file-name=libc.so",
        &["--sysroot=rb-sysroot", "-print-file-name=libc.so"],
    ),
    (
        "-m32 -B WORLDTMP/usr/lib32 --sysroot -print-file-name=crt1.o",
        &[
            "-m32",
            "-Brb-sysroot/usr/lib32",
            "--sysroot=rb-sysroot",
            "-print-file-name=crt1.o",
        ],
    ),
];

/// Make the files of [`TREE`] under `work`.
pub fn make_tree(work: &Path) -> Result<(), String> {
    for name in TREE {
        let path = work.join(name);
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        }
        std::fs::write(&path, "").map_err(|e| format!("{}: {e}", path.display()))?;
        if name.ends_with("/ld") {
            use std::os::unix::fs::PermissionsExt as _;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
                .map_err(|e| format!("{}: {e}", path.display()))?;
        }
    }
    Ok(())
}

/// The fixed probes.
#[must_use]
pub fn fixed() -> Vec<Probe> {
    let mut probes = vec![
        Probe::new("--version", &["--version"], "", Reads::Identity),
        Probe::new("-v", &["-v"], "", Reads::VersionLine),
        Probe::new(
            "echo __FreeBSD_cc_version | cc -E -",
            &["-E", "-"],
            "__FreeBSD_cc_version\n",
            Reads::Output,
        ),
        Probe::new(
            "-print-resource-dir",
            &["-print-resource-dir"],
            "",
            Reads::Status,
        ),
        Probe::new(
            "-print-prog-name=ld",
            &["-print-prog-name=ld"],
            "",
            Reads::Status,
        ),
        Probe::new(
            "-print-libgcc-file-name",
            &["-print-libgcc-file-name"],
            "",
            Reads::Status,
        ),
        Probe::new(
            "-dM -E (gnu11)",
            &["-std=gnu11", "-dM", "-E", "-"],
            "",
            Reads::Macros,
        ),
        Probe::new(
            "-dM -E (gnu99, -ffreestanding)",
            &["-std=gnu99", "-ffreestanding", "-dM", "-E", "-"],
            "",
            Reads::Macros,
        ),
        Probe::new(
            "-dM -E (-fPIC)",
            &["-fPIC", "-dM", "-E", "-"],
            "",
            Reads::Macros,
        ),
        Probe::new(
            "__has_feature and __has_extension (gnu11)",
            &["-std=gnu11", "-E", "-P", "-"],
            FEATURE_TESTS,
            Reads::Output,
        ),
        Probe::new(
            "__has_feature and __has_extension (gnu99)",
            &["-std=gnu99", "-E", "-P", "-"],
            FEATURE_TESTS,
            Reads::Output,
        ),
    ];
    for flags in FLAGS {
        let mut args: Vec<&str> = flags.to_vec();
        args.extend(["-c", "rb-probe.c", "-o", "rb-probe.o"]);
        probes.push(Probe::new(&flags.join(" "), &args, "", Reads::Status));
    }
    for (name, args) in SEARCH {
        probes.push(Probe::new(name, args, "", Reads::Output));
    }
    probes
}

/// The probes a build asked: the calls of `records` marked as probes in the C compiler slot, once
/// each. An output file is moved into the work directory, so that asking again writes nothing in
/// the tree. Returns the probes and the number of records that were not C compiler probes.
#[must_use]
pub fn from_records(records: &[CompileRecord]) -> (Vec<Probe>, usize) {
    let mut seen = BTreeSet::new();
    let mut probes = Vec::new();
    let mut other = 0;
    for r in records {
        let cc = r.slot.as_deref().is_none_or(|s| s == "cc");
        if !r.probe || !cc || r.argv.is_empty() {
            other += 1;
            continue;
        }
        let mut args: Vec<String> = Vec::new();
        let mut words = r.argv[1..].iter();
        while let Some(a) = words.next() {
            if a == "-o" {
                let out = words.next().cloned().unwrap_or_default();
                args.push(a.clone());
                args.push(if out == "/dev/null" || out == "-" {
                    out
                } else {
                    "rb-probe.out".to_string()
                });
            } else {
                args.push(a.clone());
            }
        }
        let key = (r.cwd.clone(), args.clone(), r.stdin.clone());
        if !seen.insert(key) {
            continue;
        }
        let reads = reads_of(&args);
        probes.push(Probe {
            name: format!("{}: {}", r.cwd, args.join(" ")),
            args,
            stdin: r.stdin.clone(),
            cwd: Some(PathBuf::from(&r.cwd)),
            reads,
        });
    }
    (probes, other)
}

/// What a build reads from a probe with these arguments.
fn reads_of(args: &[String]) -> Reads {
    let has = |f: &str| args.iter().any(|a| a == f);
    if has("--version") {
        Reads::Identity
    } else if args == ["-v"] {
        Reads::VersionLine
    } else if has("-dM") {
        Reads::Macros
    } else if has("-E")
        || args
            .iter()
            .any(|a| a.starts_with("-print-") || a.starts_with("-dump"))
    {
        Reads::Output
    } else {
        Reads::Status
    }
}

/// What one compiler answered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Answer {
    /// The exit status, or -1 when it did not run or died on a signal.
    pub status: i32,
    /// Standard output.
    pub stdout: String,
    /// Standard error.
    pub stderr: String,
    /// Milliseconds.
    pub ms: u128,
}

/// Ask `cc` one probe in `dir`. The shim's records of it go to `log`.
#[must_use]
pub fn ask(cc: &Path, probe: &Probe, dir: &Path, log: &Path) -> Answer {
    let start = Instant::now();
    let child = Command::new(cc)
        .args(&probe.args)
        .current_dir(probe.cwd.as_deref().unwrap_or(dir))
        .env("RB_COMPILE_LOG", log)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn();
    let mut child = match child {
        Ok(c) => c,
        Err(e) => {
            return Answer {
                status: -1,
                stdout: String::new(),
                stderr: format!("{}: {e}", cc.display()),
                ms: 0,
            };
        }
    };
    if let Some(mut input) = child.stdin.take() {
        let _ = input.write_all(probe.stdin.as_bytes());
    }
    let out = child.wait_with_output();
    let ms = start.elapsed().as_millis();
    match out {
        Ok(o) => Answer {
            status: o.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&o.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&o.stderr).into_owned(),
            ms,
        },
        Err(e) => Answer {
            status: -1,
            stdout: String::new(),
            stderr: e.to_string(),
            ms,
        },
    }
}

/// The type and version `bsd.compiler.mk` reads from `--version` output (05.3): the type from the
/// words, and the version from every word that matches `[1-9]*.[0-9]*`, joined and split at dots
/// as `awk -F.` does.
#[must_use]
pub fn identity(text: &str) -> (String, u64) {
    let words: Vec<&str> = text.split_whitespace().collect();
    let kind = if words
        .iter()
        .any(|w| *w == "gcc" || *w == "(GCC)" || w.ends_with("GNU"))
    {
        "gcc"
    } else if words.contains(&"clang") {
        "clang"
    } else {
        "unknown"
    };
    let matched: Vec<&str> = words.iter().copied().filter(|w| version_word(w)).collect();
    let line = matched.join(" ");
    let fields: Vec<&str> = line.split('.').collect();
    let num = |i: usize| fields.get(i).map_or(0, |f| awk_number(f));
    (kind.to_string(), num(0) * 10000 + num(1) * 100 + num(2))
}

/// Whether a word matches the make pattern `[1-9]*.[0-9]*`: a first digit other than 0, and a dot
/// after it that a digit follows.
fn version_word(w: &str) -> bool {
    let b = w.as_bytes();
    b.first().is_some_and(|c| (b'1'..=b'9').contains(c))
        && b[1..]
            .windows(2)
            .any(|p| p[0] == b'.' && p[1].is_ascii_digit())
}

/// The number `awk` reads from a field: its leading digits after blanks, or 0.
fn awk_number(field: &str) -> u64 {
    let digits: String = field
        .trim_start()
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    digits.parse().unwrap_or(0)
}

/// The GCC version in the `version` line of `-v` output: the word after `gcc version`.
fn version_line(text: &str) -> String {
    let words: Vec<&str> = text.split_whitespace().collect();
    words
        .windows(3)
        .find(|w| w[0].trim_start_matches('(') == "gcc" && w[1] == "version")
        .map_or_else(|| "none".to_string(), |w| w[2].to_string())
}

/// The macros of `-dM -E` output.
fn macros(text: &str) -> BTreeMap<String, String> {
    text.lines()
        .filter_map(|l| l.strip_prefix("#define "))
        .map(|l| {
            let (name, value) = l.split_once(' ').unwrap_or((l, ""));
            (name.to_string(), value.trim().to_string())
        })
        .collect()
}

/// Standard output without line markers and blank lines.
fn output_lines(text: &str) -> Vec<String> {
    text.lines()
        .map(str::trim_end)
        .filter(|l| !l.is_empty() && !l.starts_with("# ") && !l.starts_with("#line"))
        .map(str::to_string)
        .collect()
}

/// The first line of standard error, for a report.
fn first_error(a: &Answer) -> String {
    a.stderr
        .lines()
        .next()
        .unwrap_or("")
        .chars()
        .take(160)
        .collect()
}

/// How R's answer differs from G's, or nothing when it is the same.
#[must_use]
pub fn compare(reads: Reads, g: &Answer, r: &Answer) -> Vec<String> {
    let status = |a: &Answer| {
        if a.status == 0 {
            "succeeds".to_string()
        } else {
            format!("fails ({}): {}", a.status, first_error(a))
        }
    };
    if reads == Reads::Status || g.status != 0 || r.status != 0 {
        if (g.status == 0) != (r.status == 0) {
            return vec![format!("G {}, R {}", status(g), status(r))];
        }
        if reads == Reads::Status || g.status != 0 {
            return Vec::new();
        }
    }
    match reads {
        Reads::Status => Vec::new(),
        // The type is not compared: FreeBSD's toolchain files set X_COMPILER_TYPE, and NetBSD
        // does not ask.
        Reads::Identity => {
            let (gi, ri) = (identity(&g.stdout), identity(&r.stdout));
            if gi.1 == ri.1 {
                Vec::new()
            } else {
                vec![format!(
                    "bsd.compiler.mk reads G as {} {} and R as {} {}",
                    gi.0, gi.1, ri.0, ri.1
                )]
            }
        }
        Reads::VersionLine => {
            let all = |a: &Answer| format!("{}\n{}", a.stdout, a.stderr);
            let (gv, rv) = (version_line(&all(g)), version_line(&all(r)));
            if gv == rv {
                Vec::new()
            } else {
                vec![format!(
                    "the gcc version of -v is {gv} for G and {rv} for R"
                )]
            }
        }
        Reads::Macros => {
            let (gm, rm) = (macros(&g.stdout), macros(&r.stdout));
            let mut out = Vec::new();
            for (k, v) in &gm {
                match rm.get(k) {
                    None => out.push(format!("{k}: G {v}, R undefined")),
                    Some(rv) if rv != v => out.push(format!("{k}: G {v}, R {rv}")),
                    Some(_) => {}
                }
            }
            for (k, v) in &rm {
                if !gm.contains_key(k) {
                    out.push(format!("{k}: G undefined, R {v}"));
                }
            }
            out
        }
        Reads::Output => {
            let (gl, rl) = (output_lines(&g.stdout), output_lines(&r.stdout));
            if gl == rl {
                return Vec::new();
            }
            let gs: BTreeSet<_> = gl.iter().collect();
            let rs: BTreeSet<_> = rl.iter().collect();
            let mut out: Vec<String> = gs.difference(&rs).map(|l| format!("G only: {l}")).collect();
            out.extend(rs.difference(&gs).map(|l| format!("R only: {l}")));
            if out.is_empty() {
                out.push("the same lines in another order".to_string());
            }
            out
        }
    }
}

/// The result of one probe.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    /// The probe.
    pub probe: Probe,
    /// How R differs, or nothing.
    pub differs: Vec<String>,
    /// The macros that differ and that `mk-divergences.toml` allows, each with its reason.
    pub allowed: Vec<(String, String)>,
    /// R's time.
    pub r_ms: u128,
    /// Why it was not asked, when it was not.
    pub skipped: Option<String>,
}

/// Take out of `differs` the macro differences that `divergences` allows for `system`, and return
/// them with their reasons. A macro difference starts with the macro's name and a colon.
pub fn allow(
    differs: &mut Vec<String>,
    system: &str,
    divergences: &Divergences,
) -> Vec<(String, String)> {
    let mut allowed = Vec::new();
    differs.retain(|d| {
        let name = d.split_once(':').map_or(d.as_str(), |(n, _)| n);
        match divergences.predefined(system, name) {
            Some(reason) => {
                allowed.push((name.to_string(), reason.to_string()));
                false
            }
            None => true,
        }
    });
    allowed
}

/// Ask every probe of G and R in `work`. The macro differences that `divergences` allows for
/// `system` do not count.
pub fn run(
    g: &Path,
    r: &Path,
    probes: Vec<Probe>,
    work: &Path,
    system: &str,
    divergences: &Divergences,
) -> Result<Vec<Outcome>, String> {
    std::fs::create_dir_all(work).map_err(|e| format!("{}: {e}", work.display()))?;
    let unit = work.join("rb-probe.c");
    std::fs::write(&unit, UNIT).map_err(|e| format!("{}: {e}", unit.display()))?;
    make_tree(work)?;
    let mut out = Vec::new();
    for probe in probes {
        if let Some(cwd) = &probe.cwd
            && !cwd.is_dir()
        {
            out.push(Outcome {
                probe,
                differs: Vec::new(),
                allowed: Vec::new(),
                r_ms: 0,
                skipped: Some("its directory is not on this host".to_string()),
            });
            continue;
        }
        let ga = ask(g, &probe, work, &work.join("probes-G.jsonl"));
        let ra = ask(r, &probe, work, &work.join("probes-R.jsonl"));
        let mut differs = compare(probe.reads, &ga, &ra);
        let allowed = if probe.reads == Reads::Macros {
            allow(&mut differs, system, divergences)
        } else {
            Vec::new()
        };
        out.push(Outcome {
            differs,
            allowed,
            r_ms: ra.ms,
            skipped: None,
            probe,
        });
    }
    Ok(out)
}

/// The report, and the number of probes that differ. `all` prints the probes that are the same as
/// well.
#[must_use]
pub fn report(outcomes: &[Outcome], all: bool, details: usize) -> (String, usize) {
    let mut s = String::new();
    let (mut same, mut differ, mut skipped, mut slow) = (0, 0, 0, 0);
    let mut allowed: BTreeMap<&str, (&str, usize)> = BTreeMap::new();
    for o in outcomes {
        for (name, reason) in &o.allowed {
            allowed.entry(name).or_insert((reason, 0)).1 += 1;
        }
        if let Some(why) = &o.skipped {
            skipped += 1;
            if all {
                let _ = writeln!(s, "skipped  {}: {why}", o.probe.name);
            }
            continue;
        }
        if o.r_ms > 20 {
            slow += 1;
        }
        if o.differs.is_empty() {
            same += 1;
            if all {
                let _ = writeln!(s, "same     {} (R {} ms)", o.probe.name, o.r_ms);
            }
        } else {
            differ += 1;
            let _ = writeln!(s, "differs  {} (R {} ms)", o.probe.name, o.r_ms);
            for d in o.differs.iter().take(details) {
                let _ = writeln!(s, "         {d}");
            }
            if o.differs.len() > details {
                let _ = writeln!(s, "         and {} more", o.differs.len() - details);
            }
        }
    }
    if !allowed.is_empty() {
        s.push_str("allowed by mk-divergences.toml:\n");
        for (name, (reason, n)) in &allowed {
            let _ = writeln!(s, "         {name} in {n} probes: {reason}");
        }
    }
    let _ = writeln!(
        s,
        "probes: {} asked, {same} the same, {differ} differ, {skipped} skipped; R took more than 20 ms on {slow}",
        same + differ
    );
    (s, differ)
}

/// `rb probes` on the command line.
pub mod cli {
    use super::{fixed, from_records, report, run};
    use crate::cli::Args;
    use crate::divergences::Divergences;
    use crate::toolchain::cli::Files;
    use rb_shim::config::{FILE_NAME, ShimConfig};
    use std::fmt::Write as _;
    use std::path::{Path, PathBuf};

    /// The C compiler of a side: a file, or the name of the `cc` slot in a directory of shims or in
    /// the `bin` of a `TOOLDIR`.
    fn compiler(given: &str) -> Result<PathBuf, String> {
        let path = PathBuf::from(given);
        if !path.is_dir() {
            return Ok(path);
        }
        for bin in [path.join("bin"), path.clone()] {
            let file = bin.join(FILE_NAME);
            if let Ok(text) = std::fs::read_to_string(&file) {
                let config: ShimConfig =
                    toml::from_str(&text).map_err(|e| format!("{}: {e}", file.display()))?;
                if let Some((name, _)) = config.names.iter().find(|(_, slot)| *slot == "cc") {
                    return Ok(bin.join(name));
                }
            }
        }
        Err(format!(
            "{given} is a directory with no {FILE_NAME} that names a cc slot"
        ))
    }

    /// Run `rb probes`. Returns the report and whether every probe was the same.
    pub fn run_cli(args: &Args, root: &Path) -> Result<(String, bool), String> {
        let files = Files::load(root, args.target.as_deref(), args.get("row"))?;
        let expected = files.expected()?;
        let (g, r) = match (args.get("dir"), args.get("g"), args.get("r")) {
            (Some(d), None, None) => {
                let bin = Path::new(d).join("bin");
                (
                    bin.join(format!("{}-rb-cc-ref", expected.target)),
                    bin.join(format!("{}-rb-cc-rucc", expected.target)),
                )
            }
            (None, Some(g), Some(r)) => (compiler(g)?, compiler(r)?),
            _ => return Err("give --dir DIR, or --g and --r".into()),
        };
        let work = args.get("work").map_or_else(
            || std::env::temp_dir().join(format!("rb-probes-{}", std::process::id())),
            PathBuf::from,
        );
        let mut probes = fixed();
        let mut text = format!("G: {}\nR: {}\n", g.display(), r.display());
        if let Some(log) = args.get("from") {
            let (records, bad) = harness_shim::record::read_log(Path::new(log))
                .map_err(|e| format!("{log}: {e}"))?;
            let (more, other) = from_records(&records);
            let _ = writeln!(
                text,
                "{log}: {} records, {} probes of the C compiler asked once each, {other} other records, {bad} unreadable lines",
                records.len(),
                more.len()
            );
            probes.extend(more);
        }
        let divergences = Divergences::load(root, args.get("divergences").map(Path::new))?;
        let outcomes = run(&g, &r, probes, &work, &files.pin.system, &divergences)?;
        let details = args
            .get("details")
            .map(str::parse)
            .transpose()
            .map_err(|e| format!("--details: {e}"))?
            .unwrap_or(8);
        let (body, differ) = report(&outcomes, args.has("all"), details);
        text.push_str(&body);
        Ok((text, differ == 0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn answer(status: i32, stdout: &str) -> Answer {
        Answer {
            status,
            stdout: stdout.to_string(),
            stderr: String::new(),
            ms: 1,
        }
    }

    #[test]
    fn identity_reads_as_bsd_compiler_mk_does() {
        let gcc = "x86_64-unknown-freebsd15.1-gcc15 (FreeBSD Ports Collection for amd64) 15.2.0\n\
                   Copyright (C) 2025 Free Software Foundation, Inc.\n";
        assert_eq!(identity(gcc), ("unknown".to_string(), 150_200));
        let persona = "gcc (rucc 0.30.15, GNU C persona 15.2.0) 15.2.0\n\
                       This is free software under the Apache License 2.0. There is NO warranty.\n";
        assert_eq!(identity(persona), ("gcc".to_string(), 150_200));
        let bare = "rucc 0.30.15\nA C compiler for the GNU C dialect of GCC 16 from the Free\n\
                    This is free software under the Apache License 2.0. There is NO warranty.\n";
        assert_eq!(identity(bare).1, 20_000);
    }

    #[test]
    fn version_words_follow_the_make_pattern() {
        assert!(version_word("15.2.0"));
        assert!(version_word("2.0."));
        assert!(version_word("15.2.0)"));
        assert!(!version_word("0.30.15"));
        assert!(!version_word("16"));
        assert!(!version_word("2."));
    }

    #[test]
    fn macros_report_each_difference() {
        let g = answer(0, "#define __GNUC__ 15\n#define __pie__ 2\n");
        let r = answer(0, "#define __GNUC__ 16\n#define __rucc__ 1\n");
        let d = compare(Reads::Macros, &g, &r);
        assert_eq!(
            d,
            [
                "__GNUC__: G 15, R 16",
                "__pie__: G 2, R undefined",
                "__rucc__: G undefined, R 1"
            ]
        );
    }

    #[test]
    fn allowed_macros_do_not_count() {
        let d = Divergences::parse(
            "[[divergence]]\nmacro = \"__rucc*\"\nreason = \"rucc's name\"\n\n\
             [[divergence]]\nsystem = \"netbsd\"\nmacro = \"__VERSION__\"\nreason = \"text\"\n",
        )
        .unwrap();
        let g = answer(0, "#define __GNUC__ 15\n#define __VERSION__ \"15.2.0\"\n");
        let r = answer(
            0,
            "#define __GNUC__ 15\n#define __VERSION__ \"rucc\"\n#define __rucc__ 1\n#define __rucc_major__ 0\n",
        );
        let mut differs = compare(Reads::Macros, &g, &r);
        let allowed = allow(&mut differs, "freebsd", &d);
        assert_eq!(differs, [r#"__VERSION__: G "15.2.0", R "rucc""#]);
        assert_eq!(allowed.len(), 2);
        assert_eq!(
            allowed[0],
            ("__rucc__".to_string(), "rucc's name".to_string())
        );
        let mut differs = compare(Reads::Macros, &g, &r);
        assert_eq!(allow(&mut differs, "netbsd", &d).len(), 3);
        assert!(differs.is_empty());
        let outcome = Outcome {
            probe: Probe::new("-dM -E", &["-dM", "-E", "-"], "", Reads::Macros),
            differs,
            allowed: allow(&mut compare(Reads::Macros, &g, &r), "netbsd", &d),
            r_ms: 3,
            skipped: None,
        };
        let (text, differ) = report(&[outcome], false, 8);
        assert_eq!(differ, 0);
        assert!(
            text.contains("__rucc_major__ in 1 probes: rucc's name"),
            "{text}"
        );
        assert!(text.contains("1 asked, 1 the same, 0 differ"), "{text}");
    }

    #[test]
    fn the_search_probes_have_their_tree() {
        use std::os::unix::fs::PermissionsExt as _;
        let work = std::env::temp_dir().join(format!("rb-probes-tree-{}", std::process::id()));
        make_tree(&work).unwrap();
        for name in TREE {
            assert!(work.join(name).is_file(), "{name}");
        }
        let mode = std::fs::metadata(work.join("rb-b/ld"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o111, 0o111);
        let probes = fixed();
        for (name, args) in SEARCH {
            let p = probes.iter().find(|p| p.name == *name).unwrap();
            assert_eq!(p.reads, Reads::Output);
            for a in *args {
                let path = a
                    .strip_prefix("-B")
                    .or_else(|| a.strip_prefix("--sysroot="));
                if let Some(path) = path {
                    assert!(work.join(path).is_dir(), "{a}");
                }
            }
        }
        std::fs::remove_dir_all(&work).unwrap();
    }

    #[test]
    fn status_probes_compare_success_only() {
        let ok = answer(0, "/usr/lib/clang/19");
        let other = answer(0, "/opt/rucc/lib");
        let fail = answer(1, "");
        assert!(compare(Reads::Status, &ok, &other).is_empty());
        assert_eq!(compare(Reads::Status, &ok, &fail).len(), 1);
        assert!(compare(Reads::Output, &fail, &answer(2, "x")).is_empty());
    }

    #[test]
    fn output_skips_line_markers() {
        let g = answer(0, "# 1 \"<stdin>\"\n__FreeBSD_cc_version\n");
        let r = answer(0, "__FreeBSD_cc_version\n\n");
        assert!(compare(Reads::Output, &g, &r).is_empty());
        let v = compare(
            Reads::VersionLine,
            &answer(0, "gcc version 15.2.0 (FreeBSD)"),
            &answer(0, "rucc version 0.30.15 (gcc version 16.0.0 compatible)"),
        );
        assert_eq!(
            v,
            ["the gcc version of -v is 15.2.0 for G and 16.0.0 for R"]
        );
    }

    #[test]
    fn records_give_each_probe_once_with_its_output_moved() {
        let line = |argv: &str, probe: bool, slot: &str| {
            let argv: Vec<String> = argv.split(' ').map(String::from).collect();
            let mut r = CompileRecord::new(argv, "cc".to_string(), "/obj".to_string());
            r.probe = probe;
            r.slot = Some(slot.to_string());
            r
        };
        let records = vec![
            line("cc --version", true, "cc"),
            line("cc --version", true, "cc"),
            line("cc -c a.c -o /tmp/x/a.o", true, "cc"),
            line("cc -c b.c -o b.o", false, "cc"),
            line("cpp -dM", true, "cpp"),
        ];
        let (probes, other) = from_records(&records);
        assert_eq!(probes.len(), 2);
        assert_eq!(other, 2);
        assert_eq!(probes[0].reads, Reads::Identity);
        assert_eq!(probes[1].args, ["-c", "a.c", "-o", "rb-probe.out"]);
    }
}
