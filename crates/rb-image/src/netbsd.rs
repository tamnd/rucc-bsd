//! NetBSD images: the project's own `build.sh live-image` path, with rb's files added (plan 12.3).
//!
//! The input is the `TOOLDIR` of a `build.sh` run, or its `nbmake-<machine>` wrapper. The
//! wrapper names the source tree, `DESTDIR`, `RELEASEDIR` and the machine. The release must
//! already have its sets, as for `build.sh live-image`. rb runs, with the wrapper:
//! 1. `obj` and `clean` in `distrib/<machine>/liveimage`, so the image is made again;
//! 2. `live-image` at the top of the tree, with these variables set on the command line:
//!    - `IMGDIR_EXTRA` copies rb's overlay tree into the image's root;
//!    - `SPEC_EXTRA` is the emulator image's own spec file and rb's, so `makefs` takes the new
//!      files with root as their owner;
//!    - `RC_CONF_EXTRA` is the emulator image's own `rc.conf` lines, `rb_enable=YES` and
//!      `securelevel=-1`, which keeps the level at 0 for the module checks;
//!    - `INSTALLBOOTOPTIONS` puts the BIOS boot blocks on the serial console;
//!    - `LIVEIMG_RELEASEDIR` puts the image in rb's work directory, so the release is not
//!      changed.
//!
//! The overlay also has a `/boot.cfg` with `consdev=com0`, which the EFI loader needs for the
//! serial console. It replaces the one of the `etc` set, which the spec of the image already
//! lists. Last, rb unpacks the `.img.gz` and, with `--size`, makes the file larger. The image's
//! `resize_gpt` and `resize_root` grow the root into the new space at the first boot, which
//! then reboots once.

use crate::guest::{self, GuestFile};
use crate::image::{self, Made, Request};
use crate::plan::{self, Step};
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

/// The wrapper's variables rb needs.
pub const NEEDED: &[&str] = &[
    "TOOLDIR",
    "NETBSDSRCDIR",
    "DESTDIR",
    "RELEASEDIR",
    "MACHINE",
];

/// rb's own lines in the NetBSD image's `rc.conf`, after `rb_enable`.
///
/// `/etc/rc.d/securelevel` raises `kern.securelevel` from 0 to 1 before `DAEMON` when rc.conf does
/// not set it, and rb-rc runs after `LOGIN`, so `modload` in the smoke and modules suites failed
/// with `EPERM`. With `securelevel=-1` that script says it cannot lower the level and leaves it
/// at 0. init raises it to 1 only when `/etc/rc` is done, which is after rb-rc has run.
pub const RC_CONF: &str = "securelevel=-1\n";

/// The `boot.cfg` when `DESTDIR` has none.
pub const BOOT_CFG: &str = "\
menu=Boot normally:rndseed /var/db/entropy-file;boot
menu=Boot single user:rndseed /var/db/entropy-file;boot -s
menu=Drop to boot prompt:prompt
default=1
timeout=5
";

/// The `nbmake-<machine>` wrapper of `build.sh`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Wrapper {
    /// Where it is.
    pub path: PathBuf,
    /// The variables it sets.
    pub vars: BTreeMap<String, String>,
}

/// Undo `build.sh`'s `shell_quote`: single quotes, with `'\''` for a quote inside.
fn unquote(value: &str) -> String {
    let mut out = String::new();
    let mut chars = value.chars().peekable();
    let mut quoted = false;
    while let Some(c) = chars.next() {
        match c {
            '\'' => quoted = !quoted,
            '\\' if !quoted => {
                if let Some(n) = chars.next() {
                    out.push(n);
                }
            }
            _ => out.push(c),
        }
    }
    out
}

impl Wrapper {
    /// Read the lines `NAME=value; export NAME`.
    #[must_use]
    pub fn parse(path: &Path, text: &str) -> Self {
        let mut vars = BTreeMap::new();
        for line in text.lines() {
            let Some((assign, export)) = line.rsplit_once("; export ") else {
                continue;
            };
            let Some((name, value)) = assign.split_once('=') else {
                continue;
            };
            if name == export.trim() {
                vars.insert(name.to_string(), unquote(value));
            }
        }
        Wrapper {
            path: path.to_path_buf(),
            vars,
        }
    }

    /// Find the wrapper from a `TOOLDIR` or a wrapper path, and read it.
    pub fn find(build: &Path) -> Result<Self, String> {
        let path = if build.is_file() {
            build.to_path_buf()
        } else {
            let bin = build.join("bin");
            let entries = std::fs::read_dir(&bin).map_err(|e| format!("{}: {e}", bin.display()))?;
            let mut found: Vec<PathBuf> = entries
                .flatten()
                .map(|e| e.path())
                .filter(|p| {
                    p.file_name()
                        .and_then(|n| n.to_str())
                        .is_some_and(|n| n.starts_with("nbmake-"))
                })
                .collect();
            found.sort();
            match found.len() {
                0 => return Err(format!("no nbmake-* wrapper in {}", bin.display())),
                1 => found.remove(0),
                _ => {
                    let names: Vec<String> =
                        found.iter().map(|p| p.display().to_string()).collect();
                    return Err(format!(
                        "more than one wrapper, name one: {}",
                        names.join(", ")
                    ));
                }
            }
        };
        let text =
            std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let w = Wrapper::parse(&path, &text);
        for v in NEEDED {
            if !w.vars.contains_key(*v) {
                return Err(format!("{} does not set {v}", path.display()));
            }
        }
        Ok(w)
    }

