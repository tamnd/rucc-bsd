//! `rb build`: build a pinned tree for a row with G's or R's compiler, the way the workflows do it,
//! so that a workflow, `rb baseline` and a person at a terminal run the same command.
//!
//! The directories are those of `scripts/netbsd/common.sh`, under one root: `src`, `obj`, `tools`,
//! `destdir`, `release` and `logs`. A build through the shim also gets `toolchain-<cc>`, made by
//! `rb toolchain`, and its compile records go to `logs/compile-<cc>.jsonl`.
//!
//! NetBSD runs `build.sh -U -m MACHINE -a MACHINE_ARCH -j N -O obj -T TOOLDIR -D destdir -R release`
//! once for each target, with `-u` after `tools`, and with `-V NAME=value` for each make variable
//! of the row. TOOLDIR is `tools` for `tools` and for `--no-shim`, and the shim's copy of it
//! otherwise.
//!
//! FreeBSD runs `make buildworld` and `make buildkernel` in `src` with `MAKEOBJDIRPREFIX=obj`, the
//! row's `TARGET` and `TARGET_ARCH`, `SRCCONF=conf/freebsd/CONF.conf`, `__MAKE_CONF=/dev/null`, and
//! `CROSS_TOOLCHAIN` set to the `.mk` file of `rb toolchain`, or to the reference package's name
//! under `--no-shim`, then the row's make variables. That is the command line of FreeBSD's own GCC
//! jobs.
//!
//! Each target's output goes to `logs/<name>.log` and to standard output, and one line goes to
//! `logs/phases.tsv` in the columns of `scripts/netbsd/summary.sh`.

use crate::shims::Cc;
use std::fmt::Write as _;
use std::io::{BufRead as _, BufReader, Write as _};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Instant;

/// The directories of one build.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layout {
    /// The root.
    pub root: PathBuf,
    /// The tree.
    pub src: PathBuf,
    /// The object directory, or `MAKEOBJDIRPREFIX`.
    pub obj: PathBuf,
    /// The `TOOLDIR` of `build.sh tools`.
    pub tools: PathBuf,
    /// `DESTDIR`.
    pub destdir: PathBuf,
    /// `RELEASEDIR`.
    pub release: PathBuf,
    /// The logs.
    pub logs: PathBuf,
}

impl Layout {
    /// The directories under `root`, with the tree at `src` when given.
    #[must_use]
    pub fn under(root: &Path, src: Option<&Path>) -> Self {
        Self {
            root: root.to_path_buf(),
            src: src.map_or_else(|| root.join("src"), Path::to_path_buf),
            obj: root.join("obj"),
            tools: root.join("tools"),
            destdir: root.join("destdir"),
            release: root.join("release"),
            logs: root.join("logs"),
        }
    }

    /// The toolchain directory of the shim for `cc`.
    #[must_use]
    pub fn toolchain(&self, cc: Cc) -> PathBuf {
        self.root.join(format!("toolchain-{}", cc.name()))
    }
}

/// One command of a build.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    /// The phase name, which names its log.
    pub name: String,
    /// The command.
    pub argv: Vec<String>,
    /// Where it runs.
    pub cwd: PathBuf,
    /// The environment it adds.
    pub env: Vec<(String, String)>,
    /// The `TOOLDIR`, or the toolchain directory, for the size column.
    pub tooldir: PathBuf,
}

impl Step {
    /// The command as a shell line.
    #[must_use]
    pub fn line(&self) -> String {
        let mut s = format!("cd {} &&", quote(&self.cwd.display().to_string()));
        for (k, v) in &self.env {
            let _ = write!(s, " {k}={}", quote(v));
        }
        for a in &self.argv {
            s.push(' ');
            s.push_str(&quote(a));
        }
        s
    }
}

fn quote(word: &str) -> String {
    if !word.is_empty()
        && word
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_./=:,+@%".contains(c))
    {
        word.to_string()
    } else {
        format!("'{}'", word.replace('\'', r"'\''"))
    }
}

/// What a NetBSD build needs to know.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetbsdBuild {
    /// `MACHINE`.
    pub machine: String,
    /// `MACHINE_ARCH`, for a machine with more than one.
    pub machine_arch: Option<String>,
    /// The row's make variables, as `NAME=value`.
    pub vars: Vec<String>,
    /// `-j`.
    pub jobs: usize,
    /// `-N`.
    pub noise: u32,
    /// The `TOOLDIR` the targets after `tools` use.
    pub tooldir: PathBuf,
    /// The targets, such as `distribution` and `kernel=GENERIC`.
    pub targets: Vec<String>,
}

