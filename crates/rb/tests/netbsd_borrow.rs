//! scripts/netbsd/borrow.sh, which gives the c-only image the tests set, ATF and kyua of the graded
//! R build (plan 10.5).
#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::Command;

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("rb-netbsd-borrow-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// A set: a .tgz of the files, each a path and its text, with the names that start with ./ as
/// in the NetBSD sets.
fn set(sets: &Path, name: &str, files: &[(&str, &str)]) {
    let root = sets.join(format!("{name}.root"));
    std::fs::create_dir_all(&root).unwrap();
    for (path, text) in files {
        let p = root.join(path);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, text).unwrap();
    }
    let ok = Command::new("tar")
        .arg("-C")
        .arg(&root)
        .arg("-czf")
        .arg(sets.join(format!("{name}.tgz")))
        .arg(".")
        .status()
        .unwrap();
    assert!(ok.success());
    std::fs::remove_dir_all(&root).unwrap();
}

fn names(archive: &Path) -> Vec<String> {
    let o = Command::new("tar")
        .arg("-tf")
        .arg(archive)
        .output()
        .unwrap();
    assert!(o.status.success());
    let mut v: Vec<String> = String::from_utf8(o.stdout)
        .unwrap()
        .lines()
        .map(|l| l.trim_end_matches('/').to_string())
        .collect();
    v.sort();
    v
}

const R_DIST: &str = "\
/set type=dir uname=root gname=wheel mode=0755
./etc
./etc/mtree
./usr
./usr/bin
./usr/lib
./usr/share
./usr/share/atf
./usr/share/misc
./usr/tests mode=0555
";

const C_DIST: &str = "\
/set type=dir uname=root gname=wheel mode=0755
./etc
./etc/mtree
./usr
./usr/bin
./usr/lib
./usr/share
./usr/share/misc
";

#[test]
fn the_c_only_sets_get_the_tests_set_and_what_runs_it() {
    let d = dir("sets");
    let (r, c) = (d.join("r"), d.join("c"));
    std::fs::create_dir_all(&r).unwrap();
    std::fs::create_dir_all(&c).unwrap();
    let file = "type=file uname=root gname=wheel mode=0555";
    let r_base_spec = format!(
        "./usr/bin/ls {file}\n./usr/bin/atf-run {file}\n./usr/lib/libc.so.12 {file}\n\
         ./usr/lib/libstdc++.so.9 {file}\n./usr/share/misc/x {file}\n\
         ./usr/share/atf/atf-run.hooks {file}\n"
    );
    let r_tests_spec = "./usr/tests/Atffile type=file uname=root gname=wheel mode=0444\n";
    set(
        &r,
        "base",
        &[
            ("usr/bin/ls", "ls"),
            ("usr/bin/atf-run", "atf-run"),
            ("usr/lib/libc.so.12", "libc"),
            ("usr/share/misc/x", "x"),
            ("usr/lib/libstdc++.so.9", "libstdc++"),
            ("usr/share/atf/atf-run.hooks", "hooks"),
        ],
    );
    set(
        &r,
        "tests",
        &[(
            "usr/tests/Atffile",
            "Content-Type: application/X-atf-atffile",
        )],
    );
    set(
        &r,
        "etc",
        &[
            ("etc/mtree/NetBSD.dist", R_DIST),
            ("etc/mtree/set.base", &r_base_spec),
            ("etc/mtree/set.tests", r_tests_spec),
        ],
    );
    let c_base = [
        ("usr/bin/ls", "ls"),
        ("usr/lib/libc.so.12", "libc"),
        ("usr/share/misc/x", "x"),
    ];
    set(&c, "base", &c_base);
    set(&c, "comp", &[("usr/lib/libc.a", "libc")]);
    set(&c, "kern-GENERIC", &[("netbsd", "kernel")]);
    set(
        &c,
        "etc",
        &[
            ("etc/mtree/NetBSD.dist", C_DIST),
            (
                "etc/mtree/set.base",
                &format!(
                    "./usr/bin/ls {file}\n./usr/lib/libc.so.12 {file}\n./usr/share/misc/x {file}\n"
                ),
            ),
        ],
    );

    let list = d.join("borrowed.txt");
    let o = Command::new("bash")
        .arg(repo().join("scripts/netbsd/borrow.sh"))
        .args([&r, &c, &list])
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&o.stderr);
    assert!(o.status.success(), "{stderr}");
    assert_eq!(
        String::from_utf8_lossy(&o.stdout),
        "base etc comp tests borrowed\n"
    );
    assert!(
        stderr.contains("3 paths of the tests set and 4 of the base set"),
        "{stderr}"
    );

    // The tests set is R's, as it is.
    assert_eq!(
        std::fs::read(c.join("tests.tgz")).unwrap(),
        std::fs::read(r.join("tests.tgz")).unwrap()
    );
    // The borrowed set has what the c-only base set does not have, and the spec lines for it.
    let borrowed = names(&c.join("borrowed.tgz"));
    for want in [
        "./usr/bin/atf-run",
        "./usr/lib/libstdc++.so.9",
        "./usr/share/atf/atf-run.hooks",
        "./etc/mtree/set.borrowed",
    ] {
        assert!(
            borrowed.iter().any(|n| n == want),
            "{want} not in {borrowed:?}"
        );
    }
    assert!(
        !borrowed.iter().any(|n| n == "./usr/bin/ls"),
        "{borrowed:?}"
    );
    let x = d.join("x");
    std::fs::create_dir_all(&x).unwrap();
    let ok = Command::new("tar")
        .arg("-xzf")
        .arg(c.join("borrowed.tgz"))
        .arg("-C")
        .arg(&x)
        .status()
        .unwrap();
    assert!(ok.success());
    let spec = std::fs::read_to_string(x.join("etc/mtree/set.borrowed")).unwrap();
    let lines: Vec<&str> = spec.lines().skip(1).collect();
    assert_eq!(
        lines,
        [
            "./usr/share/atf type=dir uname=root gname=wheel mode=0755",
            "./usr/tests type=dir uname=root gname=wheel mode=0555",
            "./usr/bin/atf-run type=file uname=root gname=wheel mode=0555",
            "./usr/lib/libstdc++.so.9 type=file uname=root gname=wheel mode=0555",
            "./usr/share/atf/atf-run.hooks type=file uname=root gname=wheel mode=0555",
            "./usr/tests/Atffile type=file uname=root gname=wheel mode=0444",
            "./etc/mtree/set.borrowed type=file uname=root gname=wheel mode=0444",
        ]
    );
    let list = std::fs::read_to_string(&list).unwrap();
    assert_eq!(
        list,
        "tests\t./usr\ntests\t./usr/tests\ntests\t./usr/tests/Atffile\nbase\t./usr/bin/atf-run\n\
         base\t./usr/lib/libstdc++.so.9\nbase\t./usr/share/atf\nbase\t./usr/share/atf/atf-run.hooks\n"
    );

    // A second run refuses, so the sets are not borrowed twice.
    let again = Command::new("bash")
        .arg(repo().join("scripts/netbsd/borrow.sh"))
        .args([&r, &c, &d.join("again.txt")])
        .output()
        .unwrap();
    assert!(!again.status.success());
    assert!(String::from_utf8_lossy(&again.stderr).contains("already has tests.tgz"));
    std::fs::remove_dir_all(&d).unwrap();
}
