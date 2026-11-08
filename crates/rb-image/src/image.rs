//! `rb image`: a bootable raw disk image from a build (plan 12.3).
//!
//! The input is what a G or R build leaves behind:
//! - **FreeBSD:** the staging tree of `installworld`, `installkernel` and `distribution`. rb writes
//!   its files into the tree and makes the disk with `makefs` and `mkimg` ([`crate::freebsd`]).
//! - **NetBSD:** the `TOOLDIR` of `build.sh`, or its `nbmake-<machine>` wrapper. rb runs the
//!   project's own `live-image` target with its files added ([`crate::netbsd`]).
//!
//! Both images carry rb-rc, the smoke suite, an `rb.conf` and, when given, a local package
//! repository at `/var/db/rb-pkg`. Both use the serial console.

use crate::conf::RbConf;
use crate::guest::GuestFile;
use crate::plan::Step;
use std::path::{Path, PathBuf};

/// The two systems.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum System {
    /// FreeBSD.
    Freebsd,
    /// NetBSD.
    Netbsd,
}

impl System {
    /// Read a system name.
    pub fn parse(name: &str) -> Result<Self, String> {
        match name {
            "freebsd" => Ok(System::Freebsd),
            "netbsd" => Ok(System::Netbsd),
            _ => Err(format!("unknown system {name}, give freebsd or netbsd")),
        }
    }

    /// The name, as in `rows.toml`.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            System::Freebsd => "freebsd",
            System::Netbsd => "netbsd",
        }
    }

    /// Which system a build directory is from: a NetBSD `TOOLDIR` has `bin/nbmake-*`, and a
    /// FreeBSD staging tree has `boot/loader.efi` or `bin/freebsd-version`.
    pub fn detect(build: &Path) -> Result<Self, String> {
        let is_wrapper = |p: &Path| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("nbmake-"))
        };
        if build.is_file() && is_wrapper(build) {
            return Ok(System::Netbsd);
        }
        if let Ok(dir) = std::fs::read_dir(build.join("bin"))
            && dir.flatten().any(|e| is_wrapper(&e.path()))
        {
            return Ok(System::Netbsd);
        }
        if build.join("boot/loader.efi").is_file() || build.join("bin/freebsd-version").is_file() {
            return Ok(System::Freebsd);
        }
        Err(format!(
            "{} is neither a NetBSD TOOLDIR (bin/nbmake-*) nor a FreeBSD staging tree (boot/loader.efi); give --system",
            build.display()
        ))
    }
}

/// What to make.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    /// The staging tree (FreeBSD), or the `TOOLDIR` or wrapper (NetBSD).
    pub build: PathBuf,
    /// The image to write.
    pub out: PathBuf,
    /// The system.
    pub system: System,
    /// The `rb.conf` of the image.
    pub conf: RbConf,
    /// A directory of packages for the local repository.
    pub packages: Option<PathBuf>,
    /// The size: of the root file system on FreeBSD, of the whole disk on NetBSD. NetBSD's image
    /// grows into it at its first boot.
    pub size: Option<u64>,
    /// Where FreeBSD's `makefs` and `mkimg` are, when they are not on `PATH`.
    pub tools: Option<PathBuf>,
    /// The scratch directory, with a log per step.
    pub work: PathBuf,
}

/// What was made, or would be.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Made {
    /// The image.
    pub out: PathBuf,
    /// Its size in bytes. Zero for a dry run.
    pub bytes: u64,
    /// The commands, as shell lines.
    pub script: String,
    /// The files rb added, relative to the image's root.
    pub added: Vec<String>,
}

/// Where the local package repository goes in the image.
pub const PKG_DIR: &str = "var/db/rb-pkg";

/// The lines rb adds to a configuration file go between these two.
pub const BEGIN: &str = "# rb image: begin";
/// See [`BEGIN`].
pub const END: &str = "# rb image: end";

/// A file with rb's block in it. An older rb block is replaced, so making an image twice from one
/// tree gives the same file.
#[must_use]
pub fn with_block(existing: &str, block: &str) -> String {
    let mut out = String::new();
    let mut inside = false;
    for line in existing.lines() {
        if line == BEGIN {
            inside = true;
        } else if line == END {
            inside = false;
        } else if !inside {
            out.push_str(line);
            out.push('\n');
        }
    }
    out.push_str(BEGIN);
    out.push('\n');
    out.push_str(block);
    if !block.ends_with('\n') {
        out.push('\n');
    }
    out.push_str(END);
    out.push('\n');
    out
}

