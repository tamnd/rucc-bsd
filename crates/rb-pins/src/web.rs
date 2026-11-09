//! What `rb` reads from the network: web pages, git refs, commits and files at a commit.
//!
//! The work goes through the [`Web`] trait so that tests can answer from a table. [`Curl`] is the
//! real one. It runs `curl` and `git`, which every builder has, so `rb` needs no TLS library.

use std::collections::HashMap;
use std::io::Write;
use std::process::{Command, Stdio};

/// A commit and its committer date.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Commit {
    /// The full hash.
    pub sha: String,
    /// The committer date, as GitHub gives it.
    pub date: String,
}

/// One ref of a remote, with tags peeled.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ref {
    /// The full ref name, such as `refs/tags/release/15.1.0`.
    pub name: String,
    /// The commit it names. For an annotated tag this is the peeled commit.
    pub sha: String,
}

/// The network, as `rb sets` and `rb fetch` see it.
pub trait Web {
    /// The body of a URL.
    fn get(&self, url: &str) -> Result<String, String>;
    /// The branches and tags of a GitHub repository, given as `owner/name`.
    fn refs(&self, slug: &str) -> Result<Vec<Ref>, String>;
    /// The newest commit on `branch` at or before `until`, or the head when `until` is `None`.
    fn commit_at(&self, slug: &str, branch: &str, until: Option<&str>) -> Result<Commit, String>;
    /// A file of a GitHub repository at a commit.
    fn file(&self, slug: &str, sha: &str, path: &str) -> Result<String, String> {
        self.get(&format!(
            "https://raw.githubusercontent.com/{slug}/{sha}/{path}"
        ))
    }
}

/// The network through `curl` and `git`. `GITHUB_TOKEN`, if set, goes to the GitHub API on
/// standard input, so it is not on a command line.
#[derive(Debug, Clone, Default)]
pub struct Curl {
    /// The token for the GitHub API.
    pub token: Option<String>,
}

impl Curl {
    /// Read the token from the environment.
    #[must_use]
    pub fn from_env() -> Self {
        Self {
            token: std::env::var("GITHUB_TOKEN").ok().filter(|t| !t.is_empty()),
        }
    }

    fn curl(url: &str, headers: &[String]) -> Result<String, String> {
        let mut command = Command::new("curl");
        command.args([
            "-fsSL",
            "--retry",
            "3",
            "--retry-all-errors",
            "--max-time",
            "120",
        ]);
        if !headers.is_empty() {
            command.args(["-H", "@-"]);
        }
        command
            .arg(url)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = command
            .spawn()
            .map_err(|e| format!("cannot run curl: {e}"))?;
        if let Some(mut stdin) = child.stdin.take() {
            for header in headers {
                writeln!(stdin, "{header}").map_err(|e| format!("curl: {e}"))?;
            }
        }
        let out = child.wait_with_output().map_err(|e| format!("curl: {e}"))?;
        if !out.status.success() {
            let err = String::from_utf8_lossy(&out.stderr);
            return Err(format!("curl {url}: {}", err.trim()));
        }
        String::from_utf8(out.stdout).map_err(|_| format!("{url} is not UTF-8"))
    }

    fn api(&self, path: &str) -> Result<serde_json::Value, String> {
        let mut headers = vec![
            "Accept: application/vnd.github+json".to_string(),
            "X-GitHub-Api-Version: 2022-11-28".to_string(),
        ];
        if let Some(token) = &self.token {
            headers.push(format!("Authorization: Bearer {token}"));
        }
        let url = format!("https://api.github.com/{path}");
        let body = Self::curl(&url, &headers)?;
        serde_json::from_str(&body).map_err(|e| format!("{url}: {e}"))
    }
}

impl Web for Curl {
    fn get(&self, url: &str) -> Result<String, String> {
        Self::curl(url, &[])
    }

