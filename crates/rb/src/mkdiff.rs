//! `rb mk-diff`: the make variables a build computes from its compiler's answers, read in R's and
//! G's configuration, and compared (plan 05.5 and 02.2).
//!
//! Each side is a toolchain description or a dump that an earlier run wrote. For FreeBSD the
//! description is a `CROSS_TOOLCHAIN` file, and rb runs the host's `make` with it. For NetBSD it is
//! a `TOOLDIR`, and rb runs its `nbmake-MACHINE` wrapper, which `build.sh` writes on its first run
//! with that `TOOLDIR`. rb reads the variables at these points:
//! - `top`: the top of the tree. FreeBSD reads `Makefile.inc1` there, which is where the `X_`
//!   variables are computed, and its `make showconfig` gives the `showconfig` point.
//! - `lib` and `prog`: `lib/libc` and `bin/sh`. FreeBSD gets `CC`, `CXX` and `CPP` from the
//!   `CROSS_TOOLCHAIN` file there, as `buildworld` passes them down.
//! - `stand`: `stand/libsa`, FreeBSD only.
//! - `kernel`: a kernel build directory, when one is given. Its `opt_*.h` files are read too, and
//!   each one is a point of its own.
//!
//! Each value has the side's toolchain directory replaced by `<toolchain>`, the side's FreeBSD
//! `MAKEOBJDIRPREFIX` replaced by `<obj>`, and the shim names `rb-cc-ref` and `rb-cc-rucc` replaced
//! by `rb-cc`, so that the paths rb gives each side are not a difference.

use crate::divergences::Divergences;
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The first word of a dump file.
pub const DUMP_HEADER: &str = "# rb mk-diff";

/// The variables FreeBSD computes from the compiler and the linker, without and with `X_`.
pub const FREEBSD_TOOLCHAIN: &[&str] = &[
    "COMPILER_TYPE",
    "COMPILER_VERSION",
    "COMPILER_FEATURES",
    "COMPILER_FREEBSD_VERSION",
    "COMPILER_RESOURCE_DIR",
    "COMPILER_ABSOLUTE_PATH",
    "LINKER_TYPE",
    "LINKER_VERSION",
    "LINKER_FEATURES",
    "LINKER_FREEBSD_VERSION",
];

/// Other FreeBSD variables that depend on the compiler at every point.
pub const FREEBSD_OTHER: &[&str] = &["CC", "CFLAGS", "CWARNFLAGS", "CXXFLAGS", "NO_WERROR"];

/// The NetBSD variables that depend on the compiler at every point. The `MK*`, `HAVE_*` and
/// `CC_WNO_*` names of `share/mk` are added at the top.
pub const NETBSD_VARS: &[&str] = &[
    "ACTIVE_CC",
    "ACTIVE_CXX",
    "ACTIVE_CPP",
    "HAVE_GCC",
    "HAVE_LLVM",
    "HAVE_LIBGCC",
    "HAVE_LIBGCC_EH",
    "MKGCC",
    "MKLIBSTDCXX",
    "CC",
    "CFLAGS",
    "CWARNFLAGS",
    "COPTS",
    "NOGCCERROR",
];

/// One side's answers: the value of each variable at each point.
pub type Dump = BTreeMap<(String, String), String>;

/// Write a dump. One line per variable: the point, the name and the value, split by tabs.
#[must_use]
pub fn render(label: &str, dump: &Dump) -> String {
    let mut out = format!("{DUMP_HEADER} {label}\n");
    for ((point, name), value) in dump {
        let _ = writeln!(out, "{point}\t{name}\t{value}");
    }
    out
}

/// Read a dump that [`render`] wrote.
pub fn parse(text: &str) -> Result<Dump, String> {
    let mut lines = text.lines();
    if !lines.next().is_some_and(|l| l.starts_with(DUMP_HEADER)) {
        return Err(format!("not a dump: the first line is not {DUMP_HEADER}"));
    }
    let mut dump = Dump::new();
    for (i, line) in lines.enumerate() {
        let mut parts = line.splitn(3, '\t');
        let (Some(point), Some(name), Some(value)) = (parts.next(), parts.next(), parts.next())
        else {
            return Err(format!("line {}: not point, name and value", i + 2));
        };
        dump.insert((point.into(), name.into()), value.into());
    }
    Ok(dump)
}

