//! `rb sets`: expand `sets.toml` into pins from what the projects publish (plan 04.5 to 04.7).

use crate::good::Good;
use crate::pins::{Gone, Pin, PinsFile, Tarball};
use crate::publish;
use crate::sets::{Current, Releases, Sets};
use crate::web::{Ref, Web};

/// The FreeBSD repository on GitHub.
pub const FREEBSD_REPO: &str = "freebsd/freebsd-src";
/// The NetBSD git mirror on GitHub.
pub const NETBSD_REPO: &str = "NetBSD/src";
/// The page with FreeBSD's table of supported branches and releases.
pub const FREEBSD_SECURITY: &str = "https://www.freebsd.org/security/";
/// Where FreeBSD release files are published.
pub const FREEBSD_RELEASES: &str = "https://download.freebsd.org/releases/amd64";
/// The page with NetBSD's list of supported releases.
pub const NETBSD_RELEASES: &str = "https://www.netbsd.org/releases";
/// Where NetBSD release files are published.
pub const NETBSD_CDN: &str = "https://cdn.netbsd.org/pub/NetBSD";

/// GCC's version file in the NetBSD tree.
pub const NETBSD_BASE_VER: &str = "external/gpl3/gcc/dist/gcc/BASE-VER";

/// The pins, and the notes a person should read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Expanded {
    /// The new file.
    pub file: PinsFile,
    /// What changed that needs a person: a persona review, a branch left behind, a release left
    /// out by a rule.
    pub notes: Vec<String>,
}

/// Expand `sets` as of `as_of`, an RFC 3339 UTC time. `until` is passed to the commit queries;
/// it is `None` for "now". `old` is the current `pins.toml`, which keeps last-g-good pins. `good`
/// is the complete baselines, newest first, from [`crate::good::from_baselines`].
pub fn expand(
    sets: &Sets,
    web: &dyn Web,
    as_of: &str,
    until: Option<&str>,
    old: Option<&PinsFile>,
    good: &[Good],
) -> Result<Expanded, String> {
    let mut run = Run {
        web,
        until,
        old,
        good,
        last_good: sets.pin_policy.head == "last-g-good",
        notes: Vec::new(),
        pins: Vec::new(),
    };
    run.freebsd(&sets.freebsd.current, &sets.freebsd.releases)?;
    run.netbsd(&sets.netbsd.current, &sets.netbsd.releases)?;

    let mut gone: Vec<Gone> = old.map(|o| o.gone.clone()).unwrap_or_default();
    let day = as_of.get(..10).unwrap_or(as_of).to_string();
    if let Some(old) = old {
        for pin in &old.pin {
            if run.pins.iter().any(|p| p.name == pin.name)
                || gone.iter().any(|g| g.name == pin.name)
            {
                continue;
            }
            run.notes.push(format!(
                "{} left its set; it is kept under [[gone]]",
                pin.name
            ));
            gone.push(Gone {
                name: pin.name.clone(),
                system: pin.system.clone(),
                version: pin.version.clone(),
                commit: pin.commit.clone(),
                left: day.clone(),
            });
        }
    }
    gone.retain(|g| !run.pins.iter().any(|p| p.name == g.name));

    for pin in &run.pins {
        let Some(before) = old.and_then(|o| o.find(&pin.name)) else {
            continue;
        };
        if before.gcc.is_some() && before.gcc != pin.gcc {
            run.notes.push(format!(
                "persona review: {} GCC moved from {} to {} (04.7)",
                pin.name,
                before.gcc.as_deref().unwrap_or("-"),
                pin.gcc.as_deref().unwrap_or("-"),
            ));
        }
    }

    Ok(Expanded {
        file: PinsFile {
            as_of: as_of.to_string(),
            pin: run.pins,
            gone,
        },
        notes: run.notes,
    })
}

