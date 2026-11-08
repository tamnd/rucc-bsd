//! FreeBSD images: `makefs` and `mkimg` over a staging tree (plan 12.3).
//!
//! These are the tools `release/Makefile.vm` and `tools/boot/rootgen.sh` use. The `vm-image`
//! target is not used, because on `main` it needs a pkgbase repository.
//!
//! The disk is GPT with five partitions:
//!
//! | partition | label | contents |
//! |---|---|---|
//! | `freebsd-boot` | `rbboot` | `gptboot`, for the BIOS boot |
//! | `efi` | `rbefi` | a FAT32 file system with `loader.efi` as `EFI/BOOT/BOOTX64.EFI` |
//! | `freebsd-ufs` | `rbroot` | the root, from the staging tree |
//! | `freebsd-swap` | `rbswap` | 1 GiB of swap |
//! | `freebsd-ufs` | `rbspare` | 16 MiB with one file, for the smoke suite's fsck and mount |
//!
//! rb writes its files into the staging tree. Files of the tree that rb changes (`rc.conf`,
//! `loader.conf`, `ddb.conf`) get rb's lines between two marker lines, so a second run replaces
//! them. A tree from a `-DNO_ROOT` build has a `METALOG`. Then rb writes `METALOG.rb` with its
//! own files added, and `makefs` reads that, as `release/tools/vmimage.subr` does.

use crate::guest::{self, GuestFile};
use crate::image::{self, Made, Request};
use crate::plan::{self, Step};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

/// The boot files the staging tree must have.
pub const BOOT_FILES: &[&str] = &["boot/pmbr", "boot/gptboot", "boot/loader.efi"];

/// The default size of the root file system.
pub const ROOT_SIZE: &str = "10g";

/// `/etc/fstab`. The partitions are found by their GPT labels.
pub const FSTAB: &str = "\
# Written by rb image (plan 12.3).
/dev/gpt/rbroot\t/\tufs\trw\t1\t1
/dev/gpt/rbswap\tnone\tswap\tsw\t0\t0
";

/// rb's lines in `/etc/rc.conf`. No services that use the network are started.
pub const RC_CONF: &str = "\
hostname=\"rb\"
ifconfig_vtnet0=\"DHCP\"
sendmail_enable=\"NONE\"
dumpdev=\"NO\"
ddb_enable=\"YES\"
rb_enable=\"YES\"
";

/// rb's lines in `/boot/loader.conf`: the serial console, a short boot delay, and the kernel
/// debugger on a panic, so the backtrace reaches the console (plan 12.4).
pub const LOADER_CONF: &str = "\
console=\"comconsole\"
comconsole_speed=\"115200\"
boot_serial=\"YES\"
autoboot_delay=\"2\"
beastie_disable=\"YES\"
debug.debugger_on_panic=\"1\"
kern.geom.label.disk_ident.enable=\"0\"
kern.geom.label.gptid.enable=\"0\"
";

/// rb's line in `/etc/ddb.conf`: on a panic, print the backtrace and stay at the `db>` prompt,
/// where rb boot sees it.
pub const DDB_CONF: &str = "script kdb.enter.panic=bt; show pcpu\n";

/// `/boot.config`: `gptboot` on the serial console.
pub const BOOT_CONFIG: &str = "-S115200 -h\n";

/// The file in the spare partition.
pub const SPARE_FILE: &str = "rb-spare";

/// A tool: from `--tools` when given, else from `PATH`.
fn tool(req: &Request, name: &str) -> String {
    match &req.tools {
        Some(dir) => dir.join(name).display().to_string(),
        None => name.to_string(),
    }
}

fn s(p: &Path) -> String {
    p.display().to_string()
}

