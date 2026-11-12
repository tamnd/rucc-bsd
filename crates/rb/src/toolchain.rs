//! `rb toolchain` and `rb toolchain-diff`: the toolchain descriptions that put rucc in a build,
//! and the check that R's differs from G's in the C compiler and nothing else (plan 05.4).
//!
//! FreeBSD reads a `CROSS_TOOLCHAIN` file. `rb toolchain` writes G's `ref.mk` and R's `rucc.mk`
//! next to a directory of shims, and the two files differ in the `XCC` line only.
//!
//! NetBSD reads `TOOLDIR`. `rb toolchain` copies the `TOOLDIR` that `build.sh tools` made and puts
//! the shim in place of its C compiler and its preprocessor, with `rb-shim.toml` beside them. G's
//! copy and R's copy differ in the side that `rb-shim.toml` names, and nothing else. Every other
//! file is the same, byte for byte, as in the `TOOLDIR` of `build.sh tools`.

use crate::shims::{self, Shims};
use rb_pins::fetch::sha256;
use rb_pins::pins::Pin;
use rb_pins::places::Row;
use rb_pins::toolchains::{Personas, RuccPin, Toolchains};
use rb_shim::config::{FILE_NAME, ShimConfig, Side};
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

/// The name of G's `CROSS_TOOLCHAIN` file.
pub const REF_MK: &str = "ref.mk";
/// The name of R's `CROSS_TOOLCHAIN` file.
pub const RUCC_MK: &str = "rucc.mk";

/// A FreeBSD `CROSS_TOOLCHAIN` file for the shims in `bin`. `bin` may be a make expression, such
/// as `${RB_SHIMS}` in the files under `conf/freebsd`.
///
/// On aarch64 the file also turns off GCC's outline atomics for C and C++, on both sides. GCC
/// calls `__aarch64_ldadd8_acq_rel` and its family by default, FreeBSD's `libgcc_s` does not export
/// them, and `libcompiler_rt.a` has them hidden. So a library linked without `libgcc.a`, such as
/// `libc++.so`, keeps the call undefined, and the link of `zfsd_unittest` fails. FreeBSD's kernel
/// turns them off the same way (`sys/conf/kern.mk:150`), and rucc has none.
#[must_use]
pub fn freebsd_mk(bin: &str, target: &str, cross_prefix: &str, side: Side) -> String {
    let cc = match side {
        Side::R => "rb-cc-rucc",
        Side::G => "rb-cc-ref",
    };
    let mut text = format!(
        "XCC={bin}/{target}-{cc}\n\
         XCXX={bin}/{target}-rb-c++\n\
         XCPP={bin}/{target}-rb-cpp\n\
         CROSS_BINUTILS_PREFIX={cross_prefix}\n\
         X_COMPILER_TYPE=gcc\n"
    );
    if target.starts_with("aarch64-") {
        text.push_str("XCFLAGS=-mno-outline-atomics\n");
    }
    text
}

/// The `CROSS_BINUTILS_PREFIX` of a reference toolchain or of what it needs.
pub fn cross_prefix(toolchains: &Toolchains, reference: &str) -> Result<String, String> {
    let t = toolchains
        .toolchain
        .get(reference)
        .ok_or_else(|| format!("toolchains.toml has no {reference}"))?;
    t.cross_prefix
        .clone()
        .or_else(|| {
            t.needs.iter().find_map(|n| {
                toolchains
                    .toolchain
                    .get(n)
                    .and_then(|d| d.cross_prefix.clone())
            })
        })
        .ok_or_else(|| format!("{reference} and what it needs have no cross_prefix"))
}

/// One check of `rb toolchain-diff`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Check {
    /// What was checked.
    pub what: String,
    /// Whether it held.
    pub ok: bool,
    /// What was found, when it did not hold.
    pub detail: Vec<String>,
}

impl Check {
    fn new(what: impl Into<String>, detail: Vec<String>) -> Self {
        Self {
            what: what.into(),
            ok: detail.is_empty(),
            detail,
        }
    }
}