    /// A variable the wrapper sets.
    #[must_use]
    pub fn get(&self, name: &str) -> &str {
        self.vars.get(name).map_or("", String::as_str)
    }

    /// The directory of the emulator image's own files.
    #[must_use]
    pub fn emuimage(&self) -> PathBuf {
        Path::new(self.get("NETBSDSRCDIR")).join(format!(
            "distrib/{}/liveimage/emuimage",
            self.get("MACHINE")
        ))
    }
}

fn s(p: &Path) -> String {
    p.display().to_string()
}

/// A list of files for a make variable that `cat` reads: the emulator image's own file, when the
/// tree has it, then rb's.
fn cat_list(own: &Path, ours: &Path) -> String {
    if own.is_file() {
        format!("{} {}", s(own), s(ours))
    } else {
        s(ours)
    }
}

/// The commands.
#[must_use]
pub fn steps(req: &Request, w: &Wrapper) -> Vec<Step> {
    let make = s(&w.path);
    let src = Path::new(w.get("NETBSDSRCDIR"));
    let live = src.join(format!("distrib/{}/liveimage", w.get("MACHINE")));
    let emu = w.emuimage();
    let work = &req.work;
    vec![
        Step::new("obj", &[&make, "-C", &s(&live), "obj"]),
        Step::new("clean", &[&make, "-C", &s(&live), "clean"]),
        Step::new(
            "live-image",
            &[
                &make,
                "-C",
                &s(src),
                "live-image",
                &format!("LIVEIMG_RELEASEDIR={}", s(&work.join("live"))),
                &format!("IMGDIR_EXTRA={} .", s(&work.join("overlay"))),
                &format!(
                    "SPEC_EXTRA={}",
                    cat_list(&emu.join("spec.emuimage"), &work.join("rb.spec"))
                ),
                &format!(
                    "RC_CONF_EXTRA={}",
                    cat_list(&emu.join("rc.conf.emuimage"), &work.join("rb.rc.conf"))
                ),
                "INSTALLBOOTOPTIONS=-o console=com0,speed=115200",
            ],
        ),
    ]
}