/// The commands, given whether the tree has a `METALOG`.
#[must_use]
pub fn steps(req: &Request, metalog: bool) -> Vec<Step> {
    let makefs = tool(req, "makefs");
    let mkimg = tool(req, "mkimg");
    let w = &req.work;
    let root_size = req
        .size
        .map_or_else(|| ROOT_SIZE.to_string(), |n| n.to_string());
    let tmp = tmp_out(req);
    let mut steps = vec![
        Step::new(
            "esp",
            &[
                &makefs,
                "-t",
                "msdos",
                "-o",
                "fat_type=32,sectors_per_cluster=1,volume_label=EFISYS",
                "-s",
                "64m",
                &s(&w.join("esp.img")),
                &s(&w.join("esp")),
            ],
        ),
        Step::new(
            "spare",
            &[
                &makefs,
                "-B",
                "little",
                "-t",
                "ffs",
                "-s",
                "16m",
                "-o",
                "label=rbspare,version=2",
                &s(&w.join("spare.ufs")),
                &s(&w.join("spare")),
            ],
        ),
    ];
    let mut root: Vec<String> = [
        &makefs, "-B", "little", "-t", "ffs", "-Z", "-s", &root_size, "-f", "200000",
    ]
    .iter()
    .map(|x| (*x).to_string())
    .collect();
    root.extend([
        "-o".into(),
        "label=rbroot,version=2,softupdates=1,bsize=32768,fsize=4096".into(),
    ]);
    let mut root_step = if metalog {
        root.extend(["-D".into(), "-N".into(), s(&req.build.join("etc"))]);
        root.extend([s(&w.join("root.ufs")), "METALOG.rb".into()]);
        Step {
            name: "root".into(),
            argv: root,
            cwd: None,
        }
        .in_dir(&req.build)
    } else {
        root.extend([s(&w.join("root.ufs")), s(&req.build)]);
        Step {
            name: "root".into(),
            argv: root,
            cwd: None,
        }
    };
    root_step.name = "root".into();
    steps.push(root_step);
    let b = |f: &str| s(&req.build.join(f));
    steps.push(Step::new(
        "disk",
        &[
            &mkimg,
            "-s",
            "gpt",
            "-f",
            "raw",
            "-b",
            &b("boot/pmbr"),
            "-p",
            &format!("freebsd-boot/rbboot:={}", b("boot/gptboot")),
            "-p",
            &format!("efi/rbefi:={}", s(&w.join("esp.img"))),
            "-p",
            &format!("freebsd-ufs/rbroot:={}", s(&w.join("root.ufs"))),
            "-p",
            "freebsd-swap/rbswap::1G",
            "-p",
            &format!("freebsd-ufs/rbspare:={}", s(&w.join("spare.ufs"))),
            "-o",
            &s(&tmp),
        ],
    ));
    steps
}

fn tmp_out(req: &Request) -> PathBuf {
    let mut name = req
        .out
        .file_name()
        .map(std::ffi::OsString::from)
        .unwrap_or_default();
    name.push(".tmp");
    req.out.with_file_name(name)
}

/// The files rb writes into the tree, with the tree's own files where rb adds a block.
#[must_use]
pub fn files(req: &Request) -> Vec<GuestFile> {
    let mut files = guest::files(&req.conf);
    let read = |p: &str| std::fs::read_to_string(req.build.join(p)).unwrap_or_default();
    let file = |path, mode, data: String| GuestFile { path, mode, data };
    files.push(file("etc/fstab", 0o644, FSTAB.to_string()));
    files.push(file(
        "etc/rc.conf",
        0o644,
        image::with_block(&read("etc/rc.conf"), RC_CONF),
    ));
    files.push(file(
        "etc/ddb.conf",
        0o644,
        image::with_block(&read("etc/ddb.conf"), DDB_CONF),
    ));
    files.push(file(
        "boot/loader.conf",
        0o644,
        image::with_block(&read("boot/loader.conf"), LOADER_CONF),
    ));
    files.push(file("boot.config", 0o644, BOOT_CONFIG.to_string()));
    files
}

/// A `METALOG` with rb's files: the tree's lines for those paths are dropped, and a line for
/// each of rb's directories and files is added.
#[must_use]
pub fn metalog(existing: &str, dirs: &[String], files: &[(String, u32, usize)]) -> String {
    let ours = |path: &str| {
        let p = path.strip_prefix("./").unwrap_or(path);
        dirs.iter().any(|d| d == p) || files.iter().any(|(f, _, _)| f == p)
    };
    let mut out = String::new();
    for line in existing.lines() {
        let path = line.split_whitespace().next().unwrap_or("");
        if !ours(path) {
            out.push_str(line);
            out.push('\n');
        }
    }
    for d in dirs {
        let _ = writeln!(out, "./{d} type=dir uname=root gname=wheel mode=0755");
    }
    for (f, mode, size) in files {
        let _ = writeln!(
            out,
            "./{f} type=file uname=root gname=wheel mode={mode:04o} size={size}"
        );
    }
    out
}

