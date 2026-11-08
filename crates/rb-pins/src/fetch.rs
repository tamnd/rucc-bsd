//! `rb fetch`: put a pinned tree in the cache and prove it is the pinned one.
//!
//! A pin with a commit is fetched with git, one commit deep, and `git rev-parse HEAD` must give
//! the pinned commit. A pin with only tarballs, or any release when tarballs are asked for, is
//! downloaded with curl, checked against the digest the project publishes, and unpacked. Either
//! way the tree's own version file must agree with the pin's version. The tree is built in a
//! `.part` directory and renamed when every check passed, so a tree in the cache is a whole one.

use crate::pins::{Pin, Tarball};
use crate::publish;
use sha2::{Digest, Sha256, Sha512};
use std::fmt::Write as _;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The file in a fetched tree that says what it is.
pub const MARK: &str = ".rb-fetched";

/// How a tree was fetched.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Way {
    /// By commit, with git.
    Git,
    /// From the release tarballs.
    Tarball,
}

/// A fetched tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fetched {
    /// The directory with the tree.
    pub dir: PathBuf,
    /// The source tree inside it: `dir` for git, `dir/usr/src` for tarballs.
    pub src: PathBuf,
    /// How it was fetched.
    pub way: Way,
    /// Whether the cache had it already.
    pub cached: bool,
}

/// Choose how to fetch `pin`. Tarballs are used when asked for, or when there is no commit.
pub fn way(pin: &Pin, tarball: bool) -> Result<Way, String> {
    let has_git = pin.commit.is_some() && pin.repo.is_some();
    let has_tarballs = !pin.tarball.is_empty();
    if has_git && !tarball {
        Ok(Way::Git)
    } else if has_tarballs {
        Ok(Way::Tarball)
    } else if has_git {
        Err(format!("{} has no tarballs; fetch it by commit", pin.name))
    } else {
        Err(format!("{} has neither a commit nor tarballs", pin.name))
    }
}

/// Fetch `pin` into `cache`, unless it is there already.
pub fn fetch(pin: &Pin, cache: &Path, way: Way) -> Result<Fetched, String> {
    let id = match way {
        Way::Git => pin.commit.clone().unwrap_or_default(),
        Way::Tarball => format!("tarball-{}", pin.version),
    };
    let dir = cache.join("trees").join(&pin.name).join(&id);
    let src = match way {
        Way::Git => dir.clone(),
        Way::Tarball => dir.join("usr").join("src"),
    };
    let mark = format!(
        "pin = {:?}\nway = {:?}\nid = {:?}\n",
        pin.name,
        way_name(way),
        id
    );
    if std::fs::read_to_string(dir.join(MARK)).is_ok_and(|m| m == mark) {
        return Ok(Fetched {
            dir,
            src,
            way,
            cached: true,
        });
    }

    // Not with_extension: "tarball-11.1" would become "tarball-11.part".
    let part = cache
        .join("trees")
        .join(&pin.name)
        .join(format!("{id}.part"));
    let _ = std::fs::remove_dir_all(&part);
    std::fs::create_dir_all(&part).map_err(|e| format!("{}: {e}", part.display()))?;
    if let Err(e) = build(pin, cache, way, &part, &mark) {
        let _ = std::fs::remove_dir_all(&part);
        return Err(e);
    }
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::rename(&part, &dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    Ok(Fetched {
        dir,
        src,
        way,
        cached: false,
    })
}

/// Fill `part` with the tree and check it. The caller removes `part` when this fails.
fn build(pin: &Pin, cache: &Path, way: Way, part: &Path, mark: &str) -> Result<(), String> {
    match way {
        Way::Git => git(pin, part)?,
        Way::Tarball => {
            let downloads = cache.join("downloads").join(&pin.name);
            for t in &pin.tarball {
                let file = download(t, &downloads)?;
                untar(&file, part)?;
            }
        }
    }
    let src = match way {
        Way::Git => part.to_path_buf(),
        Way::Tarball => part.join("usr").join("src"),
    };
    check_version(pin, &src)?;
    std::fs::write(part.join(MARK), mark).map_err(|e| format!("{}: {e}", part.display()))
}

fn way_name(way: Way) -> &'static str {
    match way {
        Way::Git => "git",
        Way::Tarball => "tarball",
    }
}