/// The checks, as text, and whether all held.
#[must_use]
pub fn report(checks: &[Check]) -> (String, bool) {
    let mut s = String::new();
    for c in checks {
        let _ = writeln!(s, "{} {}", if c.ok { "ok  " } else { "FAIL" }, c.what);
        for d in &c.detail {
            let _ = writeln!(s, "       {d}");
        }
    }
    let ok = checks.iter().all(|c| c.ok);
    let failed = checks.iter().filter(|c| !c.ok).count();
    let _ = writeln!(
        s,
        "{}",
        if ok {
            "toolchain-diff: R and G differ in the C compiler only".to_string()
        } else {
            format!("toolchain-diff: {failed} of {} checks failed", checks.len())
        }
    );
    (s, ok)
}

/// What the repository's files say a pin's shims must be.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Expected {
    /// rucc's arguments.
    pub rucc_args: Vec<String>,
    /// rucc's `--target`.
    pub target: String,
    /// The reference compilers, for FreeBSD: cc, c++, cpp and as.
    pub reference: Option<[PathBuf; 4]>,
    /// FreeBSD's `CROSS_BINUTILS_PREFIX`.
    pub cross_prefix: Option<String>,
}

impl Expected {
    /// Read them from the repository's files.
    pub fn of(
        pin: &Pin,
        row: &Row,
        personas: &Personas,
        toolchains: &Toolchains,
    ) -> Result<Self, String> {
        let persona = personas
            .for_pin(pin)
            .ok_or_else(|| format!("personas.toml has no persona for {}", pin.name))?;
        let arch = row.rucc_arch.as_str();
        let mut expected = Self {
            rucc_args: persona.rucc_args(pin, arch),
            target: persona.target(pin, arch),
            reference: None,
            cross_prefix: None,
        };
        if pin.system == "freebsd" {
            let name = persona.reference_for(row, toolchains);
            let t = toolchains
                .toolchain
                .get(&name)
                .ok_or_else(|| format!("toolchains.toml has no {name}"))?;
            let prefix = cross_prefix(toolchains, &name)?;
            let path = |v: &Option<String>| PathBuf::from(v.clone().unwrap_or_default());
            expected.reference = Some([
                path(&t.cc),
                path(&t.cxx),
                path(&t.cpp),
                PathBuf::from(format!("{prefix}as")),
            ]);
            expected.cross_prefix = Some(prefix);
        }
        Ok(expected)
    }
}

/// Check one `rb-shim.toml` against the repository and the rucc pin. `side` is the side the
/// file must name, or `None` for FreeBSD's shared directory.
#[must_use]
pub fn check_config(
    label: &str,
    config: &ShimConfig,
    expected: &Expected,
    pin: Option<&RuccPin>,
    rucc_needed: bool,
    ungraded: bool,
) -> Vec<Check> {
    let mut checks = Vec::new();
    let mut wrong = Vec::new();
    if config.rucc_args != expected.rucc_args {
        wrong.push(format!(
            "rucc-args is {:?}, the persona gives {:?}",
            config.rucc_args, expected.rucc_args
        ));
    }
    checks.push(Check::new(
        format!(
            "{label}: rucc gets the persona's arguments {}",
            expected.rucc_args.join(" ")
        ),
        wrong,
    ));
    if let Some(reference) = &expected.reference {
        let found = [
            &config.ref_cc,
            &config.ref_cxx,
            &config.ref_cpp,
            &config.ref_as,
        ];
        let wrong: Vec<String> = ["ref-cc", "ref-cxx", "ref-cpp", "ref-as"]
            .iter()
            .zip(found.iter().zip(reference.iter()))
            .filter(|(_, (f, e))| f.as_path() != e.as_path())
            .map(|(k, (f, e))| {
                format!(
                    "{k} is {}, toolchains.toml gives {}",
                    f.display(),
                    e.display()
                )
            })
            .collect();
        checks.push(Check::new(
            format!("{label}: the reference compilers are those of toolchains.toml"),
            wrong,
        ));
    }
    if !config.bringup.is_empty() {
        checks.push(bringup_check(label, config, ungraded));
    }
    if rucc_needed {
        checks.push(rucc_check(label, &config.rucc, pin, ungraded));
    }
    checks
}