/// The package files of a repository directory, relative to it, sorted. Subdirectories such as
/// FreeBSD's `All` are kept.
pub fn package_files(dir: &Path) -> Result<Vec<String>, String> {
    fn walk(top: &Path, dir: &Path, out: &mut Vec<String>) -> Result<(), String> {
        let entries = std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        for e in entries.flatten() {
            let path = e.path();
            if path.is_dir() {
                walk(top, &path, out)?;
            } else if let Ok(rel) = path.strip_prefix(top) {
                out.push(rel.display().to_string());
            }
        }
        Ok(())
    }
    let mut out = Vec::new();
    walk(dir, dir, &mut out)?;
    out.sort();
    Ok(out)
}

/// The directories that hold `files`, parents first, leaving out the ones every image has.
#[must_use]
pub fn new_dirs(files: &[String], existing: &[&str]) -> Vec<String> {
    let mut dirs = Vec::new();
    for f in files {
        let mut p = Path::new(f).parent();
        let mut chain = Vec::new();
        while let Some(d) = p {
            let s = d.display().to_string();
            if s.is_empty() || existing.contains(&s.as_str()) {
                break;
            }
            chain.push(s);
            p = d.parent();
        }
        for d in chain.into_iter().rev() {
            if !dirs.contains(&d) {
                dirs.push(d);
            }
        }
    }
    dirs
}

/// The directories every FreeBSD and NetBSD root has, which rb's files go under.
pub const BASE_DIRS: &[&str] = &[
    "etc",
    "etc/rc.d",
    "boot",
    "usr",
    "usr/libexec",
    "var",
    "var/db",
];

/// Copy the package repository under `root`, and return the paths it added.
pub fn copy_packages(src: &Path, root: &Path) -> Result<Vec<String>, String> {
    let mut added = Vec::new();
    for rel in package_files(src)? {
        let to = root.join(PKG_DIR).join(&rel);
        if let Some(parent) = to.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
        }
        std::fs::copy(src.join(&rel), &to).map_err(|e| format!("{}: {e}", to.display()))?;
        added.push(format!("{PKG_DIR}/{rel}"));
    }
    Ok(added)
}

/// Make the image, or with `dry_run` only say how.
pub fn make(req: &Request, dry_run: bool) -> Result<Made, String> {
    match req.system {
        System::Freebsd => crate::freebsd::make(req, dry_run),
        System::Netbsd => crate::netbsd::make(req, dry_run),
    }
}

/// The paths of files, for a report.
pub(crate) fn paths(files: &[GuestFile]) -> Vec<String> {
    files.iter().map(|f| f.path.to_string()).collect()
}

/// Finish: the steps' result at `tmp` becomes `out`.
pub(crate) fn finish(
    tmp: &Path,
    out: &Path,
    script: &[Step],
    added: Vec<String>,
) -> Result<Made, String> {
    std::fs::rename(tmp, out)
        .map_err(|e| format!("{} to {}: {e}", tmp.display(), out.display()))?;
    let bytes = std::fs::metadata(out)
        .map_err(|e| format!("{}: {e}", out.display()))?
        .len();
    Ok(Made {
        out: out.to_path_buf(),
        bytes,
        script: crate::plan::script(script),
        added,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks() {
        let once = with_block("hostname=x\n", "rb_enable=\"YES\"\n");
        assert_eq!(
            once,
            format!("hostname=x\n{BEGIN}\nrb_enable=\"YES\"\n{END}\n")
        );
        let twice = with_block(&once, "rb_enable=\"YES\"");
        assert_eq!(twice, once);
        assert_eq!(with_block("", "a\n"), format!("{BEGIN}\na\n{END}\n"));
    }

    #[test]
    fn dirs() {
        let files = vec![
            "usr/libexec/rb/run".to_string(),
            "usr/libexec/rb/smoke".to_string(),
            "var/db/rb-pkg/All/pkg-2.0.pkg".to_string(),
            "etc/rb.conf".to_string(),
        ];
        assert_eq!(
            new_dirs(&files, BASE_DIRS),
            ["usr/libexec/rb", "var/db/rb-pkg", "var/db/rb-pkg/All"]
        );
    }

    #[test]
    fn systems() {
        let top = std::env::temp_dir().join(format!("rb-image-detect-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&top);
        std::fs::create_dir_all(top.join("tools/bin")).unwrap();
        std::fs::write(top.join("tools/bin/nbmake-amd64"), "").unwrap();
        std::fs::create_dir_all(top.join("stage/boot")).unwrap();
        std::fs::write(top.join("stage/boot/loader.efi"), "").unwrap();
        assert_eq!(System::detect(&top.join("tools")).unwrap(), System::Netbsd);
        assert_eq!(
            System::detect(&top.join("tools/bin/nbmake-amd64")).unwrap(),
            System::Netbsd
        );
        assert_eq!(System::detect(&top.join("stage")).unwrap(), System::Freebsd);
        assert!(System::detect(&top).is_err());
        assert_eq!(System::parse("netbsd").unwrap().name(), "netbsd");
        assert!(System::parse("openbsd").is_err());
        std::fs::remove_dir_all(&top).unwrap();
    }
}