/// Whether a file is a dump.
#[must_use]
pub fn is_dump(path: &Path) -> bool {
    path.is_file() && std::fs::read_to_string(path).is_ok_and(|t| t.starts_with(DUMP_HEADER))
}

/// The knob names a NetBSD `share/mk` file uses: `MK*`, `HAVE_*` and `CC_WNO_*`.
#[must_use]
pub fn knobs(text: &str) -> Vec<String> {
    let mut out = std::collections::BTreeSet::new();
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let starts = i == 0 || !is_name(bytes[i - 1]);
        if starts && bytes[i].is_ascii_uppercase() {
            let end = (i..bytes.len())
                .find(|&j| !is_name(bytes[j]))
                .unwrap_or(bytes.len());
            let word = &text[i..end];
            let knob = (word.starts_with("MK") && word.len() > 2)
                || (word.starts_with("HAVE_") && word.len() > 5)
                || (word.starts_with("CC_WNO_") && word.len() > 7);
            if knob && !word.ends_with('_') {
                out.insert(word.to_string());
            }
            i = end;
        } else {
            i += 1;
        }
    }
    out.into_iter().collect()
}

fn is_name(b: u8) -> bool {
    b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_' || b.is_ascii_lowercase()
}

/// The knobs of FreeBSD's `make showconfig`: lines such as `MK_LIB32 = yes`.
#[must_use]
pub fn showconfig(text: &str) -> Vec<(String, String)> {
    text.lines()
        .filter_map(|l| {
            let (name, value) = l.split_once('=')?;
            let name = name.trim();
            let ok = (name.starts_with("MK_") || name.starts_with("OPT_"))
                && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_');
            ok.then(|| (name.to_string(), value.trim().to_string()))
        })
        .collect()
}

/// The macros of a kernel option header: each `#define NAME value` line.
#[must_use]
pub fn defines(text: &str) -> Vec<(String, String)> {
    text.lines()
        .filter_map(|l| {
            let rest = l.trim().strip_prefix("#define")?;
            let rest = rest.trim_start();
            let (name, value) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
            Some((name.to_string(), value.trim().to_string()))
        })
        .collect()
}

/// A value with the side's own names taken out.
#[must_use]
pub fn normalize(value: &str, toolchain: &str, obj: &str) -> String {
    let mut out = value.trim().to_string();
    if !obj.is_empty() {
        out = out.replace(obj, "<obj>");
    }
    if !toolchain.is_empty() {
        out = out.replace(toolchain, "<toolchain>");
    }
    out.replace("rb-cc-rucc", "rb-cc")
        .replace("rb-cc-ref", "rb-cc")
}

/// How one side runs make.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Maker {
    /// The system.
    pub system: String,
    /// The make command and the variables every point gets.
    pub argv: Vec<String>,
    /// The variables the points below the top get.
    pub below: Vec<String>,
    /// The environment.
    pub env: Vec<(String, String)>,
    /// The toolchain directory, which [`normalize`] takes out of values.
    pub toolchain: String,
    /// The object directory prefix, which [`normalize`] takes out of values.
    pub obj: String,
}

/// What a FreeBSD build needs besides the `CROSS_TOOLCHAIN` file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FreebsdPlace<'a> {
    /// `TARGET`.
    pub target: &'a str,
    /// `TARGET_ARCH`.
    pub target_arch: &'a str,
    /// `SRCCONF`.
    pub srcconf: &'a Path,
    /// `MAKEOBJDIRPREFIX`.
    pub obj: &'a Path,
}