/// Make the image.
pub fn make(req: &Request, dry_run: bool) -> Result<Made, String> {
    for f in BOOT_FILES {
        if !req.build.join(f).is_file() && !dry_run {
            return Err(format!(
                "{} has no {f}; is it a FreeBSD staging tree?",
                req.build.display()
            ));
        }
    }
    let has_metalog = req.build.join("METALOG").is_file();
    let steps = steps(req, has_metalog);
    let files = files(req);
    let mut added = image::paths(&files);
    if dry_run {
        if let Some(p) = &req.packages {
            added.extend(
                image::package_files(p)?
                    .into_iter()
                    .map(|f| format!("{}/{f}", image::PKG_DIR)),
            );
        }
        return Ok(Made {
            out: req.out.clone(),
            bytes: 0,
            script: plan::script(&steps),
            added,
        });
    }
    let w = &req.work;
    let _ = std::fs::remove_dir_all(w);
    for d in [w.join("esp/EFI/BOOT"), w.join("spare")] {
        std::fs::create_dir_all(&d).map_err(|e| format!("{}: {e}", d.display()))?;
    }
    let efi = w.join("esp/EFI/BOOT/BOOTX64.EFI");
    std::fs::copy(req.build.join("boot/loader.efi"), &efi)
        .map_err(|e| format!("{}: {e}", efi.display()))?;
    std::fs::write(
        w.join("spare").join(SPARE_FILE),
        "rb spare partition (plan 12.2)\n",
    )
    .map_err(|e| format!("{}: {e}", w.display()))?;

    guest::write_tree(&req.build, &files)?;
    if let Some(p) = &req.packages {
        added.extend(image::copy_packages(p, &req.build)?);
    }
    if has_metalog {
        let existing = std::fs::read_to_string(req.build.join("METALOG"))
            .map_err(|e| format!("{}/METALOG: {e}", req.build.display()))?;
        let mut sized = Vec::new();
        for path in &added {
            let mode = files
                .iter()
                .find(|f| f.path == path)
                .map_or(0o644, |f| f.mode);
            let len = std::fs::metadata(req.build.join(path))
                .map_err(|e| format!("{path}: {e}"))?
                .len();
            sized.push((
                path.clone(),
                mode,
                usize::try_from(len).unwrap_or(usize::MAX),
            ));
        }
        let dirs = image::new_dirs(&added, image::BASE_DIRS);
        let text = metalog(&existing, &dirs, &sized);
        std::fs::write(req.build.join("METALOG.rb"), text)
            .map_err(|e| format!("METALOG.rb: {e}"))?;
    }
    let tmp = tmp_out(req);
    let _ = std::fs::remove_file(&tmp);
    plan::run(&steps, &w.join("logs"))?;
    image::finish(&tmp, &req.out, &steps, added)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::conf::RbConf;
    use crate::image::System;

    fn request() -> Request {
        Request {
            build: "/stage".into(),
            out: "/img/fb.img".into(),
            system: System::Freebsd,
            conf: RbConf::default(),
            packages: None,
            size: None,
            tools: Some("/obj/tmp/legacy/usr/bin".into()),
            work: "/img/work".into(),
        }
    }

    #[test]
    fn commands() {
        let script = plan::script(&steps(&request(), false));
        let lines: Vec<&str> = script.lines().collect();
        assert_eq!(lines.len(), 4);
        assert_eq!(
            lines[0],
            "/obj/tmp/legacy/usr/bin/makefs -t msdos -o fat_type=32,sectors_per_cluster=1,volume_label=EFISYS -s 64m /img/work/esp.img /img/work/esp"
        );
        assert!(
            lines[1].contains("-o label=rbspare,version=2 /img/work/spare.ufs /img/work/spare")
        );
        assert_eq!(
            lines[2],
            "/obj/tmp/legacy/usr/bin/makefs -B little -t ffs -Z -s 10g -f 200000 -o label=rbroot,version=2,softupdates=1,bsize=32768,fsize=4096 /img/work/root.ufs /stage"
        );
        assert_eq!(
            lines[3],
            "/obj/tmp/legacy/usr/bin/mkimg -s gpt -f raw -b /stage/boot/pmbr -p freebsd-boot/rbboot:=/stage/boot/gptboot -p efi/rbefi:=/img/work/esp.img -p freebsd-ufs/rbroot:=/img/work/root.ufs -p freebsd-swap/rbswap::1G -p freebsd-ufs/rbspare:=/img/work/spare.ufs -o /img/fb.img.tmp"
        );
        let mut req = request();
        req.size = Some(4 << 30);
        req.tools = None;
        let script = plan::script(&steps(&req, true));
        let root = script.lines().nth(2).unwrap();
        assert!(
            root.starts_with("(cd /stage && makefs -B little -t ffs -Z -s 4294967296 "),
            "{root}"
        );
        assert!(
            root.ends_with(" -D -N /stage/etc /img/work/root.ufs METALOG.rb)"),
            "{root}"
        );
    }

    #[test]
    fn metalog_lines() {
        let existing = "#mtree 2.0\n./etc type=dir mode=0755\n./etc/rc.conf type=file mode=0600 size=3\n./bin/sh type=file\n";
        let out = metalog(
            existing,
            &["usr/libexec/rb".into()],
            &[
                ("etc/rc.conf".into(), 0o644, 40),
                ("usr/libexec/rb/run".into(), 0o555, 9),
            ],
        );
        assert_eq!(
            out,
            "#mtree 2.0\n./etc type=dir mode=0755\n./bin/sh type=file\n\
             ./usr/libexec/rb type=dir uname=root gname=wheel mode=0755\n\
             ./etc/rc.conf type=file uname=root gname=wheel mode=0644 size=40\n\
             ./usr/libexec/rb/run type=file uname=root gname=wheel mode=0555 size=9\n"
        );
    }
}
