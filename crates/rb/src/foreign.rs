//! `rb foreign`: the project's own binaries on a rucc world (plan 12.8).
//!
//! This module does the packages part of 12.8. A file such as `conf/netbsd/foreign.toml` names a
//! package repository of the project and about 30 packages, each with a check. [`fetch`] reads the
//! repository's summary, finds each package and what it depends on, and downloads them. The
//! foreign suite of rb-rc (`guest/rb-rc/foreign.sh`) installs them in the guest and runs the
//! checks. [`grade`] reads the results of R and of G: a check that failed on R counts when it
//! passed on G, because a package that also fails on G says nothing about rucc.
//!
//! The summary is NetBSD's `pkg_summary`: one record per package, a `KEY=VALUE` line per field and
//! an empty line after each record. A `DEPENDS` field is a pattern, such as `perl>=5.38`,
//! `gettext-lib-[0-9]*` or `{python313,python312}>=0`.

use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::Path;
use std::process::Command;

/// The file that names the repository and the packages.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Conf {
    /// The directory of the package files, which has `pkg_summary.gz`.
    pub repository: String,
    /// The packages to install and check, in order.
    pub package: Vec<Package>,
}

/// One package to check.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Package {
    /// The package's name with no version, such as `bash`.
    pub name: String,
    /// The shell command that checks it.
    pub check: String,
}

impl Conf {
    /// Read and check the file.
    pub fn load(path: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        Self::parse(&text).map_err(|e| format!("{}: {e}", path.display()))
    }

    /// Read the text of the file. Each name is unique, and each check is one line with no tab,
    /// because the `checks` file of the suite has one line per package with a tab after the name.
    pub fn parse(text: &str) -> Result<Self, String> {
        let mut conf: Conf = toml::from_str(text).map_err(|e| e.to_string())?;
        let mut seen = BTreeSet::new();
        for p in &mut conf.package {
            p.check = p.check.trim().to_string();
            if p.name.is_empty() || p.name.contains(char::is_whitespace) {
                return Err(format!("the package name {:?} is not a name", p.name));
            }
            if !seen.insert(p.name.clone()) {
                return Err(format!("the package {} is there twice", p.name));
            }
            if p.check.is_empty() || p.check.contains(['\t', '\n']) {
                return Err(format!(
                    "the check of {} must be one line with no tab",
                    p.name
                ));
            }
        }
        if conf.package.is_empty() {
            return Err("no packages".into());
        }
        Ok(conf)
    }

    /// The `checks` file of the foreign suite: the name, a tab and the check, per package.
    #[must_use]
    pub fn checks(&self) -> String {
        let mut s = String::new();
        for p in &self.package {
            let _ = writeln!(s, "{}\t{}", p.name, p.check);
        }
        s
    }
}

/// One package of the summary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// `PKGNAME`, the name with the version, such as `bash-5.3.20`.
    pub pkgname: String,
    /// `FILE_NAME`, else `PKGNAME.tgz`.
    pub file: String,
    /// `FILE_SIZE`, the size of the file.
    pub size: Option<u64>,
    /// The patterns of `DEPENDS`.
    pub depends: Vec<String>,
}

/// The records of a `pkg_summary`. A record with no `PKGNAME` is left out.
#[must_use]
pub fn parse_summary(text: &str) -> Vec<Entry> {
    let mut entries = Vec::new();
    let mut fields: Vec<(&str, &str)> = Vec::new();
    for line in text.lines().chain(std::iter::once("")) {
        if !line.is_empty() {
            if let Some(kv) = line.split_once('=') {
                fields.push(kv);
            }
            continue;
        }
        let get = |key: &str| fields.iter().find(|(k, _)| *k == key).map(|(_, v)| *v);
        if let Some(pkgname) = get("PKGNAME") {
            entries.push(Entry {
                pkgname: pkgname.to_string(),
                file: get("FILE_NAME").map_or_else(|| format!("{pkgname}.tgz"), str::to_string),
                size: get("FILE_SIZE").and_then(|s| s.parse().ok()),
                depends: fields
                    .iter()
                    .filter(|(k, _)| *k == "DEPENDS")
                    .map(|(_, v)| (*v).to_string())
                    .collect(),
            });
        }
        fields.clear();
    }
    entries
}

/// The name of a package with no version: `bash-5.3.20` is `bash`.
#[must_use]
pub fn base(pkgname: &str) -> &str {
    pkgname.rsplit_once('-').map_or(pkgname, |(name, _)| name)
}