/// `boot.cfg` with the serial console: the tree's file with `consdev=com0` first, and any other
/// `consdev` line dropped.
#[must_use]
pub fn boot_cfg(existing: Option<&str>) -> String {
    let mut out = String::from("consdev=com0\n");
    for line in existing.unwrap_or(BOOT_CFG).lines() {
        if !line.starts_with("consdev=") {
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

/// The spec lines for rb's files: one per new directory and per file, owned by root.
#[must_use]
pub fn spec(dirs: &[String], files: &[(String, u32)]) -> String {
    let mut out = String::from("# rb's files (plan 12.3), written by rb image.\n");
    for d in dirs {
        let _ = writeln!(out, "./{d} type=dir uname=root gname=wheel mode=0755");
    }
    for (f, mode) in files {
        let _ = writeln!(
            out,
            "./{f} type=file uname=root gname=wheel mode={mode:04o}"
        );
    }
    out
}

/// Make the image.
pub fn make(req: &Request, dry_run: bool) -> Result<Made, String> {
    let w = Wrapper::find(&req.build)?;
    let steps = steps(req, &w);
    let files: Vec<GuestFile> = guest::files(&req.conf);
    let mut added = image::paths(&files);
    let pkgs = match &req.packages {
        Some(p) => image::package_files(p)?
            .into_iter()
            .map(|f| format!("{}/{f}", image::PKG_DIR))
            .collect(),
        None => Vec::new(),
    };
    added.extend(pkgs.iter().cloned());
    if dry_run {
        added.push("boot.cfg".into());
        return Ok(Made {
            out: req.out.clone(),
            bytes: 0,
            script: plan::script(&steps),
            added,
        });
    }
    let work = &req.work;
    let _ = std::fs::remove_dir_all(work);
    let overlay = work.join("overlay");
    guest::write_tree(&overlay, &files)?;
    if let Some(p) = &req.packages {
        image::copy_packages(p, &overlay)?;
    }
    let destdir_cfg = std::fs::read_to_string(Path::new(w.get("DESTDIR")).join("boot.cfg")).ok();
    std::fs::write(overlay.join("boot.cfg"), boot_cfg(destdir_cfg.as_deref()))
        .map_err(|e| format!("{}: {e}", overlay.display()))?;
    let mut spec_files: Vec<(String, u32)> =
        files.iter().map(|f| (f.path.to_string(), f.mode)).collect();
    spec_files.extend(pkgs.iter().map(|p| (p.clone(), 0o644)));
    let dirs = image::new_dirs(&added, image::BASE_DIRS);
    std::fs::write(work.join("rb.spec"), spec(&dirs, &spec_files))
        .map_err(|e| format!("rb.spec: {e}"))?;
    std::fs::write(
        work.join("rb.rc.conf"),
        format!("\n# rb-rc (plan 12.4)\n{}{RC_CONF}", guest::RC_CONF),
    )
    .map_err(|e| format!("rb.rc.conf: {e}"))?;
    added.push("boot.cfg".into());

    plan::run(&steps, &work.join("logs"))?;

    let live = work.join("live");
    let gz = std::fs::read_dir(&live)
        .map_err(|e| format!("{}: {e}", live.display()))?
        .flatten()
        .map(|e| e.path())
        .find(|p| p.to_str().is_some_and(|n| n.ends_with(".img.gz")))
        .ok_or_else(|| format!("live-image wrote no .img.gz to {}", live.display()))?;
    let tmp = req.out.with_extension("img.tmp");
    let file = std::fs::File::create(&tmp).map_err(|e| format!("{}: {e}", tmp.display()))?;
    let status = std::process::Command::new("gzip")
        .arg("-dc")
        .arg(&gz)
        .stdout(file)
        .status()
        .map_err(|e| format!("gzip: {e}"))?;
    if !status.success() {
        return Err(format!("gzip -dc {} failed: {status}", gz.display()));
    }
    if let Some(size) = req.size {
        plan::grow(&tmp, size)?;
    }
    let mut script = steps.clone();
    script.push(Step::new("unpack", &["gzip", "-dc", &s(&gz)]));
    image::finish(&tmp, &req.out, &script, added)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::conf::RbConf;
    use crate::image::System;

    const WRAPPER: &str = "\
#! /bin/sh
# Set proper variables to allow easy \"make\" building of a NetBSD subtree.
#

DESTDIR=/mnt/nb/destdir; export DESTDIR
MACHINE=amd64; export MACHINE
MAKEFLAGS='-de -m /mnt/nb/src/share/mk  MKOBJDIRS=yes'; export MAKEFLAGS
NETBSDSRCDIR=/mnt/nb/src; export NETBSDSRCDIR
RELEASEDIR=/mnt/nb/release; export RELEASEDIR
TOOLDIR=/mnt/nb/tools; export TOOLDIR
X='it'\\''s'; export X
unset BUILDID

exec \"${TOOLDIR}/bin/nbmake\" ${1+\"$@\"}
";

    #[test]
    fn wrapper() {
        let w = Wrapper::parse(Path::new("/mnt/nb/tools/bin/nbmake-amd64"), WRAPPER);
        assert_eq!(w.get("MACHINE"), "amd64");
        assert_eq!(
            w.get("MAKEFLAGS"),
            "-de -m /mnt/nb/src/share/mk  MKOBJDIRS=yes"
        );
        assert_eq!(w.get("X"), "it's");
        assert!(!w.vars.contains_key("BUILDID"));
        assert_eq!(
            w.emuimage(),
            Path::new("/mnt/nb/src/distrib/amd64/liveimage/emuimage")
        );
    }

    #[test]
    fn commands() {
        let w = Wrapper::parse(Path::new("/t/bin/nbmake-amd64"), WRAPPER);
        let req = Request {
            build: "/t".into(),
            out: "/img/nb.img".into(),
            system: System::Netbsd,
            conf: RbConf::default(),
            packages: None,
            size: None,
            tools: None,
            work: "/img/work".into(),
        };
        let script = plan::script(&steps(&req, &w));
        assert_eq!(
            script,
            "/t/bin/nbmake-amd64 -C /mnt/nb/src/distrib/amd64/liveimage obj\n\
             /t/bin/nbmake-amd64 -C /mnt/nb/src/distrib/amd64/liveimage clean\n\
             /t/bin/nbmake-amd64 -C /mnt/nb/src live-image LIVEIMG_RELEASEDIR=/img/work/live 'IMGDIR_EXTRA=/img/work/overlay .' SPEC_EXTRA=/img/work/rb.spec RC_CONF_EXTRA=/img/work/rb.rc.conf 'INSTALLBOOTOPTIONS=-o console=com0,speed=115200'\n"
        );
    }

    #[test]
    fn boot_cfg_has_the_serial_console() {
        assert_eq!(
            boot_cfg(Some("consdev=pc\nmenu=Boot:boot\ntimeout=5\n")),
            "consdev=com0\nmenu=Boot:boot\ntimeout=5\n"
        );
        assert!(boot_cfg(None).starts_with("consdev=com0\nmenu=Boot normally:"));
    }

    #[test]
    fn spec_lines() {
        assert_eq!(
            spec(&["usr/libexec/rb".into()], &[("etc/rc.d/rb".into(), 0o555)]),
            "# rb's files (plan 12.3), written by rb image.\n\
             ./usr/libexec/rb type=dir uname=root gname=wheel mode=0755\n\
             ./etc/rc.d/rb type=file uname=root gname=wheel mode=0555\n"
        );
    }
}