/// A bring-up class is on: the run is ungraded, so `--ungraded` must say the caller knows.
fn bringup_check(label: &str, config: &ShimConfig, ungraded: bool) -> Check {
    let classes = config.bringup.join(", ");
    let mut wrong = Vec::new();
    if let Err(e) = rb_shim::config::CLASSES.check(&config.bringup) {
        wrong.push(e);
    }
    if config.delegates("cxx-c") && config.bringup_units.is_empty() {
        wrong.push("cxx-c is on and bringup-units is empty".to_string());
    }
    if !ungraded {
        wrong.push(format!(
            "bringup is {classes}, which makes the run ungraded; pass --ungraded to build with it anyway"
        ));
        return Check::new(format!("{label}: no bring-up class is on"), wrong);
    }
    Check::new(
        format!("{label}: bring-up {classes} is on, so the run is ungraded; --ungraded allows it"),
        wrong,
    )
}

/// Whether the shims' rucc is the pinned release, or `--ungraded` allows another.
fn rucc_check(label: &str, rucc: &Path, pin: Option<&RuccPin>, ungraded: bool) -> Check {
    let what = format!("{label}: rucc is the pinned release");
    if rucc.as_os_str().is_empty() {
        return Check::new(what, vec!["rb-shim.toml names no rucc".into()]);
    }
    let digest = match sha256(rucc) {
        Ok(d) => d,
        Err(e) => return Check::new(what, vec![e]),
    };
    match pin.and_then(|p| p.host_of(&digest)) {
        Some(_) => Check::new(what, Vec::new()),
        None if ungraded => Check::new(
            format!(
                "{label}: rucc {} (sha256 {digest}) is not the pinned release; --ungraded allows it",
                rucc.display()
            ),
            Vec::new(),
        ),
        None => Check::new(
            what,
            vec![format!(
                "{} has sha256 {digest}, which is not in [rucc.binary_sha256]; pass --ungraded to build with it anyway",
                rucc.display()
            )],
        ),
    }
}

/// Read the lines of a make file as `NAME=value` pairs, in order. Comments and blank lines and
/// `?=` defaults are left out.
pub(crate) fn assignments(text: &str) -> Vec<(String, String)> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .filter_map(|l| {
            let (k, v) = l.split_once('=')?;
            if k.ends_with('?') {
                return None;
            }
            Some((k.trim().to_string(), v.trim().to_string()))
        })
        .collect()
}

/// Expand `${NAME}` from the `NAME?=value` lines of the same file.
pub(crate) fn expand(text: &str, value: &str) -> String {
    let mut out = value.to_string();
    for line in text.lines() {
        if let Some((k, v)) = line.split_once("?=") {
            out = out.replace(&format!("${{{}}}", k.trim()), v.trim());
        }
    }
    out
}

/// The FreeBSD checks: G's and R's `CROSS_TOOLCHAIN` files, and the shim directory they name.
pub fn diff_freebsd(
    g: &Path,
    r: &Path,
    expected: &Expected,
    pin: Option<&RuccPin>,
    rucc_needed: bool,
    ungraded: bool,
) -> Result<Vec<Check>, String> {
    let read = |p: &Path| std::fs::read_to_string(p).map_err(|e| format!("{}: {e}", p.display()));
    let (g_text, r_text) = (read(g)?, read(r)?);
    let (ga, ra) = (assignments(&g_text), assignments(&r_text));
    let mut checks = Vec::new();

    let g_map: BTreeMap<_, _> = ga.iter().cloned().collect();
    let r_map: BTreeMap<_, _> = ra.iter().cloned().collect();
    let mut wrong = Vec::new();
    for key in g_map
        .keys()
        .chain(r_map.keys())
        .collect::<std::collections::BTreeSet<_>>()
    {
        let (gv, rv) = (g_map.get(key), r_map.get(key));
        if key == "XCC" {
            continue;
        }
        if gv != rv {
            wrong.push(format!(
                "{key}: G has {}, R has {}",
                gv.map_or("nothing", String::as_str),
                rv.map_or("nothing", String::as_str)
            ));
        }
    }
    let g_cc = g_map.get("XCC").cloned().unwrap_or_default();
    let r_cc = r_map.get("XCC").cloned().unwrap_or_default();
    if !g_cc.ends_with("-rb-cc-ref") || r_cc != g_cc.replace("-rb-cc-ref", "-rb-cc-rucc") {
        wrong.push(format!(
            "XCC: G has {g_cc} and R has {r_cc}; they must be the -rb-cc-ref and -rb-cc-rucc names of one shim directory"
        ));
    }
    checks.push(Check::new(
        format!("{} and {} differ in XCC only", g.display(), r.display()),
        wrong,
    ));

    let mut wrong = Vec::new();
    if g_map.get("X_COMPILER_TYPE").map(String::as_str) != Some("gcc") {
        wrong.push("X_COMPILER_TYPE is not gcc".to_string());
    }
    if let Some(prefix) = &expected.cross_prefix
        && g_map.get("CROSS_BINUTILS_PREFIX") != Some(prefix)
    {
        wrong.push(format!(
            "CROSS_BINUTILS_PREFIX is {}, toolchains.toml gives {prefix}",
            g_map
                .get("CROSS_BINUTILS_PREFIX")
                .map_or("nothing", String::as_str)
        ));
    }
    for key in ["XCC", "XCXX", "XCPP"] {
        let v = g_map
            .get(key)
            .map(|v| expand(&g_text, v))
            .unwrap_or_default();
        let name = Path::new(&v)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        if !name.starts_with(&format!("{}-rb-", expected.target)) {
            wrong.push(format!(
                "{key} is {v}, not a shim named for {}",
                expected.target
            ));
        }
    }
    checks.push(Check::new(
        "the compiler type, the binutils and the shim names are those of toolchains.toml",
        wrong,
    ));

    let xcc = expand(&g_text, &g_cc);
    let dir = Path::new(&xcc).parent().unwrap_or(Path::new("."));
    let file = dir.join(FILE_NAME);
    let config = load_config(&file)?;
    checks.extend(check_config(
        &file.display().to_string(),
        &config,
        expected,
        pin,
        rucc_needed,
        ungraded,
    ));
    Ok(checks)
}

