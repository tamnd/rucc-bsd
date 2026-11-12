//! scripts/tcg/boot.sh, which makes a tcg boot again when its kernel panics, with a stub of rb.
#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("rb-tcg-boot-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// A stub of rb: the boot that the file "panics" counts down panics, and the boots after it
/// pass. Each boot appends its arguments to "args".
fn stub(d: &Path, panics: u32) -> PathBuf {
    std::fs::write(d.join("panics"), format!("{panics}\n")).unwrap();
    let rb = d.join("rb");
    let text = format!(
        r##"#!/bin/sh
d={d}
echo "$*" >> "$d/args"
while [ $# -gt 0 ]; do
	case $1 in
	--out) out=$2; shift ;;
	esac
	shift
done
mkdir -p "$out"
n=$(cat "$d/panics")
if [ "$n" -gt 0 ]; then
	echo $((n - 1)) > "$d/panics"
	printf '{{"result": "panic", "seen": {{"panic": "panic: softints stuck", "panic_case": "lib/t_a:a"}}}}\n' > "$out/boot.json"
	echo "# rb boot: panic"
	exit 1
fi
printf '{{"result": "pass", "seen": {{}}}}\n' > "$out/boot.json"
echo "# rb boot: pass"
"##,
        d = d.display()
    );
    std::fs::write(&rb, text).unwrap();
    std::fs::set_permissions(&rb, std::fs::Permissions::from_mode(0o755)).unwrap();
    rb
}

fn boot(d: &Path, seconds: &str, min_left: &str) -> Output {
    Command::new("bash")
        .arg(repo().join("scripts/tcg/boot.sh"))
        .arg(d.join("out"))
        .arg(seconds)
        .arg(d.join("rb"))
        .args(["--image", "x.img", "--suite", "boot"])
        .env("RB_SUMMARY", d.join("summary"))
        .env("RB_MIN_LEFT", min_left)
        .output()
        .unwrap()
}

fn read(p: &Path) -> String {
    std::fs::read_to_string(p).unwrap_or_default()
}

#[test]
fn a_panic_is_booted_again_and_kept() {
    let d = dir("again");
    stub(&d, 2);
    let o = boot(&d, "3600", "1800");
    assert!(o.status.success(), "{o:?}");
    let args = read(&d.join("args"));
    assert_eq!(args.lines().count(), 3, "{args}");
    assert!(
        args.lines()
            .all(|l| l.starts_with("boot --image x.img --suite boot --out ")),
        "{args}"
    );
    for n in [1, 2] {
        let kept = read(&d.join(format!("out/panic-{n}/boot.json")));
        assert!(kept.contains("\"panic\""), "panic-{n}: {kept}");
    }
    assert!(read(&d.join("out/boot.json")).contains("\"pass\""));
    let summary = read(&d.join("summary"));
    assert!(
        summary.contains(
            "The kernel panicked: panic: softints stuck, at lib/t_a:a. The boot is made again (2 of at most 3)"
        ),
        "{summary}"
    );
    assert!(
        summary.contains("The boot passed after 2 panicked boots."),
        "{summary}"
    );
    std::fs::remove_dir_all(&d).unwrap();
}

#[test]
fn three_panics_fail() {
    let d = dir("three");
    stub(&d, 5);
    let o = boot(&d, "3600", "1800");
    assert_eq!(o.status.code(), Some(1), "{o:?}");
    assert_eq!(read(&d.join("args")).lines().count(), 3);
    assert!(d.join("out/panic-2").is_dir());
    assert!(!d.join("out/panic-3").exists());
    assert!(read(&d.join("summary")).contains("The boot failed after 2 panicked boots."));
    std::fs::remove_dir_all(&d).unwrap();
}

#[test]
fn no_retry_without_the_time() {
    let d = dir("time");
    stub(&d, 1);
    let o = boot(&d, "600", "1800");
    assert_eq!(o.status.code(), Some(1), "{o:?}");
    assert_eq!(read(&d.join("args")).lines().count(), 1);
    assert!(read(&d.join("summary")).contains("so the boot is not made again"));
    assert!(!d.join("out/panic-1").exists());
    std::fs::remove_dir_all(&d).unwrap();
}

#[test]
fn a_pass_boots_once() {
    let d = dir("pass");
    stub(&d, 0);
    let o = boot(&d, "3600", "1800");
    assert!(o.status.success(), "{o:?}");
    assert_eq!(read(&d.join("args")).lines().count(), 1);
    assert!(!read(&d.join("summary")).contains("panicked"));
    std::fs::remove_dir_all(&d).unwrap();
}
