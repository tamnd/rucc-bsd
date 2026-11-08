//! `rb fetch` against a local git repository and local tarballs, through file:// URLs.
#![cfg(unix)]

use rb_pins::fetch::{MARK, Way, fetch, sha256, sha512};
use rb_pins::pins::{Pin, Tarball};
use std::path::{Path, PathBuf};
use std::process::Command;

struct Scratch(PathBuf);

impl Scratch {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("rb-fetch-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn sh(dir: &Path, line: &str) -> String {
    let out = Command::new("sh")
        .arg("-c")
        .arg(line)
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@example.org")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@example.org")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{line}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// A FreeBSD-like repository with two commits. Returns the repository and both commits.
fn repo(root: &Path) -> (PathBuf, String, String) {
    let repo = root.join("src");
    std::fs::create_dir_all(repo.join("sys/conf")).unwrap();
    sh(
        &repo,
        "git init -q -b main . && git config uploadpack.allowAnySHA1InWant true",
    );
    std::fs::write(
        repo.join("sys/conf/newvers.sh"),
        "TYPE=\"FreeBSD\"\nREVISION=\"15.1\"\nBRANCH=\"RELEASE\"\n",
    )
    .unwrap();
    sh(&repo, "git add -A && git commit -q -m one");
    let first = sh(&repo, "git rev-parse HEAD");
    std::fs::write(
        repo.join("sys/conf/newvers.sh"),
        "TYPE=\"FreeBSD\"\nREVISION=\"16.0\"\nBRANCH=\"CURRENT\"\n",
    )
    .unwrap();
    sh(&repo, "git commit -q -a -m two");
    let second = sh(&repo, "git rev-parse HEAD");
    (repo, first, second)
}

fn git_pin(repo: &Path, commit: &str, version: &str) -> Pin {
    Pin {
        name: "freebsd-15.1".into(),
        system: "freebsd".into(),
        version: version.into(),
        repo: Some(format!("file://{}", repo.display())),
        commit: Some(commit.into()),
        ..Pin::default()
    }
}

#[test]
fn a_commit_that_is_not_the_head_is_fetched_and_checked() {
    let s = Scratch::new("git");
    let (repo, first, second) = repo(&s.0);
    let cache = s.0.join("cache");
    let pin = git_pin(&repo, &first, "15.1-RELEASE");
    let got = fetch(&pin, &cache, Way::Git).unwrap();
    assert!(!got.cached);
    assert_eq!(sh(&got.dir, "git rev-parse HEAD"), first);
    assert!(got.dir.join(MARK).is_file());
    assert_ne!(first, second);

    let again = fetch(&pin, &cache, Way::Git).unwrap();
    assert!(again.cached);
}

#[test]
fn a_tree_whose_version_differs_from_the_pin_is_refused_and_not_kept() {
    let s = Scratch::new("version");
    let (repo, _, second) = repo(&s.0);
    let cache = s.0.join("cache");
    let pin = git_pin(&repo, &second, "15.1-RELEASE");
    let err = fetch(&pin, &cache, Way::Git).unwrap_err();
    assert!(err.contains("16.0-CURRENT"), "{err}");
    let trees = cache.join("trees/freebsd-15.1");
    assert!(!trees.join(&second).exists());
    assert!(!trees.join(format!("{second}.part")).exists());
}

#[test]
fn a_commit_the_repository_does_not_have_is_an_error() {
    let s = Scratch::new("missing");
    let (repo, _, _) = repo(&s.0);
    let pin = git_pin(&repo, &"0".repeat(40), "15.1-RELEASE");
    assert!(fetch(&pin, &s.0.join("cache"), Way::Git).is_err());
}

/// Two NetBSD-like source sets that unpack into one usr/src.
fn sets(root: &Path) -> Vec<Tarball> {
    let stage = root.join("stage");
    std::fs::create_dir_all(stage.join("a/usr/src/sys/sys")).unwrap();
    std::fs::create_dir_all(stage.join("b/usr/src/bin")).unwrap();
    std::fs::write(
        stage.join("a/usr/src/sys/sys/param.h"),
        "#define\t__NetBSD_Version__\t1101000000\t/* NetBSD 11.1 */\n",
    )
    .unwrap();
    std::fs::write(stage.join("b/usr/src/bin/README"), "bin\n").unwrap();
    let mut out = Vec::new();
    for (name, part) in [("syssrc.tgz", "a"), ("src.tgz", "b")] {
        let file = root.join(name);
        sh(
            &stage.join(part),
            &format!("tar -czf {} usr", file.display()),
        );
        out.push(Tarball {
            name: name.into(),
            url: format!("file://{}", file.display()),
            sha512: Some(sha512(&file).unwrap()),
            ..Tarball::default()
        });
    }
    out
}

#[test]
fn source_sets_are_checked_and_unpacked_into_one_tree() {
    let s = Scratch::new("tar");
    let pin = Pin {
        name: "netbsd-11.1".into(),
        system: "netbsd".into(),
        version: "11.1".into(),
        tarball: sets(&s.0),
        ..Pin::default()
    };
    let cache = s.0.join("cache");
    let got = fetch(&pin, &cache, Way::Tarball).unwrap();
    assert_eq!(got.dir, cache.join("trees/netbsd-11.1/tarball-11.1"));
    assert!(got.src.join("sys/sys/param.h").is_file());
    assert!(got.src.join("bin/README").is_file());
    assert!(cache.join("downloads/netbsd-11.1/src.tgz").is_file());
    assert!(fetch(&pin, &cache, Way::Tarball).unwrap().cached);
}

#[test]
fn a_tarball_with_the_wrong_digest_is_refused_and_deleted() {
    let s = Scratch::new("digest");
    let mut tarballs = sets(&s.0);
    tarballs[1].sha512 = Some("0".repeat(128));
    let pin = Pin {
        name: "netbsd-11.1".into(),
        system: "netbsd".into(),
        version: "11.1".into(),
        tarball: tarballs,
        ..Pin::default()
    };
    let cache = s.0.join("cache");
    let err = fetch(&pin, &cache, Way::Tarball).unwrap_err();
    assert!(
        err.contains("src.tgz") && err.contains("publishes"),
        "{err}"
    );
    assert!(!cache.join("downloads/netbsd-11.1/src.tgz").exists());
    assert!(!cache.join("downloads/netbsd-11.1/src.tgz.part").exists());
    assert!(!cache.join("trees/netbsd-11.1/tarball-11.1.part").exists());

    let file = s.0.join("syssrc.tgz");
    assert_eq!(sha256(&file).unwrap().len(), 64);
}