    fn refs(&self, slug: &str) -> Result<Vec<Ref>, String> {
        let url = format!("https://github.com/{slug}");
        let out = Command::new("git")
            .args(["ls-remote", "--heads", "--tags", &url])
            .output()
            .map_err(|e| format!("cannot run git: {e}"))?;
        if !out.status.success() {
            return Err(format!(
                "git ls-remote {url}: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            ));
        }
        Ok(parse_ls_remote(&String::from_utf8_lossy(&out.stdout)))
    }

    fn commit_at(&self, slug: &str, branch: &str, until: Option<&str>) -> Result<Commit, String> {
        let base = format!("repos/{slug}/commits?sha={branch}&per_page=100");
        first_parent_at(until, |page| self.api(&format!("{base}&page={page}")))
            .map_err(|e| format!("{slug} {branch}: {e}"))
    }
}

/// How many pages of 100 commits [`first_parent_at`] reads before it gives up.
pub const PAGES: usize = 30;

/// The newest commit at or before `until` on the first-parent line from the head of a branch, or
/// the head when `until` is `None`. `page(n)` gives page `n` of the GitHub list of the branch's
/// commits, newest first, 100 to a page. GitHub lists every ancestor by date, so a commit of a
/// side branch merged later, such as a FreeBSD vendor import, can come before the commit that was
/// the head at `until`. The first-parent line leaves it out, so the same `until` gives the same
/// commit after later merges.
pub fn first_parent_at(
    until: Option<&str>,
    mut page: impl FnMut(usize) -> Result<serde_json::Value, String>,
) -> Result<Commit, String> {
    let mut known: HashMap<String, (String, Option<String>)> = HashMap::new();
    let mut at: Option<String> = None;
    for n in 1..=PAGES {
        let value = page(n)?;
        let list = value.as_array().ok_or("the API gave no list of commits")?;
        if list.is_empty() {
            break;
        }
        for c in list {
            let sha = c["sha"].as_str().unwrap_or_default().to_string();
            let date = c["commit"]["committer"]["date"]
                .as_str()
                .unwrap_or_default()
                .to_string();
            if sha.len() != 40 || date.is_empty() {
                return Err("the API gave no sha or date".into());
            }
            let parent = c["parents"]
                .get(0)
                .and_then(|p| p["sha"].as_str())
                .map(str::to_string);
            at.get_or_insert_with(|| sha.clone());
            known.insert(sha, (date, parent));
        }
        while let Some(sha) = at.clone() {
            let Some((date, parent)) = known.get(&sha) else {
                break;
            };
            if until.is_none_or(|u| date.as_str() <= u) {
                return Ok(Commit {
                    sha,
                    date: date.clone(),
                });
            }
            at.clone_from(parent);
        }
        if at.is_none() {
            break;
        }
    }
    Err(format!(
        "no commit at or before {} in {PAGES} pages",
        until.unwrap_or("now")
    ))
}

/// Parse `git ls-remote` output. A peeled tag (`^{}`) replaces the tag object it follows.
#[must_use]
pub fn parse_ls_remote(text: &str) -> Vec<Ref> {
    let mut refs: Vec<Ref> = Vec::new();
    for line in text.lines() {
        let Some((sha, name)) = line.split_once('\t') else {
            continue;
        };
        if let Some(tag) = name.strip_suffix("^{}") {
            if let Some(r) = refs.iter_mut().rev().find(|r| r.name == tag) {
                r.sha = sha.to_string();
                continue;
            }
            refs.push(Ref {
                name: tag.to_string(),
                sha: sha.to_string(),
            });
            continue;
        }
        refs.push(Ref {
            name: name.to_string(),
            sha: sha.to_string(),
        });
    }
    refs
}

#[cfg(test)]
mod tests {
    use super::*;

    fn commit(sha: char, date: &str, parent: Option<char>) -> serde_json::Value {
        serde_json::json!({
            "sha": sha.to_string().repeat(40),
            "commit": { "committer": { "date": date } },
            "parents": parent.map_or_else(Vec::new, |p| vec![serde_json::json!({ "sha": p.to_string().repeat(40) })]),
        })
    }

    #[test]
    fn the_commit_at_a_time_is_on_the_first_parent_line() {
        // a merges the side branch v into the line b, c. v is newer than b and c, and older
        // than a, so GitHub lists it second.
        let pages = [
            serde_json::json!([
                commit('a', "2026-11-08T10:00:00Z", Some('b')),
                commit('v', "2026-11-08T05:00:00Z", Some('w')),
            ]),
            serde_json::json!([
                commit('b', "2026-11-08T04:00:00Z", Some('c')),
                commit('c', "2026-11-07T00:00:00Z", None),
            ]),
            serde_json::json!([]),
        ];
        let at = |until| first_parent_at(until, |n| Ok(pages[n - 1].clone()));
        let sha = |c: char| c.to_string().repeat(40);
        assert_eq!(at(None).unwrap().sha, sha('a'));
        assert_eq!(at(Some("2026-11-08T06:00:00Z")).unwrap().sha, sha('b'));
        assert_eq!(
            at(Some("2026-11-08T04:00:00Z")).unwrap().date,
            "2026-11-08T04:00:00Z"
        );
        assert_eq!(at(Some("2026-11-07T12:00:00Z")).unwrap().sha, sha('c'));
        assert!(at(Some("2026-11-01T00:00:00Z")).is_err());
    }

    #[test]
    fn a_peeled_tag_names_its_commit() {
        let refs = parse_ls_remote(
            "aaa\trefs/heads/main\n\
             bbb\trefs/tags/release/15.1.0\n\
             ccc\trefs/tags/release/15.1.0^{}\n\
             ddd\trefs/tags/light\n",
        );
        assert_eq!(refs.len(), 3);
        assert_eq!(refs[1].name, "refs/tags/release/15.1.0");
        assert_eq!(refs[1].sha, "ccc");
        assert_eq!(refs[2].sha, "ddd");
    }
}