/// The NetBSD steps.
#[must_use]
pub fn netbsd_steps(layout: &Layout, b: &NetbsdBuild) -> Vec<Step> {
    b.targets
        .iter()
        .map(|target| {
            let tooldir = if target == "tools" {
                layout.tools.clone()
            } else {
                b.tooldir.clone()
            };
            let mut argv: Vec<String> = vec![
                "./build.sh".into(),
                "-U".into(),
                "-N".into(),
                b.noise.to_string(),
                "-m".into(),
                b.machine.clone(),
            ];
            if let Some(arch) = &b.machine_arch {
                argv.extend(["-a".into(), arch.clone()]);
            }
            argv.extend([
                "-j".into(),
                b.jobs.to_string(),
                "-O".into(),
                layout.obj.display().to_string(),
                "-T".into(),
                tooldir.display().to_string(),
                "-D".into(),
                layout.destdir.display().to_string(),
                "-R".into(),
                layout.release.display().to_string(),
            ]);
            for var in &b.vars {
                argv.extend(["-V".into(), var.clone()]);
            }
            if target != "tools" {
                argv.push("-u".into());
            }
            argv.push(target.clone());
            Step {
                name: phase_name(target),
                argv,
                cwd: layout.src.clone(),
                env: Vec::new(),
                tooldir,
            }
        })
        .collect()
}

/// A phase name from a target: `kernel=GENERIC` is `kernel`.
fn phase_name(target: &str) -> String {
    target.split('=').next().unwrap_or(target).to_string()
}

/// What a FreeBSD build needs to know.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FreebsdBuild {
    /// `TARGET`.
    pub target: String,
    /// `TARGET_ARCH`.
    pub target_arch: String,
    /// `-j`.
    pub jobs: usize,
    /// `CROSS_TOOLCHAIN`: a `.mk` file or a package name.
    pub cross_toolchain: String,
    /// `SRCCONF`.
    pub srcconf: PathBuf,
    /// `KERNCONF`.
    pub kernconf: String,
    /// The targets: `world`, `kernel`, or a target of `Makefile.inc1` such as `installworld`,
    /// which gets `DESTDIR=destdir`.
    pub targets: Vec<String>,
    /// The toolchain directory, for the size column.
    pub tooldir: PathBuf,
    /// The row's make variables, as `NAME=value`.
    pub vars: Vec<String>,
}

/// The FreeBSD steps.
#[must_use]
pub fn freebsd_steps(layout: &Layout, b: &FreebsdBuild) -> Vec<Step> {
    b.targets
        .iter()
        .map(|target| {
            let make_target = match target.as_str() {
                "world" => "buildworld",
                "kernel" => "buildkernel",
                other => other,
            };
            let mut argv: Vec<String> = vec![
                "make".into(),
                "-C".into(),
                layout.src.display().to_string(),
                format!("-j{}", b.jobs),
                make_target.into(),
                format!("TARGET={}", b.target),
                format!("TARGET_ARCH={}", b.target_arch),
                format!("KERNCONF={}", b.kernconf),
                format!("CROSS_TOOLCHAIN={}", b.cross_toolchain),
                "__MAKE_CONF=/dev/null".into(),
                format!("SRCCONF={}", b.srcconf.display()),
            ];
            argv.extend(b.vars.iter().cloned());
            if !make_target.starts_with("build") {
                argv.push(format!("DESTDIR={}", layout.destdir.display()));
                argv.push("-DNO_ROOT".into());
            }
            Step {
                name: target.clone(),
                argv,
                cwd: layout.src.clone(),
                env: vec![("MAKEOBJDIRPREFIX".into(), layout.obj.display().to_string())],
                tooldir: b.tooldir.clone(),
            }
        })
        .collect()
}

/// The size of a directory in MiB, or 0 when it does not exist.
fn du(dir: &Path) -> u64 {
    if !dir.is_dir() {
        return 0;
    }
    Command::new("du")
        .arg("-sxm")
        .arg(dir)
        .output()
        .ok()
        .and_then(|o| {
            String::from_utf8_lossy(&o.stdout)
                .split_whitespace()
                .next()
                .and_then(|n| n.parse().ok())
        })
        .unwrap_or(0)
}

