//! `rb shim` end to end: the repository's personas and toolchains, a fake rucc, and the real
//! `rb-cc` when cargo built it next to `rb`, which `cargo test --workspace` does.
#![cfg(unix)]

use harness_shim::record::read_log;
use std::path::{Path, PathBuf};
use std::process::Command;

/// A rucc that prints a persona's `--version` and writes its arguments to `calls`.
const FAKE_RUCC: &str = r#"#!/bin/sh
echo "$*" >> "$(dirname "$0")/calls"
case " $* " in
*" --version "*) echo "gcc (rucc 0.30.15, GNU C persona 15.2.0) 15.2.0"; exit 0 ;;
esac
out=
while [ $# -gt 0 ]; do
	if [ "$1" = -o ]; then out=$2; shift; fi
	shift
done
if [ -n "$out" ]; then echo object > "$out"; fi
exit 0
"#;

const PINS: &str = r#"as_of = "2026-11-08T06:00:00Z"

[[pin]]
name = "freebsd-15.1"
system = "freebsd"
set = "releases"
kind = "release"
series = "15"
version = "15.1-RELEASE"
cadence = "weekly"
date = "2026-06-02"

[[pin]]
name = "netbsd-trunk"
system = "netbsd"
set = "current"
kind = "branch"
series = "current"
version = "11.99.10"
cadence = "nightly"
date = "2026-11-04T00:00:00Z"
gcc = "14.4.0"
"#;

fn rig(tag: &str) -> PathBuf {
    let top = std::env::temp_dir().join(format!("rb-shim-cli-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&top);
    let root = top.join("repo");
    std::fs::create_dir_all(root.join("bin")).unwrap();
    let real = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    for file in ["sets.toml", "rows.toml", "personas.toml", "toolchains.toml"] {
        std::fs::copy(real.join(file), root.join(file)).unwrap();
    }
    std::fs::write(root.join("pins.toml"), PINS).unwrap();
    let rucc = root.join("bin/rucc");
    std::fs::write(&rucc, FAKE_RUCC).unwrap();
    Command::new("chmod").arg("+x").arg(&rucc).status().unwrap();
    top
}

fn rb(top: &Path, args: &[&str]) -> (bool, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_rb"))
        .args(args)
        .env("RB_ROOT", top.join("repo"))
        .env_remove("RB_RUCC")
        .env_remove("RB_SHIM")
        .env(
            "PATH",
            format!("{}:/usr/bin:/bin", top.join("repo/bin").display()),
        )
        .output()
        .unwrap();
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    (out.status.success(), text)
}

/// The real shim, or a stand-in file when cargo did not build it.
fn shim(top: &Path) -> (PathBuf, bool) {
    let real = Path::new(env!("CARGO_BIN_EXE_rb")).with_file_name("rb-cc");
    if real.is_file() {
        (real, true)
    } else {
        let fake = top.join("rb-cc");
        std::fs::write(&fake, "").unwrap();
        (fake, false)
    }
}

#[test]
fn freebsd_shims_run_rucc_with_the_persona_and_record_its_objects() {
    let top = rig("freebsd");
    let (binary, real) = shim(&top);
    let dir = top.join("shims");
    let log = top.join("compile.jsonl");
    let (ok, text) = rb(
        &top,
        &[
            "shim",
            "freebsd-15.1",
            "--dir",
            dir.to_str().unwrap(),
            "--log",
            log.to_str().unwrap(),
            "--shim",
            binary.to_str().unwrap(),
        ],
    );
    assert!(ok, "{text}");
    assert!(
        text.contains("--target=x86_64-unknown-freebsd15.1 -fgnuc-version=15.2.0"),
        "{text}"
    );
    assert!(text.contains("not the pinned v0.30.15"), "{text}");
    let config = std::fs::read_to_string(dir.join("rb-shim.toml")).unwrap();
    assert!(
        config.contains("x86_64-unknown-freebsd15.1-gcc15"),
        "{config}"
    );
    for name in ["rb-cc-rucc", "rb-cc-ref", "rb-c++", "rb-cpp", "rb-as"] {
        assert!(
            dir.join(format!("x86_64-unknown-freebsd15.1-{name}"))
                .is_file(),
            "{name}"
        );
    }
    if !real {
        eprintln!("rb-cc is not built next to rb, so the shim is not called");
        return;
    }
    let work = top.join("work");
    std::fs::create_dir_all(&work).unwrap();
    std::fs::write(work.join("a.c"), "int a;\n").unwrap();
    let status = Command::new(dir.join("x86_64-unknown-freebsd15.1-rb-cc-rucc"))
        .args(["-O2", "-c", "a.c", "-o", "a.o"])
        .current_dir(&work)
        .env_remove("RB_SIDE")
        .env_remove("RB_RUCC")
        .env_remove("RB_RUCC_ARGS")
        .env_remove("RB_COMPILE_LOG")
        .status()
        .unwrap();
    assert!(status.success());
    let calls = std::fs::read_to_string(top.join("repo/bin/calls")).unwrap();
    assert!(
        calls.contains(
            "--target=x86_64-unknown-freebsd15.1 -fgnuc-version=15.2.0 -O2 -c a.c -o a.o"
        ),
        "{calls}"
    );
    let (records, _) = read_log(&log).unwrap();
    let last = records.last().unwrap();
    assert_eq!(last.origin.as_deref(), Some("rucc-c"));
    assert_eq!(last.side.as_deref(), Some("R"));
    std::fs::remove_dir_all(&top).ok();
}

#[test]
fn netbsd_shims_need_the_tools_and_a_side() {
    let top = rig("netbsd");
    let (binary, _) = shim(&top);
    let dir = top.join("tooldir-r/bin");
    let base = [
        "shim",
        "netbsd-trunk",
        "--dir",
        dir.to_str().unwrap(),
        "--shim",
        binary.to_str().unwrap(),
    ];
    let (ok, text) = rb(&top, &base);
    assert!(!ok && text.contains("--tools"), "{text}");
    let mut full = base.to_vec();
    full.extend([
        "--tools",
        "/nb/tools",
        "--destdir",
        "/nb/destdir",
        "--cc",
        "rucc",
    ]);
    let (ok, text) = rb(&top, &full);
    assert!(ok, "{text}");
    assert!(text.contains("side: R"), "{text}");
    assert!(dir.join("x86_64--netbsd-gcc").is_file());
    assert!(dir.join("x86_64--netbsd-cpp").is_file());
    let config = std::fs::read_to_string(dir.join("rb-shim.toml")).unwrap();
    assert!(
        config.contains("rucc-sysroot = \"/nb/destdir\""),
        "{config}"
    );
    assert!(
        config.contains("--target=x86_64-unknown-netbsd11.99"),
        "{config}"
    );
    let (ok, text) = rb(
        &top,
        &[
            "shim",
            "netbsd-trunk",
            "--dir",
            dir.to_str().unwrap(),
            "--cc",
            "rucc",
            "--rucc",
            "/no/such/rucc",
            "--tools",
            "/nb/tools",
        ],
    );
    assert!(!ok && text.contains("/no/such/rucc"), "{text}");
    std::fs::remove_dir_all(&top).ok();
}

#[test]
fn freebsd_toolchain_files_differ_in_xcc_only() {
    let top = rig("fbsd-tc");
    let (binary, _) = shim(&top);
    let dir = top.join("tc");
    let make = [
        "toolchain",
        "freebsd-15.1",
        "--dir",
        dir.to_str().unwrap(),
        "--shim",
        binary.to_str().unwrap(),
    ];
    // The fake rucc is not the pinned release.
    let (ok, text) = rb(&top, &make);
    assert!(!ok && text.contains("pass --ungraded"), "{text}");
    let mut ungraded = make.to_vec();
    ungraded.push("--ungraded");
    let (ok, text) = rb(&top, &ungraded);
    assert!(ok, "{text}");
    assert!(text.contains("differ in the C compiler only"), "{text}");
    let r = std::fs::read_to_string(dir.join("rucc.mk")).unwrap();
    assert!(r.contains("-rb-cc-rucc\n"), "{r}");
    assert!(
        dir.join("bin/x86_64-unknown-freebsd15.1-rb-cc-rucc")
            .is_file()
    );

    let diff = [
        "toolchain-diff",
        "freebsd-15.1",
        "--dir",
        dir.to_str().unwrap(),
        "--ungraded",
    ];
    let (ok, text) = rb(&top, &diff);
    assert!(ok, "{text}");
    std::fs::write(
        dir.join("rucc.mk"),
        r.replace("X_COMPILER_TYPE=gcc", "X_COMPILER_TYPE=clang"),
    )
    .unwrap();
    let (ok, text) = rb(&top, &diff);
    assert!(
        !ok && text.contains("X_COMPILER_TYPE: G has gcc, R has clang"),
        "{text}"
    );
    std::fs::remove_dir_all(&top).ok();
}

#[test]
fn netbsd_tooldirs_differ_from_the_tools_in_the_shim_only() {
    let top = rig("nb-tc");
    let (binary, _) = shim(&top);
    let tools = top.join("tools");
    std::fs::create_dir_all(tools.join("bin")).unwrap();
    std::fs::create_dir_all(tools.join("x86_64--netbsd/lib")).unwrap();
    for (file, text) in [
        ("bin/x86_64--netbsd-gcc", "gcc"),
        ("bin/x86_64--netbsd-cpp", "cpp"),
        ("bin/x86_64--netbsd-as", "as"),
        ("bin/nbmake-amd64", "old wrapper"),
        ("x86_64--netbsd/lib/ldscripts", "ld"),
    ] {
        std::fs::write(tools.join(file), text).unwrap();
    }
    let side = |cc: &str, ungraded: bool| {
        let dir = top.join(format!("tooldir-{cc}"));
        let mut args = vec![
            "toolchain".to_string(),
            "netbsd-trunk".into(),
            "--dir".into(),
            dir.display().to_string(),
            "--tools".into(),
            tools.display().to_string(),
            "--destdir".into(),
            top.join("destdir").display().to_string(),
            "--cc".into(),
            cc.into(),
            "--shim".into(),
            binary.display().to_string(),
        ];
        if ungraded {
            args.push("--ungraded".into());
        }
        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        let (ok, text) = rb(&top, &refs);
        (ok, text, dir)
    };
    let (ok, text, g) = side("ref", false);
    assert!(ok, "{text}");
    assert!(!g.join("bin/nbmake-amd64").exists());
    let (ok, text, _) = side("rucc", false);
    assert!(!ok && text.contains("pass --ungraded"), "{text}");
    let (ok, text, r) = side("rucc", true);
    assert!(ok, "{text}");
    assert_eq!(
        std::fs::read(r.join("x86_64--netbsd/lib/ldscripts")).unwrap(),
        b"ld"
    );

    let diff = [
        "toolchain-diff",
        "netbsd-trunk",
        "--g",
        g.to_str().unwrap(),
        "--r",
        r.to_str().unwrap(),
        "--ungraded",
    ];
    let (ok, text) = rb(&top, &diff);
    assert!(ok, "{text}");
    assert!(
        text.contains("differ in the side of rb-shim.toml only"),
        "{text}"
    );
    std::fs::write(r.join("bin/x86_64--netbsd-as"), "patched").unwrap();
    let (ok, text) = rb(&top, &diff);
    assert!(
        !ok && text.contains("bin/x86_64--netbsd-as differs"),
        "{text}"
    );
    std::fs::remove_dir_all(&top).ok();
}

#[test]
fn probes_compare_what_the_build_reads() {
    let top = rig("probes");
    let gcc = top.join("gcc");
    std::fs::write(
        &gcc,
        "#!/bin/sh\ncase \" $* \" in *\" --version \"*) echo 'x86_64-unknown-freebsd15.1-gcc15 (FreeBSD Ports Collection for amd64) 15.2.0'; echo 'Copyright (C) 2025 Free Software Foundation, Inc.' ;; esac\nexit 0\n",
    )
    .unwrap();
    Command::new("chmod").arg("+x").arg(&gcc).status().unwrap();
    let work = top.join("work");
    let rucc = top.join("repo/bin/rucc");
    let args = [
        "probes",
        "freebsd-15.1",
        "--g",
        gcc.to_str().unwrap(),
        "--r",
        rucc.to_str().unwrap(),
        "--work",
        work.to_str().unwrap(),
    ];
    let (ok, text) = rb(&top, &args);
    assert!(ok, "{text}");
    assert!(text.contains("26 asked, 26 the same, 0 differ"), "{text}");
    std::fs::write(
        &gcc,
        "#!/bin/sh\ncase \" $* \" in *\" -dM \"*) echo '#define __GNUC__ 15' ;; esac\nexit 0\n",
    )
    .unwrap();
    let (ok, text) = rb(&top, &args);
    assert!(
        !ok && text.contains("__GNUC__: G 15, R undefined"),
        "{text}"
    );
    std::fs::remove_dir_all(&top).ok();
}

#[test]
fn netbsd_build_runs_build_sh_with_the_shims_tooldir() {
    let top = rig("nb-build");
    let (binary, _) = shim(&top);
    let root = top.join("nb");
    let tools = root.join("tools/bin");
    std::fs::create_dir_all(&tools).unwrap();
    for name in ["x86_64--netbsd-gcc", "x86_64--netbsd-cpp", "nbmake-amd64"] {
        std::fs::write(tools.join(name), name).unwrap();
    }
    // A build.sh that writes its arguments where the test can read them.
    let src = root.join("src");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(
        src.join("build.sh"),
        "#!/bin/sh\necho \"build.sh $*\" >> ../calls\necho building \"$*\"\nexit 0\n",
    )
    .unwrap();
    Command::new("chmod")
        .arg("+x")
        .arg(src.join("build.sh"))
        .status()
        .unwrap();
    let base = [
        "build",
        "netbsd-trunk",
        "--cc",
        "rucc",
        "--root",
        root.to_str().unwrap(),
        "--jobs",
        "2",
        "--targets",
        "kernel=GENERIC",
        "--shim",
        binary.to_str().unwrap(),
    ];
    let mut args = base.to_vec();
    args.push("--ungraded");
    let (ok, text) = rb(&top, &args);
    assert!(ok, "{text}");
    assert!(text.contains("every target built"), "{text}");
    let calls = std::fs::read_to_string(root.join("calls")).unwrap();
    assert!(
        calls.contains(&format!(
            "-T {} -D {} -R {} -u kernel=GENERIC",
            root.join("toolchain-rucc").display(),
            root.join("destdir").display(),
            root.join("release").display()
        )),
        "{calls}"
    );
    let config = std::fs::read_to_string(root.join("toolchain-rucc/bin/rb-shim.toml")).unwrap();
    assert!(config.contains("compile-rucc.jsonl"), "{config}");
    let log = std::fs::read_to_string(root.join("logs/kernel.log")).unwrap();
    assert!(log.starts_with("building"), "{log}");
    let tsv = std::fs::read_to_string(root.join("logs/phases.tsv")).unwrap();
    assert!(tsv.starts_with("kernel\t0\t"), "{tsv}");

    // Without --ungraded the fake rucc stops the build before build.sh runs.
    std::fs::remove_file(root.join("calls")).unwrap();
    let (ok, text) = rb(&top, &base);
    assert!(!ok && text.contains("pass --ungraded"), "{text}");
    assert!(!root.join("calls").exists());
    std::fs::remove_dir_all(&top).ok();
}

#[test]
fn freebsd_build_dry_run_names_the_toolchain_file() {
    let top = rig("fb-build");
    let (ok, text) = rb(
        &top,
        &[
            "build",
            "freebsd-15.1",
            "--cc",
            "rucc",
            "--root",
            "/b",
            "--src",
            "/usr/src",
            "--jobs",
            "8",
            "--dry-run",
        ],
    );
    assert!(ok, "{text}");
    assert!(
        text.contains("make -C /usr/src -j8 buildworld TARGET=amd64 TARGET_ARCH=amd64 KERNCONF=GENERIC CROSS_TOOLCHAIN=/b/toolchain-rucc/rucc.mk"),
        "{text}"
    );
    let (ok, text) = rb(
        &top,
        &[
            "build",
            "freebsd-15.1",
            "--cc",
            "ref",
            "--no-shim",
            "--root",
            "/b",
            "--dry-run",
        ],
    );
    assert!(
        ok && text.contains("CROSS_TOOLCHAIN=amd64-gcc15 "),
        "{text}"
    );
    std::fs::remove_dir_all(&top).ok();
}