struct Run<'a> {
    web: &'a dyn Web,
    until: Option<&'a str>,
    old: Option<&'a PinsFile>,
    good: &'a [Good],
    last_good: bool,
    notes: Vec<String>,
    pins: Vec<Pin>,
}

impl Run<'_> {
    /// A branch pin at its head, or at its last good commit under last-g-good.
    ///
    /// A commit is good when a complete baseline names it. Under last-g-good the pin is the head
    /// if G is good there; else the commit of the newest complete baseline; else the commit the
    /// old `pins.toml` kept as good; else the head, untried. A branch with `no_ref` is at its head
    /// with `reference = "no-ref"` and the reason, whatever its baselines say.
    fn branch(
        &mut self,
        slug: &str,
        refs: &[Ref],
        mut pin: Pin,
        branch: &str,
        no_ref: Option<&String>,
    ) -> Result<Pin, String> {
        if !refs
            .iter()
            .any(|r| r.name == format!("refs/heads/{branch}"))
        {
            return Err(format!("{slug} has no branch {branch}"));
        }
        let head = self.web.commit_at(slug, branch, self.until)?;
        if let Some(reason) = no_ref {
            self.notes
                .push(format!("{} has no reference: {reason}", pin.name));
            pin.commit = Some(head.sha.clone());
            pin.date.clone_from(&head.date);
            pin.reference = Some("no-ref".into());
            pin.reason = Some(reason.clone());
            pin.repo = Some(format!("https://github.com/{slug}"));
            pin.branch = Some(branch.to_string());
            pin.head = Some(head.sha);
            pin.head_date = Some(head.date);
            return Ok(pin);
        }
        let good: Vec<&Good> = self.good.iter().filter(|g| g.pin == pin.name).collect();
        let before = self.old.and_then(|o| o.find(&pin.name));
        let kept = before
            .filter(|b| b.reference.as_deref() == Some("good"))
            .and_then(|b| Some((b.commit.clone()?, b.date.clone(), "pins.toml".to_string())));
        let last = if good.iter().any(|g| g.commit == head.sha) || !self.last_good {
            None
        } else if let Some(g) = good.first() {
            let at = self.web.commit_at(slug, &g.commit, None)?;
            if at.sha != g.commit {
                return Err(format!(
                    "{slug}: the commit {} of baselines/{} is not in the repository",
                    g.commit, g.place
                ));
            }
            Some((at.sha, at.date, format!("baselines/{}", g.place)))
        } else {
            kept.filter(|(commit, _, _)| *commit != head.sha)
        };
        if let Some((commit, date, from)) = last {
            self.notes.push(format!(
                "{} stays at its last good commit {} ({from}); the head is {} ({})",
                pin.name,
                short(&commit),
                short(&head.sha),
                head.date
            ));
            pin.commit = Some(commit);
            pin.date = date;
            pin.reference = Some("good".into());
        } else {
            let at_head = good.iter().any(|g| g.commit == head.sha)
                || (self.last_good
                    && before.is_some_and(|b| {
                        b.reference.as_deref() == Some("good")
                            && b.commit.as_deref() == Some(&head.sha)
                    }));
            pin.commit = Some(head.sha.clone());
            pin.date.clone_from(&head.date);
            pin.reference = Some(if at_head { "good" } else { "untried" }.into());
        }
        pin.repo = Some(format!("https://github.com/{slug}"));
        pin.branch = Some(branch.to_string());
        pin.head = Some(head.sha);
        pin.head_date = Some(head.date);
        Ok(pin)
    }

    fn freebsd(&mut self, current: &Current, wanted: &Releases) -> Result<(), String> {
        let refs = self.web.refs(FREEBSD_REPO)?;
        let table = publish::freebsd_supported(&self.web.get(FREEBSD_SECURITY)?)?;
        let cadence = if current.nightly { "nightly" } else { "weekly" };
        let stables: Vec<&str> = table
            .iter()
            .filter_map(|row| row.branch.strip_prefix("stable/"))
            .collect();

        for branch in &current.branches {
            if let Some(major) = branch.strip_prefix("stable/")
                && !stables.contains(&major)
            {
                self.notes.push(format!(
                    "FreeBSD {branch} is not in the support table; it is left out"
                ));
                continue;
            }
            let pin = Pin {
                name: format!("freebsd-{}", branch.replace('/', "-")),
                system: "freebsd".into(),
                set: "current".into(),
                kind: "branch".into(),
                cadence: cadence.into(),
                eol: table
                    .iter()
                    .find(|r| &r.branch == branch)
                    .and_then(|r| r.eol.clone()),
                ..Pin::default()
            };
            let mut pin =
                self.branch(FREEBSD_REPO, &refs, pin, branch, current.no_ref.get(branch))?;
            self.freebsd_version(&mut pin)?;
            self.pins.push(pin);
        }

        if current.releng_in_progress {
            for r in &refs {
                let Some(version) = r.name.strip_prefix("refs/heads/releng/") else {
                    continue;
                };
                let tagged = format!("refs/tags/release/{version}.0");
                let major = version.split('.').next().unwrap_or_default();
                if refs.iter().any(|t| t.name == tagged) || !stables.contains(&major) {
                    continue;
                }
                let pin = Pin {
                    name: format!("freebsd-releng-{version}"),
                    system: "freebsd".into(),
                    set: "current".into(),
                    kind: "releng".into(),
                    cadence: cadence.into(),
                    ..Pin::default()
                };
                let mut pin =
                    self.branch(FREEBSD_REPO, &refs, pin, &format!("releng/{version}"), None)?;
                self.freebsd_version(&mut pin)?;
                let stage = pin.version.rsplit('-').next().unwrap_or_default();
                if ["ALPHA", "BETA", "RC", "PRERELEASE"]
                    .iter()
                    .any(|s| stage.starts_with(s))
                {
                    self.pins.push(pin);
                }
            }
        }

        self.freebsd_releases(&refs, &table, wanted)
    }

    fn freebsd_releases(
        &mut self,
        refs: &[Ref],
        table: &[publish::Supported],
        wanted: &Releases,
    ) -> Result<(), String> {
        let mut rows: Vec<_> = table.iter().filter(|r| r.release.is_some()).collect();
        if wanted.latest_per_major {
            let all = rows.clone();
            rows.retain(|r| {
                let v = r.release.as_deref().unwrap_or_default();
                !all.iter().any(|o| {
                    let w = o.release.as_deref().unwrap_or_default();
                    major_of(w) == major_of(v) && newer(w, v)
                })
            });
            for r in all.iter().filter(|r| !rows.contains(r)) {
                self.notes.push(format!(
                    "FreeBSD {} is supported until {} but latest_per_major leaves it out",
                    r.release.as_deref().unwrap_or_default(),
                    r.eol.as_deref().unwrap_or("?")
                ));
            }
        }
        let cadence = if wanted.weekly { "weekly" } else { "nightly" };
        for row in rows {
            let version = row.release.clone().unwrap_or_default();
            let tag = format!("release/{version}.0");
            let commit = refs
                .iter()
                .find(|r| r.name == format!("refs/tags/{tag}"))
                .ok_or_else(|| format!("{FREEBSD_REPO} has no tag {tag}"))?
                .sha
                .clone();
            let base = format!("{FREEBSD_RELEASES}/{version}-RELEASE");
            let manifest = self.web.get(&format!("{base}/MANIFEST"))?;
            let sha256 = publish::freebsd_manifest(&manifest, "src.txz")
                .ok_or_else(|| format!("{base}/MANIFEST has no src.txz"))?;
            let released = row
                .released
                .clone()
                .ok_or_else(|| format!("FreeBSD {version} has no release date in the table"))?;
            self.pins.push(Pin {
                name: format!("freebsd-{version}"),
                system: "freebsd".into(),
                set: "releases".into(),
                kind: "release".into(),
                series: major_of(&version).to_string(),
                version: format!("{version}-RELEASE"),
                cadence: cadence.into(),
                repo: Some(format!("https://github.com/{FREEBSD_REPO}")),
                tag: Some(tag),
                commit: Some(commit),
                date: released.clone(),
                released: Some(released),
                eol: row.eol.clone(),
                tarball: vec![Tarball {
                    name: "src.txz".into(),
                    url: format!("{base}/src.txz"),
                    sha256: Some(sha256),
                    sha512: None,
                }],
                ..Pin::default()
            });
        }
        Ok(())
    }

    fn freebsd_version(&self, pin: &mut Pin) -> Result<(), String> {
        let commit = pin.commit.clone().unwrap_or_default();
        let text = self
            .web
            .file(FREEBSD_REPO, &commit, "sys/conf/newvers.sh")?;
        pin.version = publish::freebsd_newvers(&text)?;
        pin.series = major_of(&pin.version).to_string();
        Ok(())
    }

    fn netbsd(&mut self, current: &Current, wanted: &Releases) -> Result<(), String> {
        let refs = self.web.refs(NETBSD_REPO)?;
        let cadence = if current.nightly { "nightly" } else { "weekly" };
        for branch in &current.branches {
            let series = branch.strip_prefix("netbsd-").unwrap_or("current");
            let pin = Pin {
                name: format!("netbsd-{series}").replace("netbsd-current", "netbsd-trunk"),
                system: "netbsd".into(),
                set: "current".into(),
                kind: "branch".into(),
                series: series.into(),
                cadence: cadence.into(),
                ..Pin::default()
            };
            let mut pin =
                self.branch(NETBSD_REPO, &refs, pin, branch, current.no_ref.get(branch))?;
            let commit = pin.commit.clone().unwrap_or_default();
            let param = self.web.file(NETBSD_REPO, &commit, "sys/sys/param.h")?;
            pin.version = publish::netbsd_param(&param)?;
            let base = self.web.file(NETBSD_REPO, &commit, NETBSD_BASE_VER)?;
            pin.gcc = Some(publish::gcc_base_ver(&base)?);
            self.pins.push(pin);
        }

        let page = self.web.get(&format!("{NETBSD_RELEASES}/"))?;
        let mut supported = publish::netbsd_supported(&page)?;
        if wanted.latest_per_major {
            let all = supported.clone();
            supported.retain(|(m, v)| !all.iter().any(|(n, w)| n == m && newer(w, v)));
        }
        let cadence = if wanted.weekly { "weekly" } else { "nightly" };
        for (major, version) in supported {
            let announce = self.web.get(&format!(
                "{NETBSD_RELEASES}/formal-{major}/NetBSD-{version}.html"
            ))?;
            let released = publish::netbsd_announced(&announce)
                .ok_or_else(|| format!("the NetBSD {version} announcement has no date"))?;
            let sets = format!("{NETBSD_CDN}/NetBSD-{version}/source/sets");
            let sums = publish::bsd_sums(&self.web.get(&format!("{sets}/SHA512"))?, "SHA512");
            let tarball: Vec<Tarball> = sums
                .into_iter()
                .filter(|(name, _)| {
                    std::path::Path::new(name)
                        .extension()
                        .is_some_and(|e| e.eq_ignore_ascii_case("tgz"))
                })
                .map(|(name, sum)| Tarball {
                    url: format!("{sets}/{name}"),
                    name,
                    sha256: None,
                    sha512: Some(sum),
                })
                .collect();
            if !tarball.iter().any(|t| t.name == "src.tgz") {
                return Err(format!("{sets}/SHA512 has no src.tgz"));
            }
            let from = format!("netbsd-{major}");
            let gcc = self
                .pins
                .iter()
                .find(|p| p.name == from)
                .and_then(|p| p.gcc.clone());
            self.pins.push(Pin {
                name: format!("netbsd-{version}"),
                system: "netbsd".into(),
                set: "releases".into(),
                kind: "release".into(),
                series: major.clone(),
                version: version.clone(),
                cadence: cadence.into(),
                date: released.clone(),
                gcc_from: gcc.is_some().then_some(from),
                gcc,
                released: Some(released),
                tarball,
                ..Pin::default()
            });
        }
        Ok(())
    }
}