fn load_config(file: &Path) -> Result<ShimConfig, String> {
    let text = std::fs::read_to_string(file).map_err(|e| format!("{}: {e}", file.display()))?;
    toml::from_str(&text).map_err(|e| format!("{}: {e}", file.display()))
}

/// Every file under `dir` by its path relative to `dir`, with its SHA-256, or the target of a
/// symbolic link. `skip` names files left out, such as `nbmake-amd64`.
pub fn hashes(dir: &Path, skip: &dyn Fn(&str) -> bool) -> Result<BTreeMap<String, String>, String> {
    let mut out = BTreeMap::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let entries = std::fs::read_dir(&d).map_err(|e| format!("{}: {e}", d.display()))?;
        for entry in entries {
            let entry = entry.map_err(|e| format!("{}: {e}", d.display()))?;
            let path = entry.path();
            let rel = path
                .strip_prefix(dir)
                .unwrap_or(&path)
                .to_string_lossy()
                .into_owned();
            let kind = entry
                .file_type()
                .map_err(|e| format!("{}: {e}", path.display()))?;
            if kind.is_dir() {
                stack.push(path);
            } else if skip(&rel) {
            } else if kind.is_symlink() {
                let target =
                    std::fs::read_link(&path).map_err(|e| format!("{}: {e}", path.display()))?;
                out.insert(rel, format!("-> {}", target.display()));
            } else {
                out.insert(rel, sha256(&path)?);
            }
        }
    }
    Ok(out)
}

/// The names of the files that differ between two maps of [`hashes`].
#[must_use]
pub fn differing(a: &BTreeMap<String, String>, b: &BTreeMap<String, String>) -> Vec<String> {
    let mut out: Vec<String> = a
        .keys()
        .chain(b.keys())
        .filter(|k| a.get(*k) != b.get(*k))
        .cloned()
        .collect();
    out.sort();
    out.dedup();
    out
}