impl Maker {
    /// FreeBSD's host `make` with a `CROSS_TOOLCHAIN` file.
    pub fn freebsd(mk: &Path, src: &Path, place: &FreebsdPlace<'_>) -> Result<Self, String> {
        let text = std::fs::read_to_string(mk).map_err(|e| format!("{}: {e}", mk.display()))?;
        let shims = std::env::var("RB_SHIMS").ok();
        let value = |key: &str| {
            crate::toolchain::assignments(&text)
                .into_iter()
                .find(|(k, _)| k == key)
                .map(|(_, v)| {
                    let v = shims
                        .as_deref()
                        .map_or(v.clone(), |s| v.replace("${RB_SHIMS}", s));
                    crate::toolchain::expand(&text, &v)
                })
        };
        let mut below = vec![
            format!("MACHINE={}", place.target),
            format!("MACHINE_ARCH={}", place.target_arch),
        ];
        for (from, to) in [
            ("XCC", "CC"),
            ("XCXX", "CXX"),
            ("XCPP", "CPP"),
            ("XLD", "LD"),
        ] {
            if let Some(v) = value(from) {
                below.push(format!("{to}={v}"));
            }
        }
        let cc = value("XCC").ok_or_else(|| format!("{} sets no XCC", mk.display()))?;
        let toolchain = Path::new(&cc)
            .parent()
            .and_then(Path::parent)
            .map(|p| p.display().to_string())
            .unwrap_or_default();
        let obj = absolute(place.obj).display().to_string();
        Ok(Self {
            system: "freebsd".into(),
            argv: vec![
                "make".into(),
                "-m".into(),
                src.join("share/mk").display().to_string(),
                format!("TARGET={}", place.target),
                format!("TARGET_ARCH={}", place.target_arch),
                format!("CROSS_TOOLCHAIN={}", absolute(mk).display()),
                "__MAKE_CONF=/dev/null".into(),
                format!("SRCCONF={}", place.srcconf.display()),
            ],
            below,
            env: vec![("MAKEOBJDIRPREFIX".into(), obj.clone())],
            toolchain,
            obj,
        })
    }

    /// NetBSD's `nbmake-MACHINE` in a `TOOLDIR`.
    pub fn netbsd(tooldir: &Path, machine: &str) -> Result<Self, String> {
        let wrapper = tooldir.join(format!("bin/nbmake-{machine}"));
        if !wrapper.is_file() {
            return Err(format!(
                "{} is missing; build.sh writes it on its first run with -T {}",
                wrapper.display(),
                tooldir.display()
            ));
        }
        Ok(Self {
            system: "netbsd".into(),
            argv: vec![wrapper.display().to_string()],
            below: Vec::new(),
            env: Vec::new(),
            toolchain: absolute(tooldir).display().to_string(),
            obj: String::new(),
        })
    }

    /// The command that prints `names` in `dir`, one line each. `top` says whether `dir` is the
    /// top of the tree.
    #[must_use]
    pub fn command(&self, dir: &Path, top: bool, names: &[String]) -> Vec<String> {
        let mut argv = self.argv.clone();
        argv.push("-C".into());
        argv.push(dir.display().to_string());
        if top && self.system == "freebsd" {
            argv.push("-f".into());
            argv.push("Makefile.inc1".into());
        }
        if !top {
            argv.extend(self.below.iter().cloned());
        }
        for name in names {
            argv.push("-V".into());
            argv.push(format!("${{{name}}}"));
        }
        argv
    }

    fn run(&self, argv: &[String]) -> Result<String, String> {
        let out = Command::new(&argv[0])
            .args(&argv[1..])
            .envs(self.env.iter().map(|(k, v)| (k, v)))
            .output()
            .map_err(|e| format!("{}: {e}", argv[0]))?;
        if !out.status.success() {
            let err = String::from_utf8_lossy(&out.stderr);
            let first: Vec<&str> = err.lines().take(5).collect();
            return Err(format!("{} failed: {}", argv.join(" "), first.join(" / ")));
        }
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    }