/// The MiB free on the file system of `dir`.
fn free(dir: &Path) -> u64 {
    Command::new("df")
        .arg("-Pm")
        .arg(dir)
        .output()
        .ok()
        .and_then(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .nth(1)
                .and_then(|l| l.split_whitespace().nth(3).and_then(|n| n.parse().ok()))
        })
        .unwrap_or(0)
}

/// Run one step: its output to its log and to standard output, and its line to `phases.tsv`.
/// Returns the exit status, or -1 when it died on a signal.
pub fn run_step(layout: &Layout, step: &Step) -> Result<i32, String> {
    std::fs::create_dir_all(&layout.logs).map_err(|e| format!("{}: {e}", layout.logs.display()))?;
    let log_path = layout.logs.join(format!("{}.log", step.name));
    let mut log =
        std::fs::File::create(&log_path).map_err(|e| format!("{}: {e}", log_path.display()))?;
    println!("rb build: {}", step.line());
    let start = Instant::now();
    // sh merges standard error into standard output, so that the log keeps their order.
    let mut child = Command::new("sh")
        .arg("-c")
        .arg("exec \"$@\" 2>&1")
        .arg("sh")
        .args(&step.argv)
        .current_dir(&step.cwd)
        .envs(step.env.iter().map(|(k, v)| (k, v)))
        .stdout(Stdio::piped())
        .spawn()
        .map_err(|e| format!("{}: {e}", step.argv[0]))?;
    if let Some(out) = child.stdout.take() {
        let mut stdout = std::io::stdout();
        for line in BufReader::new(out).split(b'\n') {
            let mut line = line.map_err(|e| e.to_string())?;
            line.push(b'\n');
            let _ = stdout.write_all(&line);
            log.write_all(&line)
                .map_err(|e| format!("{}: {e}", log_path.display()))?;
        }
    }
    let status = child
        .wait()
        .map_err(|e| format!("{}: {e}", step.argv[0]))?
        .code()
        .unwrap_or(-1);
    let seconds = start.elapsed().as_secs();
    let row = format!(
        "{}\t{status}\t{seconds}\t{}\t{}\t{}\t{}\t{}\t{}\n",
        step.name,
        du(&layout.src),
        du(&layout.obj),
        du(&step.tooldir),
        du(&layout.destdir),
        du(&layout.release),
        free(&layout.root),
    );
    let tsv = layout.logs.join("phases.tsv");
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&tsv)
        .and_then(|mut f| f.write_all(row.as_bytes()))
        .map_err(|e| format!("{}: {e}", tsv.display()))?;
    println!(
        "rb build: phase {}: exit {status} after {seconds} s",
        step.name
    );
    Ok(status)
}

/// `rb build` on the command line.
pub mod cli {
    use super::{FreebsdBuild, Layout, NetbsdBuild, Step, freebsd_steps, netbsd_steps, run_step};
    use crate::cli::Args;
    use crate::shims::{self, Ask, Cc, Rucc};
    use crate::toolchain::cli::Files;
    use crate::toolchain::{self, REF_MK, RUCC_MK, diff_freebsd, diff_netbsd, make_freebsd};
    use std::fmt::Write as _;
    use std::path::{Path, PathBuf};

    /// The NetBSD targets when none are given: those of the G workflow's world job.
    pub const NETBSD_TARGETS: &str = "distribution,kernel=GENERIC,releasekernel=GENERIC,sets";
    /// The FreeBSD targets when none are given.
    pub const FREEBSD_TARGETS: &str = "world,kernel";

    fn absolute(p: &str) -> PathBuf {
        std::path::absolute(p).unwrap_or_else(|_| PathBuf::from(p))
    }