/// The NetBSD checks: each `TOOLDIR` against the `TOOLDIR` of `build.sh tools`, and G's against
/// R's when both are given.
pub fn diff_netbsd(
    tooldirs: &[(Side, &Path)],
    expected: &Expected,
    machine: &str,
    pin: Option<&RuccPin>,
    ungraded: bool,
) -> Result<Vec<Check>, String> {
    let skip = |rel: &str| {
        Path::new(rel).file_name().is_some_and(|n| {
            n.to_string_lossy()
                .starts_with(&format!("nbmake-{machine}"))
        })
    };
    let mut checks = Vec::new();
    let mut configs = Vec::new();
    for &(side, dir) in tooldirs {
        let label = format!("{} TOOLDIR {}", side.letter(), dir.display());
        let config = load_config(&dir.join("bin").join(FILE_NAME))?;
        let names: Vec<String> = config.names.keys().map(|n| format!("bin/{n}")).collect();
        if config.side != Some(side) {
            checks.push(Check::new(
                format!("{label}: rb-shim.toml names side {}", side.letter()),
                vec![format!("it names {:?}", config.side.map(Side::letter))],
            ));
        }
        // The reference compiler is in the TOOLDIR of build.sh tools: <tools>/bin/<name>.
        let tools = config
            .ref_cc
            .parent()
            .and_then(Path::parent)
            .ok_or_else(|| {
                format!(
                    "{label}: ref-cc {} is not in a TOOLDIR",
                    config.ref_cc.display()
                )
            })?
            .to_path_buf();
        let theirs = hashes(&tools, &skip)?;
        let ours = hashes(dir, &skip)?;
        let mut allowed = names.clone();
        allowed.push(format!("bin/{FILE_NAME}"));
        allowed.sort();
        let found = differing(&theirs, &ours);
        let wrong: Vec<String> = found
            .iter()
            .filter(|f| !allowed.contains(f))
            .map(|f| format!("{f} differs from {}", tools.display()))
            .collect();
        checks.push(Check::new(
            format!(
                "{label}: only {} differ from {}",
                allowed.join(", "),
                tools.display()
            ),
            wrong,
        ));
        let shim_hashes: Vec<Option<&String>> = names.iter().map(|n| ours.get(n)).collect();
        let mut wrong = Vec::new();
        if names.len() != 2
            || shim_hashes.iter().any(Option::is_none)
            || shim_hashes.windows(2).any(|w| w[0] != w[1])
        {
            wrong.push(format!("the names {names:?} are not two links to one shim"));
        }
        if names.iter().any(|n| theirs.get(n) == ours.get(n)) {
            wrong.push("a compiler name is still the reference program".to_string());
        }
        checks.push(Check::new(
            format!("{label}: the C compiler and the preprocessor are the shim"),
            wrong,
        ));
        checks.extend(check_config(
            &label,
            &config,
            expected,
            pin,
            side == Side::R,
            ungraded,
        ));
        configs.push((side, config, ours));
    }
    if let [(_, g, g_files), (_, r, r_files)] = configs.as_slice() {
        let mut wrong: Vec<String> = differing(g_files, r_files)
            .into_iter()
            .filter(|f| *f != format!("bin/{FILE_NAME}"))
            .map(|f| format!("{f} differs"))
            .collect();
        // G never delegates, so only R's file carries bring-up.
        let same = |c: &ShimConfig| ShimConfig {
            side: None,
            log: PathBuf::new(),
            bringup: Vec::new(),
            bringup_units: Vec::new(),
            ..c.clone()
        };
        if same(g) != same(r) {
            wrong.push(
                "the two rb-shim.toml files differ in more than the side, the log and bring-up"
                    .into(),
            );
        }
        checks.push(Check::new(
            "G's and R's TOOLDIRs differ in the side of rb-shim.toml only",
            wrong,
        ));
    }
    Ok(checks)
}

/// What `rb toolchain` makes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Made {
    /// The text to print.
    pub report: String,
    /// Whether every check held.
    pub ok: bool,
}

/// FreeBSD: the shims in `dir/bin`, and `dir/ref.mk` and `dir/rucc.mk`.
pub fn make_freebsd(
    dir: &Path,
    binary: &Path,
    shims: &Shims,
    toolchains: &Toolchains,
) -> Result<(PathBuf, PathBuf), String> {
    let bin = dir.join("bin");
    shims::install(&bin, binary, shims)?;
    let prefix = cross_prefix(toolchains, &shims.reference)?;
    let bin_text = std::path::absolute(&bin)
        .unwrap_or(bin)
        .display()
        .to_string();
    let g = dir.join(REF_MK);
    let r = dir.join(RUCC_MK);
    for (path, side) in [(&g, Side::G), (&r, Side::R)] {
        std::fs::write(path, freebsd_mk(&bin_text, &shims.target, &prefix, side))
            .map_err(|e| format!("{}: {e}", path.display()))?;
    }
    Ok((g, r))
}

