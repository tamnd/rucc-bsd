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

/// The sets of the graded R build and of the c-only build. The R base set has ATF, its hooks
/// and libstdc++, which the c-only base set does not have. As in NetBSD, each set has its own
/// etc/mtree/set.NAME, and the etc set has `NetBSD.dist`.
fn fake_sets(rsets: &Path, csets: &Path) {
    std::fs::create_dir_all(rsets).unwrap();
    std::fs::create_dir_all(csets).unwrap();
    let file = "type=file uname=root gname=wheel mode=0555";
    let r_base_spec = format!(
        "./usr/bin/ls {file}\n./usr/bin/atf-run {file}\n./usr/lib/libc.so.12 {file}\n\
         ./usr/lib/libstdc++.so.9 {file}\n./usr/share/misc/x {file}\n\
         ./usr/share/atf/atf-run.hooks {file}\n"
    );
    let r_tests_spec = "./usr/tests/Atffile type=file uname=root gname=wheel mode=0444\n";
    set(
        rsets,
        "base",
        &[
            ("usr/bin/ls", "ls"),
            ("usr/bin/atf-run", "atf-run"),
            ("usr/lib/libc.so.12", "libc"),
            ("usr/share/misc/x", "x"),
            ("usr/lib/libstdc++.so.9", "libstdc++"),
            ("usr/share/atf/atf-run.hooks", "hooks"),
            ("etc/mtree/set.base", &r_base_spec),
        ],
    );
    set(
        rsets,
        "tests",
        &[
            (
                "usr/tests/Atffile",
                "Content-Type: application/X-atf-atffile",
            ),
            ("etc/mtree/set.tests", r_tests_spec),
        ],
    );
    set(rsets, "etc", &[("etc/mtree/NetBSD.dist", R_DIST)]);
    let c_base_spec =
        format!("./usr/bin/ls {file}\n./usr/lib/libc.so.12 {file}\n./usr/share/misc/x {file}\n");
    let c_base = [
        ("usr/bin/ls", "ls"),
        ("usr/lib/libc.so.12", "libc"),
        ("usr/share/misc/x", "x"),
        ("etc/mtree/set.base", &c_base_spec),
    ];
    set(csets, "base", &c_base);
    set(csets, "comp", &[("usr/lib/libc.a", "libc")]);
    set(csets, "kern-GENERIC", &[("netbsd", "kernel")]);
    set(csets, "etc", &[("etc/mtree/NetBSD.dist", C_DIST)]);
}

fn borrow(rsets: &Path, csets: &Path, list: &Path) -> std::process::Output {
    Command::new("bash")
        .arg(repo().join("scripts/netbsd/borrow.sh"))
        .args([rsets, csets, list])
        .output()
        .unwrap()
}

/// The borrowed set has what the c-only base set does not have, and the spec lines for it.
fn check_borrowed(csets: &Path, work: &Path) {
    let borrowed = names(&csets.join("borrowed.tgz"));
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
    let root = work.join("x");
    std::fs::create_dir_all(&root).unwrap();
    let ok = Command::new("tar")
        .arg("-xzf")
        .arg(csets.join("borrowed.tgz"))
        .arg("-C")
        .arg(&root)
        .status()
        .unwrap();
    assert!(ok.success());
    let spec = std::fs::read_to_string(root.join("etc/mtree/set.borrowed")).unwrap();
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
}

#[test]
fn the_c_only_sets_get_the_tests_set_and_what_runs_it() {
    let work = dir("sets");
    let (rsets, csets) = (work.join("r"), work.join("c"));
    fake_sets(&rsets, &csets);

    let list = work.join("borrowed.txt");
    let out = borrow(&rsets, &csets, &list);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{stderr}");
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "base etc comp tests borrowed\n"
    );
    assert!(
        stderr.contains("6 paths of the tests set and 4 of the base set"),
        "{stderr}"
    );

    // The tests set is R's, as it is.
    assert_eq!(
        std::fs::read(csets.join("tests.tgz")).unwrap(),
        std::fs::read(rsets.join("tests.tgz")).unwrap()
    );
    check_borrowed(&csets, &work);
    let list = std::fs::read_to_string(&list).unwrap();
    assert_eq!(
        list,
        "tests\t./etc\ntests\t./etc/mtree\ntests\t./etc/mtree/set.tests\ntests\t./usr\n\
         tests\t./usr/tests\ntests\t./usr/tests/Atffile\nbase\t./usr/bin/atf-run\n\
         base\t./usr/lib/libstdc++.so.9\nbase\t./usr/share/atf\nbase\t./usr/share/atf/atf-run.hooks\n"
    );

    // A second run refuses, so the sets are not borrowed twice.
    let again = borrow(&rsets, &csets, &work.join("again.txt"));
    assert!(!again.status.success());
    assert!(String::from_utf8_lossy(&again.stderr).contains("already has tests.tgz"));
    std::fs::remove_dir_all(&work).unwrap();
}