/// The names a dependency pattern can match, in its order. Braces give alternatives, and the
/// version is cut off: `{py313-a,py312-a}>=1` gives `py313-a` and `py312-a`, and
/// `gettext-lib-[0-9]*` gives `gettext-lib`.
#[must_use]
pub fn pattern_names(pattern: &str) -> Vec<String> {
    let mut names = Vec::new();
    for alt in expand(pattern) {
        let name = match alt.find(['<', '>', '=']) {
            Some(i) => &alt[..i],
            None => match last_dash(&alt) {
                Some(i)
                    if alt[i + 1..]
                        .starts_with(|c: char| c.is_ascii_digit() || c == '[' || c == '*') =>
                {
                    &alt[..i]
                }
                _ => &alt,
            },
        };
        if !names.iter().any(|n| n == name) {
            names.push(name.to_string());
        }
    }
    names
}

/// The place of the last `-` that is not in a `[...]` class, as in `name-[0-9]*`.
fn last_dash(pattern: &str) -> Option<usize> {
    let mut class = false;
    let mut dash = None;
    for (i, c) in pattern.char_indices() {
        match c {
            '[' => class = true,
            ']' => class = false,
            '-' if !class => dash = Some(i),
            _ => {}
        }
    }
    dash
}

/// The words of a pattern with braces, as the shell expands them.
fn expand(pattern: &str) -> Vec<String> {
    let Some(open) = pattern.find('{') else {
        return vec![pattern.to_string()];
    };
    let mut depth = 0;
    let mut close = None;
    let mut cuts = vec![open];
    for (i, c) in pattern[open..].char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    close = Some(open + i);
                    break;
                }
            }
            ',' if depth == 1 => cuts.push(open + i),
            _ => {}
        }
    }
    let Some(close) = close else {
        return vec![pattern.to_string()];
    };
    cuts.push(close);
    let (head, tail) = (&pattern[..open], &pattern[close + 1..]);
    let mut words = Vec::new();
    for pair in cuts.windows(2) {
        let word = format!("{head}{}{tail}", &pattern[pair[0] + 1..pair[1]]);
        words.extend(expand(&word));
    }
    words
}

/// The packages to fetch for `wanted`: each one and what it depends on, sorted by name. The first
/// name of a pattern that the summary has is taken. A name or a dependency that the summary does
/// not have is an error.
pub fn closure<'a>(entries: &'a [Entry], wanted: &[&str]) -> Result<Vec<&'a Entry>, String> {
    let by_name: BTreeMap<&str, &Entry> = entries.iter().map(|e| (base(&e.pkgname), e)).collect();
    let mut chosen: BTreeMap<&str, &Entry> = BTreeMap::new();
    let mut todo: Vec<(String, Option<&str>)> = wanted
        .iter()
        .rev()
        .map(|w| ((*w).to_string(), None))
        .collect();
    while let Some((name, needed_by)) = todo.pop() {
        let Some(&entry) = by_name.get(name.as_str()) else {
            return Err(match needed_by {
                None => format!("the repository has no package {name}"),
                Some(by) => format!("{by} needs {name}, which the repository does not have"),
            });
        };
        let key = base(&entry.pkgname);
        if chosen.insert(key, entry).is_some() {
            continue;
        }
        for pattern in &entry.depends {
            let names = pattern_names(pattern);
            let pick = names
                .iter()
                .find(|n| by_name.contains_key(n.as_str()))
                .or(names.first())
                .cloned()
                .unwrap_or_default();
            todo.push((pick, Some(entry.pkgname.as_str())));
        }
    }
    Ok(chosen.into_values().collect())
}

/// The data of a gzip file, one member.
pub fn gunzip(data: &[u8]) -> Result<Vec<u8>, String> {
    let bad = || "not a gzip file".to_string();
    if data.len() < 18 || data[..3] != [0x1f, 0x8b, 8] {
        return Err(bad());
    }
    let flags = data[3];
    let mut at = 10;
    if flags & 4 != 0 {
        let len = usize::from(u16::from_le_bytes([data[at], data[at + 1]]));
        at += 2 + len;
    }
    for bit in [8, 16] {
        if flags & bit != 0 {
            at += data
                .get(at..)
                .and_then(|d| d.iter().position(|&b| b == 0))
                .ok_or_else(bad)?
                + 1;
        }
    }
    if flags & 2 != 0 {
        at += 2;
    }
    let body = data.get(at..).ok_or_else(bad)?;
    miniz_oxide::inflate::decompress_to_vec(body).map_err(|e| format!("gzip: {e:?}"))
}