    /// One `rb build` command, read from its arguments.
    struct Build<'a> {
        args: &'a Args,
        root: &'a Path,
        files: Files,
        cc: Cc,
        no_shim: bool,
        layout: Layout,
        jobs: usize,
        dry_run: bool,
        freebsd: bool,
        targets: Vec<String>,
        bringup: Vec<String>,
        bringup_units: Vec<String>,
    }

    impl<'a> Build<'a> {
        fn read(args: &'a Args, root: &'a Path) -> Result<Self, String> {
            let files = Files::load(root, args.target.as_deref(), args.get("row"))?;
            let cc = Cc::parse(args.get("cc").ok_or("give --cc rucc or ref")?)?;
            let no_shim = args.has("no-shim");
            if no_shim && cc == Cc::Rucc {
                return Err(
                    "--no-shim builds G with the reference only; R always runs rb-cc".into(),
                );
            }
            let layout = Layout::under(
                &absolute(args.get("root").ok_or("give --root DIR")?),
                args.get("src").map(absolute).as_deref(),
            );
            let jobs = match args.get("jobs") {
                Some(j) => j.parse().map_err(|e| format!("--jobs: {e}"))?,
                None => std::thread::available_parallelism().map_or(1, usize::from),
            };
            let freebsd = files.pin.system == "freebsd";
            let targets = args
                .get("targets")
                .unwrap_or(if freebsd {
                    FREEBSD_TARGETS
                } else {
                    NETBSD_TARGETS
                })
                .split(',')
                .filter(|t| !t.is_empty())
                .map(String::from)
                .collect();
            let (bringup, bringup_units) = if cc == Cc::Rucc {
                shims::bringup(args.get("bringup"), args.get("bringup-units"), &|k| {
                    std::env::var(k).ok()
                })?
            } else {
                (Vec::new(), Vec::new())
            };
            Ok(Self {
                args,
                root,
                files,
                cc,
                no_shim,
                layout,
                jobs,
                dry_run: args.has("dry-run"),
                freebsd,
                targets,
                bringup,
                bringup_units,
            })
        }

        fn netbsd(&self, tooldir: &Path, targets: Vec<String>) -> Result<NetbsdBuild, String> {
            let names = &self.files.row.netbsd;
            Ok(NetbsdBuild {
                machine: self.files.machine(),
                machine_arch: names.machine_arch.clone(),
                vars: names.make.clone(),
                jobs: self.jobs,
                noise: self
                    .args
                    .get("noise")
                    .map(str::parse)
                    .transpose()
                    .map_err(|e| format!("--noise: {e}"))?
                    .unwrap_or(1),
                tooldir: tooldir.to_path_buf(),
                targets,
            })
        }

        /// Make the shim's toolchain and check it. Returns rucc, when R runs it, and whether the
        /// checks held. NetBSD's `tools` target runs here, because the copy needs its `TOOLDIR`.
        fn toolchain(&self, text: &mut String) -> Result<(Option<Rucc>, bool), String> {
            let rucc_pin = self.files.toolchains.rucc.as_ref();
            let mut rucc = None;
            if self.cc == Cc::Rucc {
                let path = Rucc::find(
                    self.args.get("rucc"),
                    std::env::var("RB_RUCC").ok().as_deref(),
                    rucc_pin,
                    std::env::var_os("PATH"),
                )?;
                let found = Rucc::describe(&path, rucc_pin)?;
                let _ = writeln!(text, "{}", found.line(rucc_pin));
                rucc = Some(found);
            }
            let log = self.args.get("log").map_or_else(
                || {
                    self.layout
                        .logs
                        .join(format!("compile-{}.jsonl", self.cc.name()))
                },
                absolute,
            );
            let ask = Ask {
                rucc: rucc.as_ref().map(|r| r.path.clone()),
                log,
                tools: Some(self.layout.tools.clone()),
                destdir: Some(self.layout.destdir.clone()),
                side: Some(self.cc.side()),
                bringup: self.bringup.clone(),
                bringup_units: self.bringup_units.clone(),
            };
            let f = &self.files;
            let shims = shims::plan(&f.pin, &f.row, &f.personas, &f.toolchains, &ask)?;
            let binary = shims::shim_binary(
                self.args.get("shim"),
                std::env::var("RB_SHIM").ok().as_deref(),
            )?;
            let expected = f.expected()?;
            let ungraded = self.args.has("ungraded");
            let dir = self.layout.toolchain(self.cc);
            let checks = if self.freebsd {
                let (g, r) = make_freebsd(&dir, &binary, &shims, &f.toolchains)?;
                diff_freebsd(&g, &r, &expected, rucc_pin, self.cc == Cc::Rucc, ungraded)?
            } else {
                if self.targets.first().map(String::as_str) == Some("tools") {
                    let tools = self.netbsd(&self.layout.tools, vec!["tools".into()])?;
                    for step in netbsd_steps(&self.layout, &tools) {
                        if run_step(&self.layout, &step)? != 0 {
                            let _ = writeln!(text, "rb build: build.sh tools failed");
                            return Ok((rucc, false));
                        }
                    }
                }
                let machine = f.machine();
                toolchain::make_netbsd(&dir, &self.layout.tools, &binary, &shims, &machine)?;
                diff_netbsd(
                    &[(self.cc.side(), &dir)],
                    &expected,
                    &machine,
                    rucc_pin,
                    ungraded,
                )?
            };
            let (checked, ok) = toolchain::report(&checks);
            text.push_str(&checked);
            Ok((rucc, ok))
        }

        fn steps(&self) -> Result<Vec<Step>, String> {
            let dir = self.layout.toolchain(self.cc);
            if !self.freebsd {
                let (tooldir, targets) = if self.no_shim || self.dry_run {
                    (
                        if self.no_shim {
                            self.layout.tools.clone()
                        } else {
                            dir
                        },
                        self.targets.clone(),
                    )
                } else {
                    // The tools target ran before the copy.
                    let rest = self
                        .targets
                        .iter()
                        .filter(|t| *t != "tools")
                        .cloned()
                        .collect();
                    (dir, rest)
                };
                return Ok(netbsd_steps(&self.layout, &self.netbsd(&tooldir, targets)?));
            }
            let conf = self.args.get("conf").unwrap_or("graded");
            let srcconf = self.root.join("conf/freebsd").join(format!("{conf}.conf"));
            if !self.dry_run && !srcconf.is_file() {
                return Err(format!("{} does not exist", srcconf.display()));
            }
            let persona = self
                .files
                .personas
                .for_pin(&self.files.pin)
                .ok_or("personas.toml has no persona for the pin")?;
            let names = &self.files.row.freebsd;
            let mk = if self.cc == Cc::Rucc { RUCC_MK } else { REF_MK };
            Ok(freebsd_steps(
                &self.layout,
                &FreebsdBuild {
                    target: names.target.clone().unwrap_or_else(|| "amd64".into()),
                    target_arch: names.target_arch.clone().unwrap_or_else(|| "amd64".into()),
                    jobs: self.jobs,
                    cross_toolchain: if self.no_shim {
                        persona.reference.clone()
                    } else {
                        dir.join(mk).display().to_string()
                    },
                    srcconf,
                    kernconf: self.args.get("kernconf").unwrap_or("GENERIC").to_string(),
                    targets: self.targets.clone(),
                    tooldir: dir,
                    vars: names.make.clone(),
                },
            ))
        }
    }

    /// Run `rb build`. Returns the report and whether every target built.
    pub fn run(args: &Args, root: &Path) -> Result<(String, bool), String> {
        let build = Build::read(args, root)?;
        let mut text = format!(
            "rb build {} {} with {}{}{}\n",
            build.files.pin.name,
            args.get("row").unwrap_or("amd64"),
            build.cc.name(),
            if build.no_shim {
                ", without the shim"
            } else {
                ""
            },
            if build.bringup.is_empty() {
                String::new()
            } else {
                format!(
                    ", ungraded because bring-up {} is on",
                    build.bringup.join(", ")
                )
            }
        );
        let mut rucc = None;
        if !build.no_shim && !build.dry_run {
            let (found, ok) = build.toolchain(&mut text)?;
            if !ok {
                return Ok((text, false));
            }
            rucc = found;
        }
        let steps = build.steps()?;
        if build.dry_run {
            for s in &steps {
                let _ = writeln!(text, "{}", s.line());
            }
            return Ok((text, true));
        }
        print!("{text}");
        text.clear();
        for step in &steps {
            let status = run_step(&build.layout, step)?;
            if status != 0 {
                let log = build.layout.logs.join(format!("{}.log", step.name));
                let _ = writeln!(
                    text,
                    "rb build: {} failed with exit {status}; the log is {}",
                    step.name,
                    log.display()
                );
                return Ok((text, false));
            }
        }
        if let Some(r) = &rucc {
            let _ = writeln!(
                text,
                "rb build: built with {}",
                r.line(build.files.toolchains.rucc.as_ref())
            );
        }
        let _ = writeln!(text, "rb build: every target built");
        if !build.bringup.is_empty() {
            let _ = writeln!(
                text,
                "rb build: the run is ungraded, because bring-up {} was on; rb census on the compile log lists the delegated units",
                build.bringup.join(", ")
            );
        }
        Ok((text, true))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn netbsd_command_lines_are_those_of_common_sh() {
        let layout = Layout::under(Path::new("/mnt/nb"), None);
        let steps = netbsd_steps(
            &layout,
            &NetbsdBuild {
                machine: "amd64".into(),
                machine_arch: None,
                vars: Vec::new(),
                jobs: 4,
                noise: 1,
                tooldir: PathBuf::from("/mnt/nb/toolchain-rucc"),
                targets: vec!["tools".into(), "kernel=GENERIC".into()],
            },
        );
        assert_eq!(
            steps[0].line(),
            "cd /mnt/nb/src && ./build.sh -U -N 1 -m amd64 -j 4 -O /mnt/nb/obj -T /mnt/nb/tools -D /mnt/nb/destdir -R /mnt/nb/release tools"
        );
        assert_eq!(steps[1].name, "kernel");
        assert!(steps[1].line().ends_with(
            "-T /mnt/nb/toolchain-rucc -D /mnt/nb/destdir -R /mnt/nb/release -u kernel=GENERIC"
        ));
        let riscv = netbsd_steps(
            &layout,
            &NetbsdBuild {
                machine: "riscv".into(),
                machine_arch: Some("riscv64".into()),
                vars: vec!["MKCOMPAT=no".into()],
                jobs: 4,
                noise: 1,
                tooldir: PathBuf::from("/mnt/nb/tools"),
                targets: vec!["distribution".into()],
            },
        );
        assert_eq!(
            riscv[0].line(),
            "cd /mnt/nb/src && ./build.sh -U -N 1 -m riscv -a riscv64 -j 4 -O /mnt/nb/obj -T /mnt/nb/tools -D /mnt/nb/destdir -R /mnt/nb/release -V MKCOMPAT=no -u distribution"
        );
    }

    #[test]
    fn freebsd_command_lines_are_those_of_the_gcc_jobs() {
        let layout = Layout::under(Path::new("/b"), Some(Path::new("/usr/src")));
        let steps = freebsd_steps(
            &layout,
            &FreebsdBuild {
                target: "amd64".into(),
                target_arch: "amd64".into(),
                jobs: 8,
                cross_toolchain: "/b/toolchain-rucc/rucc.mk".into(),
                srcconf: PathBuf::from("/r/conf/freebsd/graded.conf"),
                kernconf: "GENERIC".into(),
                targets: vec!["world".into(), "installworld".into()],
                tooldir: PathBuf::from("/b/toolchain-rucc"),
                vars: Vec::new(),
            },
        );
        assert_eq!(
            steps[0].line(),
            "cd /usr/src && MAKEOBJDIRPREFIX=/b/obj make -C /usr/src -j8 buildworld TARGET=amd64 TARGET_ARCH=amd64 KERNCONF=GENERIC CROSS_TOOLCHAIN=/b/toolchain-rucc/rucc.mk __MAKE_CONF=/dev/null SRCCONF=/r/conf/freebsd/graded.conf"
        );
        assert!(steps[1].line().ends_with("DESTDIR=/b/destdir -DNO_ROOT"));
    }

    #[test]
    fn a_step_writes_its_log_and_its_phase_line() {
        let root = std::env::temp_dir().join(format!("rb-build-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("src")).unwrap();
        let layout = Layout::under(&root, None);
        let step = Step {
            name: "echo".into(),
            argv: vec![
                "sh".into(),
                "-c".into(),
                "echo out; echo err >&2; exit 3".into(),
            ],
            cwd: layout.src.clone(),
            env: Vec::new(),
            tooldir: layout.tools.clone(),
        };
        assert_eq!(run_step(&layout, &step).unwrap(), 3);
        let log = std::fs::read_to_string(layout.logs.join("echo.log")).unwrap();
        assert_eq!(log, "out\nerr\n");
        let tsv = std::fs::read_to_string(layout.logs.join("phases.tsv")).unwrap();
        assert!(tsv.starts_with("echo\t3\t"), "{tsv}");
        assert_eq!(tsv.trim_end().split('\t').count(), 9);
        std::fs::remove_dir_all(&root).ok();
    }
}