/// NetBSD: `dir` becomes a copy of `tools` with the shim as its C compiler and preprocessor.
pub fn make_netbsd(
    dir: &Path,
    tools: &Path,
    binary: &Path,
    shims: &Shims,
    machine: &str,
) -> Result<(), String> {
    if dir.exists() {
        std::fs::remove_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    if let Some(parent) = dir.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    let status = std::process::Command::new("cp")
        .arg("-a")
        .arg(tools)
        .arg(dir)
        .status()
        .map_err(|e| format!("cp: {e}"))?;
    if !status.success() {
        return Err(format!(
            "cp -a {} {} failed",
            tools.display(),
            dir.display()
        ));
    }
    // build.sh writes the nbmake wrapper again for this TOOLDIR. The copy names the old one.
    let bin = dir.join("bin");
    for entry in std::fs::read_dir(&bin).map_err(|e| format!("{}: {e}", bin.display()))? {
        let entry = entry.map_err(|e| e.to_string())?;
        if entry
            .file_name()
            .to_string_lossy()
            .starts_with(&format!("nbmake-{machine}"))
        {
            std::fs::remove_file(entry.path()).map_err(|e| e.to_string())?;
        }
    }
    shims::install(&bin, binary, shims)
}

/// `rb toolchain` and `rb toolchain-diff` on the command line.
pub mod cli {
    use super::{Expected, Made, diff_freebsd, diff_netbsd, make_freebsd, make_netbsd, report};
    use super::{REF_MK, RUCC_MK};
    use crate::cli::Args;
    use crate::shims::{self, Ask, Cc, Rucc};
    use rb_pins::pins::{Pin, PinsFile};
    use rb_pins::places::{Row, Rows};
    use rb_pins::toolchains::{Personas, Toolchains};
    use rb_shim::config::Side;
    use std::fmt::Write as _;
    use std::path::{Path, PathBuf};

    /// The repository's files for one pin and row.
    pub struct Files {
        /// The pin.
        pub pin: Pin,
        /// The row.
        pub row: Row,
        /// `personas.toml`.
        pub personas: Personas,
        /// `toolchains.toml`.
        pub toolchains: Toolchains,
    }

    impl Files {
        /// Read them.
        pub fn load(root: &Path, pin: Option<&str>, row: Option<&str>) -> Result<Self, String> {
            let name = pin.ok_or("name the pin, such as netbsd-trunk")?;
            let pins = PinsFile::load(root)?.ok_or("no pins.toml yet")?;
            let pin = pins
                .find(name)
                .ok_or_else(|| format!("pins.toml has no pin {name}"))?
                .clone();
            let rows = Rows::load(root)?;
            let row_name = row.unwrap_or("amd64");
            let row = rows
                .row
                .get(row_name)
                .ok_or_else(|| format!("rows.toml has no row {row_name}"))?
                .clone();
            Ok(Self {
                pin,
                row,
                personas: Personas::load(root)?,
                toolchains: Toolchains::load(root)?,
            })
        }

        /// What the shims must be.
        pub fn expected(&self) -> Result<Expected, String> {
            Expected::of(&self.pin, &self.row, &self.personas, &self.toolchains)
        }

        /// NetBSD's `MACHINE` for the row.
        #[must_use]
        pub fn machine(&self) -> String {
            self.row
                .netbsd
                .machine
                .clone()
                .unwrap_or_else(|| "amd64".to_string())
        }
    }

    /// Run `rb toolchain`.
    pub fn make(args: &Args, root: &Path) -> Result<Made, String> {
        let files = Files::load(root, args.target.as_deref(), args.get("row"))?;
        let dir = PathBuf::from(args.get("dir").ok_or("give --dir DIR")?);
        let cc = args.get("cc").map(Cc::parse).transpose()?;
        let rucc_pin = files.toolchains.rucc.as_ref();
        let found = Rucc::find(
            args.get("rucc"),
            std::env::var("RB_RUCC").ok().as_deref(),
            rucc_pin,
            std::env::var_os("PATH"),
        );
        let rucc = match (found, cc) {
            (Ok(path), _) => Some(Rucc::describe(&path, rucc_pin)?),
            (Err(e), Some(Cc::Rucc)) => return Err(e),
            // G needs no rucc.
            (Err(_), _) => None,
        };
        // The shims run in the build's directories, so every path they hold is absolute.
        let full = |name: &str| {
            args.get(name)
                .map(|p| std::path::absolute(p).unwrap_or_else(|_| PathBuf::from(p)))
        };
        let (bringup, bringup_units) =
            shims::bringup(args.get("bringup"), args.get("bringup-units"), &|k| {
                std::env::var(k).ok()
            })?;
        let ask = Ask {
            rucc: rucc.as_ref().map(|r| r.path.clone()),
            log: full("log").unwrap_or_default(),
            tools: full("tools"),
            destdir: full("destdir"),
            side: cc.map(Cc::side),
            bringup,
            bringup_units,
        };
        let shims = shims::plan(
            &files.pin,
            &files.row,
            &files.personas,
            &files.toolchains,
            &ask,
        )?;
        let binary =
            shims::shim_binary(args.get("shim"), std::env::var("RB_SHIM").ok().as_deref())?;
        let expected = files.expected()?;
        let ungraded = args.has("ungraded");
        let mut text = String::new();
        let checks = if files.pin.system == "freebsd" {
            let (g, r) = make_freebsd(&dir, &binary, &shims, &files.toolchains)?;
            let _ = writeln!(text, "CROSS_TOOLCHAIN for G: {}", g.display());
            let _ = writeln!(text, "CROSS_TOOLCHAIN for R: {}", r.display());
            let rucc_needed = cc != Some(Cc::Ref);
            diff_freebsd(&g, &r, &expected, rucc_pin, rucc_needed, ungraded)?
        } else {
            let tools = ask.tools.as_ref().ok_or("give --tools DIR")?;
            let side = ask.side.ok_or("give --cc rucc or ref")?;
            make_netbsd(&dir, tools, &binary, &shims, &files.machine())?;
            let _ = writeln!(text, "TOOLDIR for {}: {}", side.letter(), dir.display());
            diff_netbsd(
                &[(side, &dir)],
                &expected,
                &files.machine(),
                rucc_pin,
                ungraded,
            )?
        };
        text.push_str(&shims::report(
            &dir.join("bin"),
            &shims,
            rucc.as_ref(),
            rucc_pin,
        ));
        let (checked, ok) = report(&checks);
        text.push_str(&checked);
        Ok(Made { report: text, ok })
    }

    /// Run `rb toolchain-diff`.
    pub fn diff(args: &Args, root: &Path) -> Result<(String, bool), String> {
        let files = Files::load(root, args.target.as_deref(), args.get("row"))?;
        let expected = files.expected()?;
        let rucc_pin = files.toolchains.rucc.as_ref();
        let ungraded = args.has("ungraded");
        let checks = if files.pin.system == "freebsd" {
            let (g, r) = match (args.get("dir"), args.get("g"), args.get("r")) {
                (Some(d), None, None) => (Path::new(d).join(REF_MK), Path::new(d).join(RUCC_MK)),
                (None, Some(g), Some(r)) => (PathBuf::from(g), PathBuf::from(r)),
                _ => return Err("give --dir DIR, or --g FILE and --r FILE".into()),
            };
            diff_freebsd(&g, &r, &expected, rucc_pin, true, ungraded)?
        } else {
            let mut dirs: Vec<(Side, &Path)> = Vec::new();
            if let Some(g) = args.get("g") {
                dirs.push((Side::G, Path::new(g)));
            }
            if let Some(r) = args.get("r") {
                dirs.push((Side::R, Path::new(r)));
            }
            if dirs.is_empty() {
                return Err("give --g TOOLDIR, --r TOOLDIR or both".into());
            }
            diff_netbsd(&dirs, &expected, &files.machine(), rucc_pin, ungraded)?
        };
        Ok(report(&checks))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_freebsd_files_differ_in_xcc_only() {
        let g = freebsd_mk("/rb/bin", "x86_64-unknown-freebsd15.1", "/p/", Side::G);
        let r = freebsd_mk("/rb/bin", "x86_64-unknown-freebsd15.1", "/p/", Side::R);
        let changed: Vec<_> = g.lines().zip(r.lines()).filter(|(a, b)| a != b).collect();
        assert_eq!(changed.len(), 1);
        assert!(changed[0].0.starts_with("XCC="));
        assert!(r.contains("XCC=/rb/bin/x86_64-unknown-freebsd15.1-rb-cc-rucc\n"));
    }

    #[test]
    fn aarch64_turns_off_outline_atomics_on_both_sides() {
        for side in [Side::G, Side::R] {
            let a64 = freebsd_mk("/rb/bin", "aarch64-unknown-freebsd15.1", "/p/", side);
            assert!(a64.ends_with("XCFLAGS=-mno-outline-atomics\n"), "{a64}");
            let rv = freebsd_mk("/rb/bin", "riscv64-unknown-freebsd15.1", "/p/", side);
            assert!(!rv.contains("XCFLAGS"), "{rv}");
        }
        let g = freebsd_mk("/rb/bin", "aarch64-unknown-freebsd15.1", "/p/", Side::G);
        let r = freebsd_mk("/rb/bin", "aarch64-unknown-freebsd15.1", "/p/", Side::R);
        let changed = g.lines().zip(r.lines()).filter(|(a, b)| a != b).count();
        assert_eq!((changed, g.lines().count()), (1, r.lines().count()));
    }

    #[test]
    fn the_files_under_conf_are_what_rb_writes() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let toolchains = Toolchains::load(&root).unwrap();
        let prefix = cross_prefix(&toolchains, "amd64-gcc15").unwrap();
        for (file, side) in [(REF_MK, Side::G), (RUCC_MK, Side::R)] {
            let text = std::fs::read_to_string(root.join("conf/freebsd").join(file)).unwrap();
            let body: String = text
                .lines()
                .filter(|l| !l.starts_with('#') && !l.contains("?="))
                .fold(String::new(), |mut s, l| {
                    s.push_str(l);
                    s.push('\n');
                    s
                });
            assert_eq!(
                body.trim_start(),
                freebsd_mk("${RB_SHIMS}", "x86_64-unknown-freebsd15.1", &prefix, side),
                "{file}"
            );
            assert_eq!(expand(&text, "${RB_SHIMS}"), "/usr/local/rb/bin", "{file}");
        }
    }

    #[test]
    fn make_assignments_skip_comments_and_defaults() {
        let text = "# c\nRB_SHIMS?=/x\n\nXCC=${RB_SHIMS}/a\n";
        assert_eq!(
            assignments(text),
            [("XCC".to_string(), "${RB_SHIMS}/a".to_string())]
        );
        assert_eq!(expand(text, "${RB_SHIMS}/a"), "/x/a");
    }

    #[test]
    fn hashes_and_differences() {
        let dir = std::env::temp_dir().join(format!("rb-toolchain-h-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for side in ["a", "b"] {
            std::fs::create_dir_all(dir.join(side).join("bin")).unwrap();
            std::fs::write(dir.join(side).join("bin/as"), "as").unwrap();
            std::fs::write(dir.join(side).join("bin/nbmake-amd64"), side).unwrap();
        }
        std::fs::write(dir.join("b/bin/gcc"), "shim").unwrap();
        let skip = |r: &str| r.ends_with("nbmake-amd64");
        let a = hashes(&dir.join("a"), &skip).unwrap();
        let b = hashes(&dir.join("b"), &skip).unwrap();
        assert_eq!(differing(&a, &b), ["bin/gcc"]);
        std::fs::remove_dir_all(&dir).ok();
    }
    #[test]
    fn bring_up_needs_ungraded_and_says_the_run_is_ungraded() {
        let expected = Expected {
            rucc_args: Vec::new(),
            target: "x86_64--netbsd".into(),
            reference: None,
            cross_prefix: None,
        };
        let config = ShimConfig {
            bringup: vec!["as".into()],
            ..ShimConfig::default()
        };
        let graded = check_config("R", &config, &expected, None, false, false);
        assert!(
            graded
                .iter()
                .any(|c| !c.ok && c.what.contains("no bring-up"))
        );
        let ungraded = check_config("R", &config, &expected, None, false, true);
        assert!(ungraded.iter().all(|c| c.ok), "{ungraded:?}");
        assert!(
            ungraded
                .iter()
                .any(|c| c.what.contains("the run is ungraded"))
        );
        let empty = ShimConfig {
            bringup: vec!["cxx-c".into()],
            ..ShimConfig::default()
        };
        let checks = check_config("R", &empty, &expected, None, false, true);
        assert!(checks.iter().any(|c| !c.ok), "{checks:?}");
    }
}