/// Download `url` to `to` with curl.
fn download(url: &str, to: &Path) -> Result<(), String> {
    let out = Command::new("curl")
        .args([
            "-fsSL",
            "--retry",
            "5",
            "--retry-all-errors",
            "--max-time",
            "900",
            "-o",
        ])
        .arg(to)
        .arg(url)
        .output()
        .map_err(|e| format!("cannot run curl: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "curl {url}: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(())
}

/// Fetch the packages of `conf` and what they need to `out/All`, and write `out/checks`. A file
/// that is there with the size of the summary is not fetched again. Returns the report.
pub fn fetch(conf: &Conf, out: &Path) -> Result<String, String> {
    let all = out.join("All");
    std::fs::create_dir_all(&all).map_err(|e| format!("{}: {e}", all.display()))?;
    let repo = conf.repository.trim_end_matches('/');
    let gz = out.join("pkg_summary.gz");
    download(&format!("{repo}/pkg_summary.gz"), &gz)?;
    let data = std::fs::read(&gz).map_err(|e| format!("{}: {e}", gz.display()))?;
    std::fs::remove_file(&gz).map_err(|e| format!("{}: {e}", gz.display()))?;
    let text = String::from_utf8_lossy(&gunzip(&data)?).into_owned();
    let entries = parse_summary(&text);
    let wanted: Vec<&str> = conf.package.iter().map(|p| p.name.as_str()).collect();
    let chosen = closure(&entries, &wanted)?;
    let mut bytes = 0;
    for e in &chosen {
        let file = all.join(&e.file);
        let have = std::fs::metadata(&file).ok().map(|m| m.len());
        if have.is_none() || have != e.size {
            download(&format!("{repo}/{}", e.file), &file)?;
        }
        let got = std::fs::metadata(&file)
            .map_err(|err| format!("{}: {err}", file.display()))?
            .len();
        if let Some(size) = e.size
            && got != size
        {
            return Err(format!(
                "{} has {got} bytes, and the summary says {size}",
                e.file
            ));
        }
        bytes += got;
    }
    let checks = out.join("checks");
    std::fs::write(&checks, conf.checks()).map_err(|e| format!("{}: {e}", checks.display()))?;
    let by_name: BTreeMap<&str, &str> = chosen
        .iter()
        .map(|e| (base(&e.pkgname), e.pkgname.as_str()))
        .collect();
    let mut s = String::new();
    let _ = writeln!(
        s,
        "rb foreign: {} packages to check, {} files with what they need, {} MiB, from {repo}",
        wanted.len(),
        chosen.len(),
        bytes >> 20
    );
    for name in wanted {
        let _ = writeln!(s, "  {}", by_name.get(name).copied().unwrap_or(name));
    }
    Ok(s)
}

/// The result of one package in the foreign suite.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    /// The package's name with no version.
    pub name: String,
    /// `pass`, `add` (`pkg_add` failed) or `check` (the check failed).
    pub result: String,
    /// The last line the failed command printed.
    pub error: String,
}

/// The lines of a `foreign.tsv`.
#[must_use]
pub fn parse_results(text: &str) -> Vec<Outcome> {
    text.lines()
        .filter_map(|line| {
            let mut f = line.splitn(3, '\t');
            Some(Outcome {
                name: f.next().filter(|n| !n.is_empty())?.to_string(),
                result: f.next()?.to_string(),
                error: f.next().unwrap_or_default().to_string(),
            })
        })
        .collect()
}

/// Read the foreign results of the rb boot whose output directory is `dir`.
fn read_results(dir: &Path) -> Result<Vec<Outcome>, String> {
    let file = dir.join("results/foreign/foreign.tsv");
    let text = std::fs::read_to_string(&file).map_err(|e| format!("{}: {e}", file.display()))?;
    Ok(parse_results(&text))
}

/// Grade R's results against G's. A package that did not pass on R counts when it passed on G, or
/// when there is no G. Returns the report and whether nothing counts. No results at all count.
#[must_use]
pub fn grade_results(r: &[Outcome], g: Option<&[Outcome]>, limit: usize) -> (String, bool) {
    let g_of = |name: &str| g.and_then(|g| g.iter().find(|o| o.name == name));
    let passed = r.iter().filter(|o| o.result == "pass").count();
    let mut counted = Vec::new();
    let mut excused = Vec::new();
    for o in r.iter().filter(|o| o.result != "pass") {
        match g_of(&o.name) {
            Some(go) if go.result != "pass" => excused.push((o, go)),
            _ => counted.push(o),
        }
    }
    let mut s = String::new();
    let _ = writeln!(
        s,
        "rb foreign: {passed} of {} packages passed on R{}",
        r.len(),
        match g {
            Some(g) => format!(
                ", {} of {} on G",
                g.iter().filter(|o| o.result == "pass").count(),
                g.len()
            ),
            None => ", and there are no G results, so each failure counts".into(),
        }
    );
    if r.is_empty() {
        s.push_str("No package ran on R.\n");
    }
    if !counted.is_empty() {
        let _ = writeln!(s, "\nFailed on R and not on G, {}:", counted.len());
        for o in counted.iter().take(limit) {
            let _ = writeln!(s, "  {} {}: {}", o.name, o.result, o.error);
        }
        if counted.len() > limit {
            let _ = writeln!(s, "  and {} more", counted.len() - limit);
        }
    }
    if !excused.is_empty() {
        let _ = writeln!(
            s,
            "\nFailed on G too, so they do not count, {}:",
            excused.len()
        );
        for (o, go) in excused.iter().take(limit) {
            let _ = writeln!(
                s,
                "  {} R {}, G {}: {}",
                o.name, o.result, go.result, o.error
            );
        }
        if excused.len() > limit {
            let _ = writeln!(s, "  and {} more", excused.len() - limit);
        }
    }
    (s, counted.is_empty() && !r.is_empty())
}

/// `rb foreign --r DIR [--g DIR]`: grade the results of two boots.
pub fn grade(r: &Path, g: Option<&Path>, limit: usize) -> Result<(String, bool), String> {
    let r = read_results(r)?;
    let g = g.map(read_results).transpose()?;
    Ok(grade_results(&r, g.as_deref(), limit))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SUMMARY: &str = "\
PKGNAME=git-base-2.55.0nb1
DEPENDS=curl>=7.0
DEPENDS={perl>=5.0,perl-base>=5.0}
DEPENDS=gettext-lib-[0-9]*
FILE_NAME=git-base-2.55.0nb1.tgz
FILE_SIZE=7271000
DESCRIPTION=A=B is not a field.

PKGNAME=curl-8.23.0
DEPENDS=libnghttp2>=1.0
FILE_SIZE=1038000

PKGNAME=perl-5.44.0nb2
FILE_SIZE=13350000

PKGNAME=gettext-lib-0.22.5
FILE_SIZE=100

PKGNAME=libnghttp2-1.64.0
DEPENDS=curl>=7.0

PKGNAME=lonely-1.0
DEPENDS=missing-[0-9]*
";

    #[test]
    fn the_summary_gives_names_files_sizes_and_depends() {
        let entries = parse_summary(SUMMARY);
        assert_eq!(entries.len(), 6);
        assert_eq!(entries[0].pkgname, "git-base-2.55.0nb1");
        assert_eq!(entries[0].file, "git-base-2.55.0nb1.tgz");
        assert_eq!(entries[0].size, Some(7_271_000));
        assert_eq!(entries[0].depends.len(), 3);
        // With no FILE_NAME the file is PKGNAME.tgz, and with no FILE_SIZE the size is unknown.
        assert_eq!(entries[4].file, "libnghttp2-1.64.0.tgz");
        assert_eq!(entries[4].size, None);
        assert_eq!(base("git-base-2.55.0nb1"), "git-base");
        assert_eq!(base("bash"), "bash");
    }

    #[test]
    fn patterns_give_the_names_they_can_match() {
        assert_eq!(pattern_names("perl>=5.38"), ["perl"]);
        assert_eq!(pattern_names("gettext-lib-[0-9]*"), ["gettext-lib"]);
        assert_eq!(pattern_names("py313-cairo-1.2*"), ["py313-cairo"]);
        assert_eq!(pattern_names("tcl>=8.6<9"), ["tcl"]);
        assert_eq!(pattern_names("some-thing"), ["some-thing"]);
        assert_eq!(
            pattern_names("{python313,python312}>=0"),
            ["python313", "python312"]
        );
        assert_eq!(
            pattern_names("py{313,312}-{a,b}-[0-9]*"),
            ["py313-a", "py313-b", "py312-a", "py312-b"]
        );
    }

    #[test]
    fn the_closure_has_each_package_and_what_it_needs_once() {
        let entries = parse_summary(SUMMARY);
        let names: Vec<&str> = closure(&entries, &["git-base"])
            .unwrap()
            .iter()
            .map(|e| e.pkgname.as_str())
            .collect();
        // The loop of curl and libnghttp2 ends, and perl comes from the first alternative.
        assert_eq!(
            names,
            [
                "curl-8.23.0",
                "gettext-lib-0.22.5",
                "git-base-2.55.0nb1",
                "libnghttp2-1.64.0",
                "perl-5.44.0nb2"
            ]
        );
        assert_eq!(
            closure(&entries, &["nothere"]).unwrap_err(),
            "the repository has no package nothere"
        );
        assert_eq!(
            closure(&entries, &["lonely"]).unwrap_err(),
            "lonely-1.0 needs missing, which the repository does not have"
        );
    }

    #[test]
    fn gunzip_reads_a_member_with_a_name() {
        let body = miniz_oxide::deflate::compress_to_vec(b"PKGNAME=a-1\n", 6);
        let mut gz = vec![0x1f, 0x8b, 8, 8, 0, 0, 0, 0, 0, 3];
        gz.extend(b"pkg_summary\0");
        gz.extend(&body);
        gz.extend([0; 8]);
        assert_eq!(gunzip(&gz).unwrap(), b"PKGNAME=a-1\n");
        assert!(gunzip(b"PKGNAME=a-1\n with no header").is_err());
    }

    #[test]
    fn the_conf_is_checked_and_gives_the_checks_file() {
        let conf = Conf::parse(
            "repository = \"https://example.org/All\"\n\
             [[package]]\nname = \"bash\"\ncheck = '''bash -c 'echo ok' '''\n\
             [[package]]\nname = \"jq\"\ncheck = \"jq -n 1\"\n",
        )
        .unwrap();
        assert_eq!(conf.checks(), "bash\tbash -c 'echo ok'\njq\tjq -n 1\n");
        let twice = "repository = \"r\"\n[[package]]\nname = \"a\"\ncheck = \"x\"\n\
                     [[package]]\nname = \"a\"\ncheck = \"y\"\n";
        assert!(Conf::parse(twice).unwrap_err().contains("twice"));
        let tab = "repository = \"r\"\n[[package]]\nname = \"a\"\ncheck = \"x\\ty\"\n";
        assert!(Conf::parse(tab).unwrap_err().contains("no tab"));
        assert!(Conf::parse("repository = \"r\"\npackage = []\n").is_err());
    }

    #[test]
    fn the_netbsd_conf_has_about_30_packages() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../conf/netbsd/foreign.toml");
        let conf = Conf::load(&path).unwrap();
        assert_eq!(conf.package.len(), 30);
        assert!(
            conf.repository
                .starts_with("https://cdn.netbsd.org/pub/pkgsrc/packages/NetBSD/")
        );
    }

    #[test]
    fn a_failure_counts_when_g_passed_or_there_is_no_g() {
        let r = parse_results("bash\tpass\t\nvim\tcheck\tE1\nperl\tadd\tno libm\n");
        let g = parse_results("bash\tpass\t\nvim\tcheck\tE1\nperl\tpass\t\n");
        let (text, ok) = grade_results(&r, Some(&g), 20);
        assert!(!ok);
        assert!(text.starts_with("rb foreign: 1 of 3 packages passed on R, 2 of 3 on G\n"));
        assert!(
            text.contains("Failed on R and not on G, 1:\n  perl add: no libm\n"),
            "{text}"
        );
        assert!(
            text.contains("do not count, 1:\n  vim R check, G check: E1\n"),
            "{text}"
        );
        let g = parse_results("bash\tpass\t\nvim\tcheck\t\nperl\tadd\t\n");
        assert!(grade_results(&r, Some(&g), 20).1);
        let (text, ok) = grade_results(&r, None, 1);
        assert!(!ok);
        assert!(text.contains("no G results"), "{text}");
        assert!(text.contains("  and 1 more\n"), "{text}");
        assert!(!grade_results(&[], None, 20).1);
    }
}
