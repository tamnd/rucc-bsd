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
    let disk = root.join("work/in/modules");
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