fn run(command: &mut Command) -> Result<String, String> {
    let out = command
        .output()
        .map_err(|e| format!("cannot run {}: {e}", command.get_program().display()))?;
    if !out.status.success() {
        return Err(format!(
            "{:?} failed: {}",
            command,
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

fn git(pin: &Pin, dir: &Path) -> Result<(), String> {
    let commit = pin.commit.as_deref().unwrap_or_default();
    let repo = pin.repo.as_deref().unwrap_or_default();
    let git = |args: &[&str]| run(Command::new("git").arg("-C").arg(dir).args(args));
    git(&["init", "-q"])?;
    git(&["fetch", "-q", "--depth", "1", repo, commit])?;
    git(&[
        "-c",
        "advice.detachedHead=false",
        "checkout",
        "-q",
        "FETCH_HEAD",
    ])?;
    let head = git(&["rev-parse", "HEAD"])?;
    if head.trim() != commit {
        return Err(format!(
            "{}: {repo} gave {} for {commit}",
            pin.name,
            head.trim()
        ));
    }
    Ok(())
}

/// Download one tarball into `dir`, or use the copy there, and check its digest.
pub fn download(t: &Tarball, dir: &Path) -> Result<PathBuf, String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let file = dir.join(&t.name);
    if file.is_file() && verify(t, &file).is_ok() {
        return Ok(file);
    }
    let part = dir.join(format!("{}.part", t.name));
    run(Command::new("curl")
        .args(["-fsSL", "--retry", "3", "--retry-all-errors", "-o"])
        .arg(&part)
        .arg(&t.url))?;
    if let Err(e) = verify(t, &part) {
        let _ = std::fs::remove_file(&part);
        return Err(e);
    }
    std::fs::rename(&part, &file).map_err(|e| format!("{}: {e}", file.display()))?;
    Ok(file)
}

/// Check a file against the tarball's published digest.
pub fn verify(t: &Tarball, file: &Path) -> Result<(), String> {
    let (want, got) = match (&t.sha256, &t.sha512) {
        (Some(want), _) => (want, sha256(file)?),
        (None, Some(want)) => (want, sha512(file)?),
        (None, None) => return Err(format!("{} has no digest to check", t.name)),
    };
    if &got == want {
        Ok(())
    } else {
        Err(format!(
            "{}: the digest is {got}, but {} publishes {want}",
            t.name, t.url
        ))
    }
}

/// The SHA-256 of a file, in hex.
pub fn sha256(file: &Path) -> Result<String, String> {
    digest::<Sha256>(file)
}

/// The SHA-512 of a file, in hex.
pub fn sha512(file: &Path) -> Result<String, String> {
    digest::<Sha512>(file)
}

fn digest<D: Digest>(file: &Path) -> Result<String, String> {
    let mut f = std::fs::File::open(file).map_err(|e| format!("{}: {e}", file.display()))?;
    let mut hasher = D::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = f
            .read(&mut buf)
            .map_err(|e| format!("{}: {e}", file.display()))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    let mut hex = String::new();
    for b in hasher.finalize() {
        let _ = write!(hex, "{b:02x}");
    }
    Ok(hex)
}

fn untar(file: &Path, dir: &Path) -> Result<(), String> {
    run(Command::new("tar").arg("-xf").arg(file).arg("-C").arg(dir)).map(|_| ())
}

/// Check that the tree's own version file gives the pin's version.
pub fn check_version(pin: &Pin, src: &Path) -> Result<(), String> {
    let read = |rel: &str| {
        let path = src.join(rel);
        std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))
    };
    let found = match pin.system.as_str() {
        "freebsd" => publish::freebsd_newvers(&read("sys/conf/newvers.sh")?)?,
        "netbsd" => publish::netbsd_param(&read("sys/sys/param.h")?)?,
        other => return Err(format!("{}: unknown system {other}", pin.name)),
    };
    if found == pin.version {
        Ok(())
    } else {
        Err(format!(
            "{}: the tree says {found}, but the pin says {}",
            pin.name, pin.version
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_way_follows_what_the_pin_has() {
        let git = Pin {
            name: "a".into(),
            commit: Some("c".into()),
            repo: Some("r".into()),
            ..Pin::default()
        };
        let tar = Pin {
            name: "b".into(),
            tarball: vec![Tarball::default()],
            ..Pin::default()
        };
        let both = Pin {
            tarball: vec![Tarball::default()],
            ..git.clone()
        };
        assert_eq!(way(&git, false), Ok(Way::Git));
        assert!(way(&git, true).unwrap_err().contains("no tarballs"));
        assert_eq!(way(&tar, false), Ok(Way::Tarball));
        assert_eq!(way(&both, false), Ok(Way::Git));
        assert_eq!(way(&both, true), Ok(Way::Tarball));
        assert!(way(&Pin::default(), false).is_err());
    }
}
