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