/// The major version of `15.1`, `15.1-RELEASE` or `16.0-CURRENT`.
fn major_of(version: &str) -> &str {
    version.split(['.', '-']).next().unwrap_or(version)
}

/// Whether dotted version `a` is newer than `b`.
fn newer(a: &str, b: &str) -> bool {
    let parts = |v: &str| -> Vec<u32> { v.split('.').map(|p| p.parse().unwrap_or(0)).collect() };
    parts(a) > parts(b)
}

fn short(sha: &str) -> &str {
    sha.get(..9).unwrap_or(sha)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::web::Commit;
    use std::collections::BTreeMap;
    use std::fmt::Write as _;

    /// A network that answers from tables and fails on anything else.
    #[derive(Default)]
    struct Table {
        pages: BTreeMap<String, String>,
        refs: BTreeMap<String, String>,
        heads: BTreeMap<String, (String, String)>,
    }

    impl Web for Table {
        fn get(&self, url: &str) -> Result<String, String> {
            self.pages
                .get(url)
                .cloned()
                .ok_or_else(|| format!("no page {url}"))
        }
        fn refs(&self, slug: &str) -> Result<Vec<Ref>, String> {
            Ok(crate::web::parse_ls_remote(&self.refs[slug]))
        }
        fn commit_at(&self, slug: &str, branch: &str, _: Option<&str>) -> Result<Commit, String> {
            let (sha, date) = self.heads[&format!("{slug} {branch}")].clone();
            Ok(Commit { sha, date })
        }
    }

    fn sha(c: char) -> String {
        c.to_string().repeat(40)
    }

    fn td(cells: [&str; 4]) -> String {
        let mut row = String::from("<tr>");
        for c in cells {
            let _ = write!(row, "<td class=\"t\"><p class=\"t\">{c}</p></td>");
        }
        row + "</tr>\n"
    }

    #[allow(clippy::too_many_lines)]
    fn world() -> Table {
        let mut t = Table::default();
        let fb = "https://raw.githubusercontent.com/freebsd/freebsd-src";
        let nb = "https://raw.githubusercontent.com/NetBSD/src";
        t.refs.insert(
            FREEBSD_REPO.into(),
            format!(
                "{a}\trefs/heads/main\n{b}\trefs/heads/stable/15\n{c}\trefs/heads/stable/14\n\
                 {d}\trefs/heads/releng/15.2\n{x}\trefs/heads/releng/2.0\n\
                 {x}\trefs/tags/release/15.1.0\n{e}\trefs/tags/release/15.1.0^{{}}\n\
                 {f}\trefs/tags/release/14.5.0\n{g}\trefs/tags/release/14.4.0\n",
                a = sha('a'),
                b = sha('b'),
                c = sha('c'),
                d = sha('d'),
                e = sha('e'),
                f = sha('f'),
                g = sha('9'),
                x = sha('0'),
            ),
        );
        for (branch, c, version) in [
            ("main", 'a', ("16.0", "CURRENT")),
            ("stable/15", 'b', ("15.1", "STABLE")),
            ("stable/14", 'c', ("14.5", "STABLE")),
            ("releng/15.2", 'd', ("15.2", "BETA1")),
        ] {
            t.heads.insert(
                format!("{FREEBSD_REPO} {branch}"),
                (sha(c), format!("2026-11-0{}T00:00:00Z", 1 + t.heads.len())),
            );
            t.pages.insert(
                format!("{fb}/{}/sys/conf/newvers.sh", sha(c)),
                format!("REVISION=\"{}\"\nBRANCH=\"{}\"\n", version.0, version.1),
            );
        }
        t.pages.insert(
            FREEBSD_SECURITY.into(),
            format!(
                "<h2 id=\"sup\"></h2><table><tbody>\n{}{}{}{}{}</tbody></table>",
                td(["stable/15", "n/a", "n/a", "December 31, 2029"]),
                td([
                    "releng/15.1",
                    "15.1-RELEASE",
                    "June 16, 2026",
                    "March 31, 2027"
                ]),
                td(["stable/14", "n/a", "n/a", "November 30, 2028"]),
                td([
                    "releng/14.5",
                    "14.5-RELEASE",
                    "September 8, 2026",
                    "June 30, 2027"
                ]),
                td([
                    "releng/14.4",
                    "14.4-RELEASE",
                    "March 10, 2026",
                    "December 31, 2026"
                ]),
            ),
        );
        for (v, c) in [("15.1", '1'), ("14.5", '2'), ("14.4", '3')] {
            t.pages.insert(
                format!("{FREEBSD_RELEASES}/{v}-RELEASE/MANIFEST"),
                format!("src.txz\t{}\t1\tsrc\t\"\"\toff\n", c.to_string().repeat(64)),
            );
        }

        t.refs.insert(
            NETBSD_REPO.into(),
            format!(
                "{}\trefs/heads/trunk\n{}\trefs/heads/netbsd-11\n{}\trefs/heads/netbsd-10\n",
                sha('4'),
                sha('5'),
                sha('6')
            ),
        );
        for (branch, c, version, gcc) in [
            ("trunk", '4', "11.99.10", "14.4.0"),
            ("netbsd-11", '5', "11.1_STABLE", "12.5.0"),
            ("netbsd-10", '6', "10.2_STABLE", "10.5.0"),
        ] {
            t.heads.insert(
                format!("{NETBSD_REPO} {branch}"),
                (sha(c), "2026-11-04T00:00:00Z".into()),
            );
            t.pages.insert(
                format!("{nb}/{}/sys/sys/param.h", sha(c)),
                format!("#define\t__NetBSD_Version__\t1\t/* NetBSD {version} */\n"),
            );
            t.pages.insert(
                format!("{nb}/{}/{NETBSD_BASE_VER}", sha(c)),
                format!("{gcc}\n"),
            );
        }
        t.pages.insert(
            format!("{NETBSD_RELEASES}/"),
            "<div class=\"sect1\" id=\"supported\">\
             <a href=\"formal-11/NetBSD-11.1.html\">11.1</a>\
             <a href=\"formal-10/NetBSD-10.2.html\">10.2</a></div>"
                .into(),
        );
        for (major, v, date) in [
            ("11", "11.1", "November 2, 2026"),
            ("10", "10.2", "September 15, 2026"),
        ] {
            t.pages.insert(
                format!("{NETBSD_RELEASES}/formal-{major}/NetBSD-{v}.html"),
                format!("<title>Announcing NetBSD {v} ({date})</title>"),
            );
            t.pages.insert(
                format!("{NETBSD_CDN}/NetBSD-{v}/source/sets/SHA512"),
                "SHA512 (src.tgz) = s\nSHA512 (gnusrc.tgz) = g\n".into(),
            );
        }
        t
    }

    fn plan(latest: bool) -> Sets {
        Sets::parse(&format!(
            "[freebsd.current]\nbranches = [\"main\", \"stable/15\", \"stable/14\"]\nreleng_in_progress = true\nnightly = true\n\
             [freebsd.releases]\nsupported_only = true\nlatest_per_major = {latest}\nweekly = true\n\
             [netbsd.current]\nbranches = [\"trunk\", \"netbsd-11\", \"netbsd-10\"]\nnightly = true\n\
             [netbsd.releases]\nsupported_only = true\nweekly = true\n\
             [pin_policy]\nhead = \"last-g-good\"\n"
        ))
        .unwrap()
    }

    #[test]
    fn the_sets_of_the_plan_expand_to_its_pins() {
        let out = expand(
            &plan(true),
            &world(),
            "2026-11-08T06:00:00Z",
            None,
            None,
            &[],
        )
        .unwrap();
        let names: Vec<&str> = out.file.pin.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "freebsd-main",
                "freebsd-stable-15",
                "freebsd-stable-14",
                "freebsd-releng-15.2",
                "freebsd-15.1",
                "freebsd-14.5",
                "netbsd-trunk",
                "netbsd-11",
                "netbsd-10",
                "netbsd-11.1",
                "netbsd-10.2",
            ]
        );
        let main = out.file.find("freebsd-main").unwrap();
        assert_eq!(main.version, "16.0-CURRENT");
        assert_eq!(main.series, "16");
        assert_eq!(main.reference.as_deref(), Some("untried"));
        let releng = out.file.find("freebsd-releng-15.2").unwrap();
        assert_eq!(releng.version, "15.2-BETA1");
        let rel = out.file.find("freebsd-15.1").unwrap();
        assert_eq!(rel.commit.as_deref(), Some(sha('e').as_str()));
        assert_eq!(rel.date, "2026-06-16");
        assert_eq!(rel.eol.as_deref(), Some("2027-03-31"));
        assert_eq!(
            rel.tarball[0].sha256.as_deref(),
            Some("1".repeat(64).as_str())
        );
        let nb = out.file.find("netbsd-11.1").unwrap();
        assert_eq!(nb.gcc.as_deref(), Some("12.5.0"));
        assert_eq!(nb.gcc_from.as_deref(), Some("netbsd-11"));
        assert_eq!(nb.date, "2026-11-02");
        assert_eq!(nb.tarball.len(), 2);
        assert_eq!(out.file.find("netbsd-trunk").unwrap().version, "11.99.10");
        assert!(
            out.notes.iter().any(|n| n.contains("14.4")),
            "{:?}",
            out.notes
        );
    }

    #[test]
    fn without_latest_per_major_every_supported_release_is_pinned() {
        let out = expand(
            &plan(false),
            &world(),
            "2026-11-08T06:00:00Z",
            None,
            None,
            &[],
        )
        .unwrap();
        assert!(out.file.find("freebsd-14.4").is_some());
    }

    #[test]
    fn a_good_pin_stays_and_a_gone_pin_is_kept() {
        let first = expand(
            &plan(true),
            &world(),
            "2026-11-08T06:00:00Z",
            None,
            None,
            &[],
        )
        .unwrap();
        let mut old = first.file.clone();
        let main = old
            .pin
            .iter_mut()
            .find(|p| p.name == "freebsd-main")
            .unwrap();
        main.commit = Some(sha('7'));
        main.date = "2026-10-30T00:00:00Z".into();
        main.reference = Some("good".into());
        old.pin
            .iter_mut()
            .find(|p| p.name == "netbsd-trunk")
            .unwrap()
            .gcc = Some("14.3.0".into());
        old.pin.push(Pin {
            name: "freebsd-13.5".into(),
            system: "freebsd".into(),
            version: "13.5-RELEASE".into(),
            ..Pin::default()
        });
        let mut world = world();
        world.pages.insert(
            format!(
                "https://raw.githubusercontent.com/freebsd/freebsd-src/{}/sys/conf/newvers.sh",
                sha('7')
            ),
            "REVISION=\"16.0\"\nBRANCH=\"CURRENT\"\n".into(),
        );
        let second = expand(
            &plan(true),
            &world,
            "2026-11-09T06:00:00Z",
            None,
            Some(&old),
            &[],
        )
        .unwrap();
        let main = second.file.find("freebsd-main").unwrap();
        assert_eq!(main.commit.as_deref(), Some(sha('7').as_str()));
        assert_eq!(main.head.as_deref(), Some(sha('a').as_str()));
        assert_eq!(main.reference.as_deref(), Some("good"));
        assert_eq!(second.file.gone.len(), 1);
        assert_eq!(second.file.gone[0].left, "2026-11-09");
        assert!(
            second
                .notes
                .iter()
                .any(|n| n.starts_with("persona review: netbsd-trunk"))
        );
        assert!(second.notes.iter().any(|n| n.contains("last good commit")));

        let third = expand(
            &plan(true),
            &world,
            "2026-11-10T06:00:00Z",
            None,
            Some(&second.file),
            &[],
        )
        .unwrap();
        assert!(third.file.same_pins(&second.file));
    }

    #[test]
    fn a_complete_baseline_makes_a_commit_good() {
        let good = |commit: String| Good {
            pin: "freebsd-main".into(),
            commit,
            made: "2026-11-08T05:00:00Z".into(),
            place: "freebsd/amd64/main/graded".into(),
        };
        let mut w = world();
        w.pages.insert(
            format!(
                "https://raw.githubusercontent.com/freebsd/freebsd-src/{}/sys/conf/newvers.sh",
                sha('7')
            ),
            "REVISION=\"16.0\"\nBRANCH=\"CURRENT\"\n".into(),
        );
        w.heads.insert(
            format!("{FREEBSD_REPO} {}", sha('7')),
            (sha('7'), "2026-10-30T00:00:00Z".into()),
        );
        let at =
            |g: &[Good]| expand(&plan(true), &w, "2026-11-08T06:00:00Z", None, None, g).unwrap();

        let out = at(&[good(sha('7'))]);
        let main = out.file.find("freebsd-main").unwrap();
        assert_eq!(main.commit.as_deref(), Some(sha('7').as_str()));
        assert_eq!(main.date, "2026-10-30T00:00:00Z");
        assert_eq!(main.head.as_deref(), Some(sha('a').as_str()));
        assert_eq!(main.reference.as_deref(), Some("good"));
        assert!(
            out.notes
                .iter()
                .any(|n| n.contains("baselines/freebsd/amd64/main/graded"))
        );
        let stable = out.file.find("freebsd-stable-15").unwrap();
        assert_eq!(stable.reference.as_deref(), Some("untried"));

        let out = at(&[good(sha('a'))]);
        let main = out.file.find("freebsd-main").unwrap();
        assert_eq!(main.commit.as_deref(), Some(sha('a').as_str()));
        assert_eq!(main.reference.as_deref(), Some("good"));

        let mut wrong = world();
        wrong.heads.insert(
            format!("{FREEBSD_REPO} {}", sha('7')),
            (sha('8'), "2026-10-30T00:00:00Z".into()),
        );
        let err = expand(
            &plan(true),
            &wrong,
            "2026-11-08T06:00:00Z",
            None,
            None,
            &[good(sha('7'))],
        );
        assert!(err.is_err());
    }

    #[test]
    fn a_no_ref_branch_is_at_its_head_with_the_reason() {
        let mut sets = plan(true);
        sets.freebsd
            .current
            .no_ref
            .insert("stable/14".into(), "no G-good commit".into());
        let good = Good {
            pin: "freebsd-stable-14".into(),
            commit: sha('7'),
            made: "2026-11-08T05:00:00Z".into(),
            place: "freebsd/amd64/stable-14/graded".into(),
        };
        let out = expand(&sets, &world(), "2026-11-08T06:00:00Z", None, None, &[good]).unwrap();
        let old = out.file.find("freebsd-stable-14").unwrap();
        assert_eq!(old.reference.as_deref(), Some("no-ref"));
        assert_eq!(old.reason.as_deref(), Some("no G-good commit"));
        assert_eq!(old.commit, old.head);
        assert_eq!(old.series, "14");
        assert!(
            out.notes
                .iter()
                .any(|n| n == "freebsd-stable-14 has no reference: no G-good commit")
        );
        let main = out.file.find("freebsd-main").unwrap();
        assert_eq!(main.reference.as_deref(), Some("untried"));
        assert_eq!(main.reason, None);
    }
}