    /// Read `names` at one point into `dump`.
    pub fn ask(
        &self,
        dump: &mut Dump,
        point: &str,
        dir: &Path,
        names: &[String],
    ) -> Result<(), String> {
        let argv = self.command(dir, point == "top", names);
        let text = self.run(&argv)?;
        let lines: Vec<&str> = text.lines().collect();
        if lines.len() != names.len() {
            return Err(format!(
                "make printed {} lines for {} variables in {}",
                lines.len(),
                names.len(),
                dir.display()
            ));
        }
        for (name, value) in names.iter().zip(lines) {
            dump.insert(
                (point.to_string(), name.clone()),
                normalize(value, &self.toolchain, &self.obj),
            );
        }
        Ok(())
    }

    /// Read every point.
    pub fn collect(&self, src: &Path, kernel: Option<&Path>) -> Result<Dump, String> {
        let mut dump = Dump::new();
        let mut every: Vec<String> = Vec::new();
        let mut top: Vec<String> = Vec::new();
        let mut places: Vec<(&str, PathBuf)> = vec![
            ("top", src.to_path_buf()),
            ("lib", src.join("lib/libc")),
            ("prog", src.join("bin/sh")),
        ];
        if self.system == "freebsd" {
            for name in FREEBSD_TOOLCHAIN {
                every.push((*name).to_string());
                every.push(format!("X_{name}"));
            }
            every.extend(FREEBSD_OTHER.iter().map(|s| (*s).to_string()));
            places.push(("stand", src.join("stand/libsa")));
        } else {
            every.extend(NETBSD_VARS.iter().map(|s| (*s).to_string()));
            for file in ["share/mk/bsd.own.mk", "share/mk/bsd.sys.mk"] {
                let path = src.join(file);
                let text = std::fs::read_to_string(&path)
                    .map_err(|e| format!("{}: {e}", path.display()))?;
                top.extend(knobs(&text));
            }
            top.retain(|k| !every.contains(k));
            top.sort();
            top.dedup();
        }
        if let Some(k) = kernel {
            places.push(("kernel", k.to_path_buf()));
        }
        for (point, dir) in &places {
            let mut names = every.clone();
            if *point == "top" {
                names.extend(top.iter().cloned());
            }
            self.ask(&mut dump, point, dir, &names)?;
        }
        if self.system == "freebsd" {
            let mut argv = self.argv.clone();
            argv.extend(["-C".into(), src.display().to_string(), "showconfig".into()]);
            for (name, value) in showconfig(&self.run(&argv)?) {
                dump.insert(("showconfig".into(), name), value);
            }
        }
        if let Some(k) = kernel {
            read_options(&mut dump, k)?;
        }
        Ok(dump)
    }
}

/// Add the `opt_*.h` files of a kernel build directory.
pub fn read_options(dump: &mut Dump, dir: &Path) -> Result<(), String> {
    let entries = std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !(name.starts_with("opt_") && Path::new(&name).extension().is_some_and(|e| e == "h")) {
            continue;
        }
        let text = std::fs::read_to_string(entry.path()).unwrap_or_default();
        dump.insert((name.clone(), "(file)".into()), "present".into());
        for (macro_name, value) in defines(&text) {
            dump.insert((name.clone(), macro_name), value);
        }
    }
    Ok(())
}

fn absolute(path: &Path) -> PathBuf {
    std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf())
}

/// One difference, with every point it shows at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Difference {
    /// The variable or macro.
    pub name: String,
    /// G's value, or `None` when G does not have it.
    pub g: Option<String>,
    /// R's value, likewise.
    pub r: Option<String>,
    /// The points.
    pub points: Vec<String>,
    /// The reason from the divergence file, when an entry allows it.
    pub reason: Option<String>,
}

