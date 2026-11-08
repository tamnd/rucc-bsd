//! What `rb` reads from the network: web pages, git refs, commits and files at a commit.
//!
//! The work goes through the [`Web`] trait so that tests can answer from a table. [`Curl`] is the
//! real one. It runs `curl` and `git`, which every builder has, so `rb` needs no TLS library.

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
        let mut path = format!("repos/{slug}/commits?sha={branch}&per_page=1");
        if let Some(until) = until {
            path.push_str("&until=");
            path.push_str(until);
        }
        let value = self.api(&path)?;
        let first = value
            .as_array()
            .and_then(|a| a.first())
            .ok_or_else(|| format!("{slug} has no commit on {branch} at {until:?}"))?;
        let sha = first["sha"].as_str().unwrap_or_default().to_string();
        let date = first["commit"]["committer"]["date"]
            .as_str()
            .unwrap_or_default()
            .to_string();
        if sha.len() != 40 || date.is_empty() {
            return Err(format!("{slug} {branch}: the API gave no sha or date"));
        }
        Ok(Commit { sha, date })
    }
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
