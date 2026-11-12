//! rb-rc run on the host, with a file as the results disk and stub suites.
#![cfg(unix)]

use rb_image::conf::RbConf;
use rb_image::guest;
use rb_image::ustar::{self, Entry};
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::process::Command;

/// A smoke suite that writes its counts.
const SMOKE: &str = r#"#!/bin/sh
mkdir -p "$1"
echo "RB-CHECK pass shells"
echo "RB-CHECK fail network"
printf 'pass shells\nfail network\n' > "$1/results"
echo 1/2 > "$1/counts"
"#;

/// A test suite that saves after its one directory, as tests.sh does after each.
const TESTS: &str = r#"#!/bin/sh
mkdir -p "$1/atf"
echo "tc-end: 1.0, t, passed" > "$1/atf/kern.tps"
echo "RB-CASE kern/t_x:t"
$RB_SAVE
echo 3/4 > "$1/counts"
"#;

struct Rig {
    root: PathBuf,
}

impl Rig {
    fn new(tag: &str, conf: &RbConf) -> Self {
        let root = std::env::temp_dir().join(format!("rb-image-rc-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let mut files = guest::files(conf);
        for f in &mut files {
            if f.path.ends_with("/smoke") {
                f.data = SMOKE.to_string();
            } else if f.path.ends_with("/tests") {
                f.data = TESTS.to_string();
            }
        }
        guest::write_tree(&root, &files).unwrap();
        Rig { root }
    }

    fn disk(&self) -> PathBuf {
        self.root.join("disk.img")
    }

    fn run(&self) -> String {
        let out = Command::new(self.root.join("usr/libexec/rb/run"))
            .env("RB_CONF", self.root.join("etc/rb.conf"))
            .env("RB_WORK", self.root.join("work"))
            .env("RB_LIBEXEC", self.root.join("usr/libexec/rb"))
            .output()
            .unwrap();
        assert!(out.status.success(), "{out:?}");
        String::from_utf8(out.stdout).unwrap()
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn read_disk(path: &Path) -> Vec<Entry> {
    ustar::read(&std::fs::read(path).unwrap()).unwrap()
}

fn find<'a>(entries: &'a [Entry], path: &str) -> &'a str {
    let e = entries
        .iter()
        .find(|e| e.path == path)
        .unwrap_or_else(|| panic!("no {path}"));
    std::str::from_utf8(&e.data).unwrap()
}

#[test]
fn suites_from_the_disk() {
    let mut conf = RbConf {
        poweroff: false,
        ..RbConf::default()
    };
    let rig = Rig::new("disk", &conf);
    // The image says only boot. The disk says boot, smoke and tests, and wins.
    conf.disk = Some(rig.disk().display().to_string());
    std::fs::write(rig.root.join("etc/rb.conf"), conf.render()).unwrap();
    let mut boot = conf.clone();
    boot.suites = vec!["boot".into(), "smoke".into(), "tests".into()];
    let mut disk = ustar::write(&[Entry::file("rb.conf", 0o644, boot.render())]).unwrap();
    disk.resize(1 << 20, 0);
    std::fs::write(rig.disk(), &disk).unwrap();

    let console = rig.run();
    let lines: Vec<&str> = console.lines().collect();
    assert!(lines[0].starts_with("RB-READY "), "{console}");
    for want in [
        "RB-SUITE smoke",
        "RB-CHECK fail network",
        "RB-SUITE-END smoke 1/2",
        "RB-CASE kern/t_x:t",
        "RB-SUITE-END tests 3/4",
    ] {
        assert!(lines.contains(&want), "no {want:?} in {console}");
    }
    assert!(lines.iter().any(|l| l.starts_with("RB-CONF ")), "{console}");
    assert_eq!(
        *lines.last().unwrap(),
        "RB-DONE boot=pass smoke=1/2 tests=3/4 archive=disk"
    );

    let got = read_disk(&rig.disk());
    assert_eq!(find(&got, "suites"), "boot pass\nsmoke 1/2\ntests 3/4\n");
    assert_eq!(find(&got, "smoke/counts"), "1/2\n");
    assert_eq!(find(&got, "tests/atf/kern.tps"), "tc-end: 1.0, t, passed\n");
    assert!(find(&got, "smoke.log").contains("RB-CHECK pass shells"));
    assert!(!find(&got, "uname.txt").is_empty());
    for path in ["dmesg.boot", "dmesg.end", "tests.log"] {
        find(&got, path);
    }
}

#[test]
fn scripts_from_the_disk() {
    let mut conf = RbConf {
        poweroff: false,
        ..RbConf::default()
    };
    let rig = Rig::new("scripts", &conf);
    conf.disk = Some(rig.disk().display().to_string());
    std::fs::write(rig.root.join("etc/rb.conf"), conf.render()).unwrap();
    // The disk asks for the tests suite with its own script, which the image does not have.
    let mut boot = conf.clone();
    boot.suites = vec!["boot".into(), "tests".into()];
    boot.scripts_on_disk = true;
    let tests =
        "#!/bin/sh\nmkdir -p \"$1\"\necho RB-CASE from/the:disk\necho 1/1 > \"$1/counts\"\n";
    let mut disk = ustar::write(&[
        Entry::file("rb.conf", 0o644, boot.render()),
        Entry::dir("rb"),
        Entry::file("rb/tests", 0o555, tests.to_string()),
    ])
    .unwrap();
    disk.resize(1 << 20, 0);
    std::fs::write(rig.disk(), &disk).unwrap();

    let console = rig.run();
    assert!(console.contains("RB-CASE from/the:disk\n"), "{console}");
    assert!(!console.contains("RB-CASE kern/t_x:t"), "{console}");
    assert!(
        console.ends_with("RB-DONE boot=pass tests=1/1 archive=disk\n"),
        "{console}"
    );
}

#[test]
fn an_empty_disk_keeps_the_image_conf() {
    let mut conf = RbConf {
        poweroff: false,
        ..RbConf::default()
    };
    let rig = Rig::new("empty", &conf);
    conf.disk = Some(rig.disk().display().to_string());
    conf.suites = vec!["boot".into(), "smoke".into()];
    std::fs::write(rig.root.join("etc/rb.conf"), conf.render()).unwrap();
    std::fs::write(rig.disk(), vec![0u8; 1 << 20]).unwrap();

    let console = rig.run();
    assert!(!console.contains("RB-CONF"), "{console}");
    assert!(
        console.ends_with("RB-DONE boot=pass smoke=1/2 archive=disk\n"),
        "{console}"
    );
    let got = read_disk(&rig.disk());
    assert_eq!(find(&got, "suites"), "boot pass\nsmoke 1/2\n");
    assert!(!got.iter().any(|e| e.path.starts_with("tests")));
}

#[test]
fn no_disk_writes_the_console() {
    if Command::new("sh")
        .args(["-c", "command -v uuencode"])
        .output()
        .unwrap()
        .stdout
        .is_empty()
    {
        eprintln!("no uuencode on this host, skipped");
        return;
    }
    let conf = RbConf {
        poweroff: false,
        disk: Some(String::new()),
        ..RbConf::default()
    };
    let rig = Rig::new("console", &conf);
    let console = rig.run();
    assert!(console.contains("RB-NODISK none\n"), "{console}");
    assert!(console.contains("\nbegin-base64 644 rb.tar\n"), "{console}");
    assert!(
        console.contains("\n====\nRB-DONE boot=pass archive=console\n"),
        "{console}"
    );
}

#[test]
fn scripts_parse() {
    for (name, text) in [
        ("rb", guest::RC_SCRIPT),
        ("run", guest::RUN),
        ("tests", guest::TESTS),
        ("modules", guest::MODULES),
        ("bench", guest::BENCH),
        ("foreign", guest::FOREIGN),
        ("smoke", guest::SMOKE),
    ] {
        let out = Command::new("sh")
            .args(["-n", "-c", text])
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{name}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
}

/// shellcheck, where the host has it. The GitHub runners for Linux do.
#[test]
fn shellcheck() {
    let top = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let files = [
        "guest/rb-rc/rb",
        "guest/rb-rc/run.sh",
        "guest/rb-rc/tests.sh",
        "guest/rb-rc/bench.sh",
        "guest/rb-rc/foreign.sh",
        "smoke/smoke.sh",
    ];
    let Ok(out) = Command::new("shellcheck")
        .args(["-s", "sh", "-S", "warning"])
        .args(files.iter().map(|f| top.join(f)))
        .output()
    else {
        eprintln!("no shellcheck on this host, skipped");
        return;
    };
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stdout)
    );
}

/// The archive of a host `tar`: bsdtar on macOS, GNU tar on Linux, with a long path.
#[test]
fn host_tar() {
    let root = std::env::temp_dir().join(format!("rb-image-tar-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let deep = format!("{}/{}", "d".repeat(60), "e".repeat(70));
    std::fs::create_dir_all(root.join("out").join(&deep)).unwrap();
    std::fs::write(root.join("out").join(&deep).join("f"), "deep\n").unwrap();
    std::fs::write(root.join("out/counts"), "1/1\n").unwrap();
    let status = Command::new("sh")
        .args(["-c", "cd out && tar -cf ../out.tar ."])
        .current_dir(&root)
        .status()
        .unwrap();
    assert!(status.success());
    let got = read_disk(&root.join("out.tar"));
    assert_eq!(find(&got, "counts"), "1/1\n");
    assert_eq!(find(&got, &format!("{deep}/f")), "deep\n");
    std::fs::remove_dir_all(&root).unwrap();
}

/// tests.sh on NetBSD, with a stub atf-run: a directory under the top, run twice.
#[test]
fn tests_under_the_top_and_again() {
    let root = std::env::temp_dir().join(format!("rb-image-tests-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("usr/tests/net/carp")).unwrap();
    std::fs::create_dir_all(root.join("bin")).unwrap();
    std::fs::write(root.join("usr/tests/Atffile"), "tp: net\n").unwrap();
    let script = root.join("tests");
    std::fs::write(&script, guest::TESTS).unwrap();
    let atf_run = root.join("bin/atf-run");
    std::fs::write(
        &atf_run,
        "#!/bin/sh\nprintf 'tp-start: 1.0, t_basic, 1\\ntc-start: 1.0, a\\ntc-end: 2.0, a, passed\\n'\n",
    )
    .unwrap();
    std::fs::set_permissions(&atf_run, std::fs::Permissions::from_mode(0o755)).unwrap();
    let path = format!(
        "{}:{}",
        root.join("bin").display(),
        std::env::var("PATH").unwrap()
    );
    let out = Command::new("sh")
        .arg(&script)
        .arg(root.join("out"))
        .env("PATH", path)
        .env("RB_TESTS", root.join("usr/tests"))
        .env("rb_tests_only", "net/carp net/carp")
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    let console = String::from_utf8(out.stdout).unwrap();
    assert_eq!(
        console.matches("RB-CASE net/carp/t_basic:a").count(),
        2,
        "{console}"
    );
    let atf = root.join("out/atf");
    for f in ["net/carp.tps", "net/carp@2.tps"] {
        assert!(atf.join(f).is_file(), "no {f}");
    }
    let status = std::fs::read_to_string(atf.join("status")).unwrap();
    let dirs: Vec<&str> = status.lines().filter_map(|l| l.split(' ').next()).collect();
    assert_eq!(dirs, ["net/carp", "net/carp@2"]);
    assert_eq!(
        std::fs::read_to_string(root.join("out/counts")).unwrap(),
        "2/2\n"
    );
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn modules_load_and_unload_one_at_a_time() {
    let root = std::env::temp_dir().join(format!("rb-image-modules-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let base = root.join("stand/modules");
    std::fs::create_dir_all(base.join("old")).unwrap();
    std::fs::write(base.join("old/old.kmod"), "").unwrap();
    let disk = root.join("work/in/kmods");
    for m in ["a", "bad", "builtin", "busy", "skipme"] {
        std::fs::create_dir_all(disk.join(m)).unwrap();
        std::fs::write(disk.join(m).join(format!("{m}.kmod")), "").unwrap();
    }
    std::fs::create_dir_all(disk.join("notamodule")).unwrap();
    // Stubs: a loaded module is a file under state. builtin is in the kernel, bad does not load
    // and busy does not unload.
    let bin = root.join("bin");
    let state = root.join("state");
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::create_dir_all(&state).unwrap();
    for (name, body) in [
        (
            "modload",
            "if [ \"$1\" = bad ]; then echo \"modload: bad: Exec format error\" >&2; exit 1; fi\ntouch \"$STATE/$1\"",
        ),
        (
            "modstat",
            "echo 'NAME CLASS SOURCE FLAGS REFS SIZE REQUIRES'\nif [ \"$1\" = builtin ] || [ -e \"$STATE/$1\" ]; then echo \"$1 misc filesys - 0 1 -\"; fi",
        ),
        (
            "modunload",
            "if [ \"$1\" = busy ]; then echo \"modunload: busy: Device busy\" >&2; exit 1; fi\nrm \"$STATE/$1\"",
        ),
    ] {
        let path = bin.join(name);
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let script = root.join("modules");
    std::fs::write(&script, guest::MODULES).unwrap();
    let path = format!("{}:{}", bin.display(), std::env::var("PATH").unwrap());
    let out = Command::new("sh")
        .arg(&script)
        .arg(root.join("out"))
        .env("PATH", path)
        .env("STATE", &state)
        .env("RB_MODULES", &base)
        .env("RB_WORK", root.join("work"))
        .env("rb_modules_skip", "skipme")
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    let console = String::from_utf8(out.stdout).unwrap();
    assert_eq!(
        console,
        "RB-CASE modules/a\nRB-CASE modules/bad\nRB-CASE modules/builtin\nRB-CASE modules/busy\n"
    );
    let read = |name: &str| std::fs::read_to_string(root.join("out").join(name)).unwrap();
    assert_eq!(
        read("modules.tsv"),
        "a\tpass\t\nbad\tload\tmodload: bad: Exec format error\nbuiltin\tpresent\t\nbusy\tunload\tmodunload: busy: Device busy\nskipme\tskip\t\n"
    );
    assert_eq!(read("counts"), "1/3\n");
    assert_eq!(read("source"), "disk\n");
    // The disk's modules took the place of the image's, which were moved aside.
    assert!(base.join("a/a.kmod").is_file());
    assert!(root.join("stand/modules.image/old/old.kmod").is_file());
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn foreign_adds_each_package_and_runs_its_check() {
    let root = std::env::temp_dir().join(format!("rb-image-foreign-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let pkgs = root.join("work/in/pkgs");
    std::fs::create_dir_all(pkgs.join("All")).unwrap();
    // The checks: one passes and writes in its own directory, one fails, one has a package that
    // does not install, and one reads its input, which must not be the rest of the checks.
    std::fs::write(
        pkgs.join("checks"),
        "good\techo hi > f && test \"$(cat f)\" = hi\n\
         reader\tcat; true\n\
         bad\techo first; echo last words; exit 3\n\
         broken\ttrue\n",
    )
    .unwrap();
    // A stub pkg_add: it fails for broken, and it says where it looks.
    let bin = root.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let stub = bin.join("pkg_add");
    std::fs::write(
        &stub,
        "#!/bin/sh\necho \"add $2 from $PKG_PATH\"\nif [ \"$2\" = broken ]; then echo 'pkg_add: no pkg found for broken'; exit 1; fi\n",
    )
    .unwrap();
    std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755)).unwrap();
    // NetBSD has timeout(1), and macOS does not. The stub runs the command with no limit.
    let timeout = bin.join("timeout");
    std::fs::write(&timeout, "#!/bin/sh\nshift\nexec \"$@\"\n").unwrap();
    std::fs::set_permissions(&timeout, std::fs::Permissions::from_mode(0o755)).unwrap();
    let script = root.join("foreign");
    std::fs::write(&script, guest::FOREIGN).unwrap();
    let path = format!("{}:{}", bin.display(), std::env::var("PATH").unwrap());
    let out = Command::new("sh")
        .arg(&script)
        .arg(root.join("out"))
        .env("PATH", path)
        .env("RB_WORK", root.join("work"))
        .env_remove("RB_SAVE")
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    let console = String::from_utf8(out.stdout).unwrap();
    assert_eq!(
        console,
        "RB-CASE foreign/good\nRB-CASE foreign/reader\nRB-CASE foreign/bad\nRB-CASE foreign/broken\n"
    );
    let read = |name: &str| std::fs::read_to_string(root.join("out").join(name)).unwrap();
    assert_eq!(
        read("foreign.tsv"),
        "good\tpass\t\nreader\tpass\t\nbad\tcheck\tlast words\nbroken\tadd\tpkg_add: no pkg found for broken\n"
    );
    assert_eq!(read("counts"), "2/4\n");
    let log = read("good.log");
    assert!(
        log.starts_with(&format!(
            "add good from {}\n+ echo hi",
            pkgs.join("All").display()
        )),
        "{log}"
    );
    assert!(!root.join("work/foreign").exists());
    // With no packages on the disk, the suite has nothing to run.
    std::fs::remove_dir_all(&pkgs).unwrap();
    let out = Command::new("sh")
        .arg(&script)
        .arg(root.join("none"))
        .env("RB_WORK", root.join("work"))
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    assert_eq!(
        std::fs::read_to_string(root.join("none/counts")).unwrap(),
        "0/0\n"
    );
    std::fs::remove_dir_all(&root).unwrap();
}

/// The smoke suite on a NetBSD whose EFI system partition is mounted on /boot, as the riscv image
/// has it. The spare check lists the mount that is there, and does not run fsck or mount on it,
/// since a second mount of it fails.
#[test]
fn netbsd_spare_reads_a_wedge_the_system_mounted() {
    let root = std::env::temp_dir().join(format!("rb-image-spare-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let boot = root.join("boot");
    std::fs::create_dir_all(boot.join("EFI")).unwrap();
    let bin = root.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let mounted = format!(
        "echo '/dev/dk1 on / type ffs (local)'\necho '/dev/dk0 on {} type msdos (local)'",
        boot.display()
    );
    for (name, body) in [
        ("uname", "echo NetBSD"),
        ("sysctl", "echo dk1"),
        (
            "dkctl",
            "case $2 in\ngetwedgeinfo) echo \"$1 at ld4: netbsd-root\" ;;\nlistwedges) echo 'dk0: EFI system, 262144 blocks at 2048, type: msdos'; echo 'dk1: netbsd-root, 4194304 blocks at 264192, type: ffs' ;;\nesac",
        ),
        ("mount", &mounted),
        ("fsck_msdos", "echo fsck_msdos must not run >&2; exit 1"),
        ("mount_msdos", "echo mount_msdos must not run >&2; exit 1"),
        ("ifconfig", "echo 'inet 10.0.2.15 netmask 0xffffff00'"),
        ("ping", "exit 0"),
        ("modload", "exit 0"),
        ("modstat", "exit 1"),
        ("modunload", "exit 0"),
    ] {
        let path = bin.join(name);
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let script = root.join("smoke");
    std::fs::write(&script, guest::SMOKE).unwrap();
    let path = format!("{}:{}", bin.display(), std::env::var("PATH").unwrap());
    let out = Command::new("sh")
        .arg(&script)
        .arg(root.join("out"))
        .env("PATH", path)
        .env("rb_pkg_dir", root.join("no-packages"))
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    let console = String::from_utf8(out.stdout).unwrap();
    let log = std::fs::read_to_string(root.join("out/spare.log")).unwrap();
    assert!(
        console.contains("RB-CHECK pass spare\n"),
        "{console}\n{log}"
    );
    assert!(
        log.contains(&format!("/dev/dk0 is mounted on {}", boot.display())),
        "{log}"
    );
    assert!(console.contains("RB-CHECK skip package\n"), "{console}");
    std::fs::remove_dir_all(&root).unwrap();
}

/// rb-bench built with the host's cc, where it has one, and run at a small scale.
fn rb_bench(root: &Path) -> Option<PathBuf> {
    let top = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let bin = root.join("rb-bench");
    std::fs::create_dir_all(root).unwrap();
    let Ok(out) = Command::new("cc")
        .args([
            "-O1",
            "-Wall",
            "-Werror",
            "-Wno-deprecated-declarations",
            "-o",
        ])
        .arg(&bin)
        .arg(top.join("bench/rb-bench.c"))
        .args(["-lpthread", "-lm"])
        .output()
    else {
        eprintln!("no cc on this host, skipped");
        return None;
    };
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    Some(bin)
}

#[test]
fn rb_bench_runs_each_benchmark() {
    let root = std::env::temp_dir().join(format!("rb-image-rb-bench-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let Some(bin) = rb_bench(&root) else { return };
    let list = Command::new(&bin).arg("-l").output().unwrap();
    let list = String::from_utf8(list.stdout).unwrap();
    // kqueue_1000 is only on the BSDs and macOS.
    assert!(list.lines().count() >= 31, "{list}");
    let out = Command::new(&bin).args(["-s", "0.0005"]).output().unwrap();
    assert!(out.status.success(), "{out:?}");
    let text = String::from_utf8(out.stdout).unwrap();
    assert_eq!(text.lines().count(), list.lines().count(), "{text}");
    for (line, listed) in text.lines().zip(list.lines()) {
        let f: Vec<&str> = line.split('\t').collect();
        assert_eq!(f.len(), 4, "{line}");
        assert_eq!(format!("{}\t{}", f[0], f[1]), listed);
        assert!(f[2].parse::<f64>().unwrap() >= 0.0, "{line}");
        assert_eq!(f[3], "lower");
    }
    // The inputs are the same bytes each time.
    let generate = |name: &str| {
        let path = root.join(name);
        let st = Command::new(&bin)
            .args(["-g", "lines"])
            .arg(&path)
            .arg("1000")
            .status()
            .unwrap();
        assert!(st.success());
        std::fs::read(path).unwrap()
    };
    let a = generate("a");
    assert_eq!(a, generate("b"));
    assert_eq!(String::from_utf8(a).unwrap().lines().count(), 1000);
    let timed = Command::new(&bin)
        .args(["-x", "tools", "t", "sh", "-c", "exit 3"])
        .output()
        .unwrap();
    assert!(!timed.status.success() && timed.stdout.is_empty());
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn bench_writes_one_line_per_benchmark() {
    let root = std::env::temp_dir().join(format!("rb-image-bench-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let Some(bin) = rb_bench(&root) else { return };
    // The binary comes from the results disk, as rb boot --bench puts it there.
    let disk = root.join("work/in");
    std::fs::create_dir_all(&disk).unwrap();
    std::fs::copy(&bin, disk.join("rb-bench")).unwrap();
    let script = root.join("bench");
    std::fs::write(&script, guest::BENCH).unwrap();
    let out = Command::new("sh")
        .arg(&script)
        .arg(root.join("out"))
        .env("RB_WORK", root.join("work"))
        .env("rb_bench_scale", "0.0005")
        .env(
            "rb_bench_only",
            "getpid qsort sh_loop sort gzip_c gzip_d seq_write nothing",
        )
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    let console = String::from_utf8(out.stdout).unwrap();
    assert_eq!(
        console,
        "RB-CASE bench/getpid\nRB-CASE bench/qsort\nRB-CASE bench/sh_loop\n\
         RB-CASE bench/sort\nRB-CASE bench/gzip_c\nRB-CASE bench/gzip_d\n\
         RB-CASE bench/seq_write\n"
    );
    let read = |name: &str| std::fs::read_to_string(root.join("out").join(name)).unwrap();
    let names: Vec<String> = read("bench.tsv")
        .lines()
        .map(|l| {
            let f: Vec<&str> = l.split('\t').collect();
            assert_eq!(f.len(), 4, "{l}");
            format!("{} {}", f[0], f[1])
        })
        .collect();
    assert_eq!(
        names,
        [
            "syscalls getpid",
            "libc qsort",
            "tools sh_loop",
            "tools sort",
            "tools gzip_c",
            "tools gzip_d",
            "fs seq_write"
        ]
    );
    assert_eq!(read("counts"), "7/7\n");
    assert_eq!(read("source"), "disk\n");
    assert_eq!(read("version"), "rb-bench 1\n");
    // The suite's scratch files are gone.
    assert!(!root.join("work/bench").exists());
    std::fs::remove_dir_all(&root).unwrap();
}