/// Compare two dumps. A variable that differs the same way at several points is one difference,
/// because it has one cause.
#[must_use]
pub fn compare(g: &Dump, r: &Dump, system: &str, divergences: &Divergences) -> Vec<Difference> {
    type Cause = (String, Option<String>, Option<String>, Option<String>);
    let mut grouped: BTreeMap<Cause, Vec<String>> = BTreeMap::new();
    let keys: std::collections::BTreeSet<&(String, String)> = g.keys().chain(r.keys()).collect();
    for key in keys {
        let (a, b) = (g.get(key), r.get(key));
        if a == b {
            continue;
        }
        let (point, name) = key;
        let reason = divergences
            .variable(system, point, name)
            .map(str::to_string);
        grouped
            .entry((name.clone(), a.cloned(), b.cloned(), reason))
            .or_default()
            .push(point.clone());
    }
    grouped
        .into_iter()
        .map(|((name, g, r, reason), points)| Difference {
            name,
            g,
            r,
            points,
            reason,
        })
        .collect()
}

/// The report, and how many differences count.
#[must_use]
pub fn report(diffs: &[Difference], g_size: usize, r_size: usize) -> (String, usize) {
    let counted = diffs.iter().filter(|d| d.reason.is_none()).count();
    let mut s = format!(
        "{g_size} values from G, {r_size} from R: {counted} differences, {} allowed by mk-divergences.toml\n",
        diffs.len() - counted
    );
    let show = |v: &Option<String>| {
        v.as_deref()
            .map_or("(not set)".to_string(), |v| format!("'{v}'"))
    };
    for d in diffs.iter().filter(|d| d.reason.is_none()) {
        let _ = writeln!(s, "\n{} at {}:", d.name, d.points.join(", "));
        let _ = writeln!(s, "  G {}", show(&d.g));
        let _ = writeln!(s, "  R {}", show(&d.r));
    }
    let allowed: Vec<&Difference> = diffs.iter().filter(|d| d.reason.is_some()).collect();
    if !allowed.is_empty() {
        s.push_str("\nallowed:\n");
        for d in allowed {
            let _ = writeln!(
                s,
                "  {} at {}: {}",
                d.name,
                d.points.join(", "),
                d.reason.as_deref().unwrap_or_default()
            );
        }
    }
    (s, counted)
}

/// The command line half.
pub mod cli {
    use super::{Dump, FreebsdPlace, Maker, compare, is_dump, parse, render, report};
    use crate::cli::Args;
    use crate::divergences::Divergences;
    use crate::toolchain::cli::Files;
    use crate::toolchain::{REF_MK, RUCC_MK};
    use std::path::{Path, PathBuf};

    /// One side: where it comes from, and its label.
    fn side(
        given: &Path,
        label: &str,
        files: &Files,
        args: &Args,
        root: &Path,
        work: &Path,
    ) -> Result<Dump, String> {
        if is_dump(given) {
            let text =
                std::fs::read_to_string(given).map_err(|e| format!("{}: {e}", given.display()))?;
            return parse(&text).map_err(|e| format!("{}: {e}", given.display()));
        }
        let src = PathBuf::from(
            args.get("src")
                .ok_or("give --src, the tree make reads, or a dump for each side")?,
        );
        let kernel: Vec<&str> = args
            .get("kernel")
            .map_or(Vec::new(), |k| k.split(',').collect());
        let kernel = match (label, kernel.as_slice()) {
            (_, []) => None,
            (_, [one]) | ("G", [one, _]) | ("R", [_, one]) => Some(PathBuf::from(one)),
            _ => return Err("--kernel takes DIR or G,R".into()),
        };
        let maker = if files.pin.system == "freebsd" {
            let conf = args.get("conf").unwrap_or("graded");
            let srcconf = root.join(format!("conf/freebsd/{conf}.conf"));
            let obj = work.join(format!("obj-{label}"));
            let names = &files.row.freebsd;
            Maker::freebsd(
                given,
                &src,
                &FreebsdPlace {
                    target: names.target.as_deref().unwrap_or("amd64"),
                    target_arch: names.target_arch.as_deref().unwrap_or("amd64"),
                    srcconf: &srcconf,
                    obj: &obj,
                },
            )?
        } else {
            Maker::netbsd(given, &files.machine())?
        };
        let dump = maker.collect(&src, kernel.as_deref())?;
        let file = work.join(format!("mk-{label}.txt"));
        std::fs::create_dir_all(work).map_err(|e| format!("{}: {e}", work.display()))?;
        std::fs::write(&file, render(&given.display().to_string(), &dump))
            .map_err(|e| format!("{}: {e}", file.display()))?;
        Ok(dump)
    }

