//! `rb image`: a bootable raw disk image from a build (plan 12.3). The work is in `rb-image`.

use crate::cli::Args;
use rb_image::conf::{RbConf, parse_suites};
use rb_image::image::{self, Request, System};
use rb_image::plan::parse_size;
use std::fmt::Write as _;
use std::path::PathBuf;

/// The request that the command line asks for.
pub fn request(args: &Args) -> Result<Request, String> {
    let build = PathBuf::from(
        args.get("build")
            .or(args.target.as_deref())
            .ok_or("name the build: a FreeBSD staging tree or a NetBSD TOOLDIR")?,
    );
    let out = PathBuf::from(args.get("out").ok_or("give --out IMG")?);
    let system = match args.get("system") {
        Some(name) => System::parse(name)?,
        None => System::detect(&build)?,
    };
    let conf = RbConf {
        suites: parse_suites(args.get("suites").unwrap_or("boot"))?,
        ..RbConf::default()
    };
    let work = args.get("work").map_or_else(
        || {
            let mut name = out
                .file_name()
                .map(std::ffi::OsString::from)
                .unwrap_or_default();
            name.push(".work");
            out.with_file_name(name)
        },
        PathBuf::from,
    );
    Ok(Request {
        build,
        out,
        system,
        conf,
        packages: args.get("packages").map(PathBuf::from),
        size: args.get("size").map(parse_size).transpose()?,
        tools: args.get("tools").map(PathBuf::from),
        work,
    })
}

/// Run `rb image` and return its report.
pub fn run(args: &Args) -> Result<String, String> {
    let req = request(args)?;
    if let Some(p) = &req.packages
        && !p.is_dir()
    {
        return Err(format!("--packages {}: not a directory", p.display()));
    }
    let dry_run = args.has("dry-run");
    let made = image::make(&req, dry_run)?;
    Ok(report(&req, &made, dry_run))
}

/// The report.
#[must_use]
pub fn report(req: &Request, made: &image::Made, dry_run: bool) -> String {
    let mut s = String::new();
    let _ = writeln!(
        s,
        "# rb image: {} from {}\n",
        req.system.name(),
        req.build.display()
    );
    if dry_run {
        let _ = writeln!(
            s,
            "A dry run. These commands would make {}:\n",
            made.out.display()
        );
        let _ = writeln!(s, "```\n{}```\n", made.script);
    } else {
        let _ = writeln!(s, "- image: {}, {} bytes", made.out.display(), made.bytes);
        let _ = writeln!(s, "- logs: {}", req.work.join("logs").display());
    }
    let _ = writeln!(
        s,
        "- suites in /etc/rb.conf: {}",
        req.conf.suites.join(", ")
    );
    let _ = writeln!(s, "- files rb adds: {}", made.added.len());
    for f in &made.added {
        let _ = writeln!(s, "  - /{f}");
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::{self, Args};

    fn args(words: &[&str]) -> Args {
        let words: Vec<String> = words.iter().map(|w| (*w).to_string()).collect();
        Args::parse(&words, cli::accepted).unwrap()
    }

    #[test]
    fn requests() {
        let req = request(&args(&[
            "image",
            "/stage",
            "--out",
            "/x/fb.img",
            "--system",
            "freebsd",
            "--size",
            "4g",
            "--suites",
            "boot,smoke",
        ]))
        .unwrap();
        assert_eq!(req.system, System::Freebsd);
        assert_eq!(req.size, Some(4 << 30));
        assert_eq!(req.work, PathBuf::from("/x/fb.img.work"));
        assert_eq!(req.conf.suites, ["boot", "smoke"]);
        let by_flag = args(&[
            "image", "--build", "/stage", "--out", "o", "--system", "freebsd",
        ]);
        assert_eq!(request(&by_flag).unwrap().build, PathBuf::from("/stage"));
        assert!(
            request(&args(&["image", "/stage", "--system", "freebsd"]))
                .unwrap_err()
                .contains("--out")
        );
        assert!(
            request(&args(&[
                "image", "/stage", "--out", "x", "--system", "freebsd", "--suites", "kyua"
            ]))
            .is_err()
        );
    }

    #[test]
    fn dry_run() {
        let a = args(&[
            "image",
            "/stage",
            "--out",
            "/x/fb.img",
            "--system",
            "freebsd",
            "--dry-run",
        ]);
        let text = run(&a).unwrap();
        assert!(text.contains("A dry run."), "{text}");
        assert!(
            text.contains("mkimg -s gpt -f raw -b /stage/boot/pmbr"),
            "{text}"
        );
        assert!(text.contains("  - /etc/rc.d/rb\n"), "{text}");
        assert!(!std::path::Path::new("/x/fb.img.work").exists());
    }
}