    /// Run `rb mk-diff`. Returns the report and whether no difference counted.
    pub fn run(args: &Args, root: &Path) -> Result<(String, bool), String> {
        let files = Files::load(root, args.target.as_deref(), args.get("row"))?;
        let (g, r) = match (args.get("dir"), args.get("g"), args.get("r")) {
            (Some(d), None, None) => (Path::new(d).join(REF_MK), Some(Path::new(d).join(RUCC_MK))),
            (None, Some(g), r) => (PathBuf::from(g), r.map(PathBuf::from)),
            _ => return Err("give --dir DIR, or --g with or without --r".into()),
        };
        let work = PathBuf::from(args.get("work").unwrap_or("mk-diff"));
        let g_dump = side(&g, "G", &files, args, root, &work)?;
        let Some(r) = r else {
            return Ok((
                format!(
                    "{} values from G, written to {}\n",
                    g_dump.len(),
                    work.join("mk-G.txt").display()
                ),
                true,
            ));
        };
        let r_dump = side(&r, "R", &files, args, root, &work)?;
        let divergences = Divergences::load(root, args.get("divergences").map(Path::new))?;
        let diffs = compare(&g_dump, &r_dump, &files.pin.system, &divergences);
        let (text, counted) = report(&diffs, g_dump.len(), r_dump.len());
        Ok((
            format!("G: {}\nR: {}\n{text}", g.display(), r.display()),
            counted == 0,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dumps_read_back() {
        let mut d = Dump::new();
        d.insert(("top".into(), "CC".into()), "<toolchain>/bin/cc -O2".into());
        d.insert(("lib".into(), "CFLAGS".into()), String::new());
        let text = render("G", &d);
        assert!(text.starts_with("# rb mk-diff G\n"));
        assert_eq!(parse(&text).unwrap(), d);
        assert!(parse("top\tCC\tx\n").is_err());
    }

    #[test]
    fn knobs_come_from_the_share_mk_text() {
        let text = "MKGCC?=\tyes\n.if ${HAVE_GCC} > 10\nCC_WNO_ARRAY_BOUNDS=-Wno-array-bounds\n\
                    .for var in MKFOO MK_ HAVE_ XMKBAR\n_MKVARS.yes= MKDOC\n";
        assert_eq!(
            knobs(text),
            ["CC_WNO_ARRAY_BOUNDS", "HAVE_GCC", "MKDOC", "MKFOO", "MKGCC"]
        );
    }

    #[test]
    fn showconfig_and_option_headers_parse() {
        let text = "MK_ACCT                        = yes\nMK_LIB32 = no\nOPT_FOO = 1\nwarning: x\n";
        assert_eq!(
            showconfig(text),
            [
                ("MK_ACCT".to_string(), "yes".to_string()),
                ("MK_LIB32".into(), "no".into()),
                ("OPT_FOO".into(), "1".into())
            ]
        );
        let opt = "#define INET 1\n#define\tCOMPAT_FREEBSD14 \n/* option `X' not defined */\n";
        assert_eq!(
            defines(opt),
            [
                ("INET".to_string(), "1".to_string()),
                ("COMPAT_FREEBSD14".into(), String::new())
            ]
        );
    }

    #[test]
    fn values_lose_the_side_s_own_names() {
        assert_eq!(
            normalize(
                " /r/tools/bin/x86_64-unknown-freebsd15.1-rb-cc-rucc ",
                "/r/tools",
                ""
            ),
            "<toolchain>/bin/x86_64-unknown-freebsd15.1-rb-cc"
        );
        assert_eq!(
            normalize(
                "-ffile-prefix-map=/w/obj-R/src=/usr/obj -g",
                "/r/tools",
                "/w/obj-R"
            ),
            "-ffile-prefix-map=<obj>/src=/usr/obj -g"
        );
        assert_eq!(normalize("gcc", "", ""), "gcc");
    }

    #[test]
    fn freebsd_commands_are_those_of_buildworld() {
        let dir = std::env::temp_dir().join(format!("rb-mk-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mk = dir.join("ref.mk");
        std::fs::write(
            &mk,
            "RB_SHIMS?=/t/bin\nXCC=${RB_SHIMS}/x-rb-cc-ref\nXCPP=${RB_SHIMS}/x-rb-cpp\n",
        )
        .unwrap();
        let place = FreebsdPlace {
            target: "amd64",
            target_arch: "amd64",
            srcconf: Path::new("/r/conf/freebsd/graded.conf"),
            obj: Path::new("/w/obj"),
        };
        let m = Maker::freebsd(&mk, Path::new("/src"), &place).unwrap();
        assert_eq!(m.toolchain, "/t");
        assert_eq!(m.obj, "/w/obj");
        assert_eq!(
            m.env,
            vec![("MAKEOBJDIRPREFIX".to_string(), "/w/obj".to_string())]
        );
        let names = vec!["X_COMPILER_TYPE".to_string()];
        let top = m.command(Path::new("/src"), true, &names).join(" ");
        assert_eq!(
            top,
            format!(
                "make -m /src/share/mk TARGET=amd64 TARGET_ARCH=amd64 CROSS_TOOLCHAIN={} __MAKE_CONF=/dev/null SRCCONF=/r/conf/freebsd/graded.conf -C /src -f Makefile.inc1 -V ${{X_COMPILER_TYPE}}",
                mk.display()
            )
        );
        let lib = m
            .command(Path::new("/src/lib/libc"), false, &names)
            .join(" ");
        assert!(
            lib.ends_with(
                "-C /src/lib/libc MACHINE=amd64 MACHINE_ARCH=amd64 CC=/t/bin/x-rb-cc-ref CPP=/t/bin/x-rb-cpp -V ${X_COMPILER_TYPE}"
            ),
            "{lib}"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_netbsd_tooldir_needs_its_wrapper() {
        let err = Maker::netbsd(Path::new("/nonexistent"), "amd64").unwrap_err();
        assert!(err.contains("nbmake-amd64"), "{err}");
    }

    #[test]
    fn differences_are_grouped_by_cause() {
        let mut g = Dump::new();
        let mut r = Dump::new();
        for point in ["top", "lib", "prog"] {
            g.insert((point.into(), "HAVE_GCC".into()), "15".into());
            r.insert((point.into(), "HAVE_GCC".into()), "16".into());
            g.insert((point.into(), "CC".into()), "x".into());
            r.insert((point.into(), "CC".into()), "x".into());
        }
        g.insert(
            ("top".into(), "X_COMPILER_RESOURCE_DIR".into()),
            "/g".into(),
        );
        r.insert(
            ("top".into(), "X_COMPILER_RESOURCE_DIR".into()),
            "/r".into(),
        );
        r.insert(("opt_x.h".into(), "NEW".into()), "1".into());
        let d = Divergences::parse(
            "[[divergence]]\nvariable = \"*COMPILER_RESOURCE_DIR\"\nreason = \"own directory\"\n",
        )
        .unwrap();
        let diffs = compare(&g, &r, "netbsd", &d);
        assert_eq!(diffs.len(), 3);
        let have = diffs.iter().find(|d| d.name == "HAVE_GCC").unwrap();
        assert_eq!(have.points, ["lib", "prog", "top"]);
        let (text, counted) = report(&diffs, g.len(), r.len());
        assert_eq!(counted, 2);
        assert!(
            text.contains("NEW at opt_x.h:\n  G (not set)\n  R '1'"),
            "{text}"
        );
        assert!(
            text.contains("X_COMPILER_RESOURCE_DIR at top: own directory"),
            "{text}"
        );
    }
}
