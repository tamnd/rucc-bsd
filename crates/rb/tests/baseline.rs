//! rb baseline over three boots of a stand-in for QEMU. The stand-in plays a FreeBSD guest that
//! runs the smoke and tests suites and writes rb-rc's files to the results disk. One test case
//! fails in the second run only, so it must come out as `g-unstable`.
#![cfg(unix)]

use rb::baseline::{self, Meta};
use rb::boot::{Bus, Firmware, Request};
use rb::cli::{Args, accepted};
use rb_image::conf::RbConf;
use rb_pins::places::Rows;
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};

const GUEST: &str = r#"#!/bin/sh
here=$(cd "$(dirname "$0")" && pwd)
for a in "$@"; do
	case $a in
	*id=rb,file=*) disk=${a#*id=rb,file=}; disk=${disk%%,*} ;;
	esac
done
n=$(($(cat "$here/count" 2>/dev/null || echo 0) + 1))
echo $n > "$here/count"
rm -rf "$here/out" && mkdir -p "$here/out/smoke" "$here/out/tests"
echo "RB-READY FreeBSD rb 15.1-RELEASE amd64"
printf 'pass shells\npass network\nskip package\n' > "$here/out/smoke/results"
if [ $n = 2 ]; then b=failed; else b=passed; fi
cat > "$here/out/tests/kyua.log" <<EOF
bin/sh/builtins:a  ->  passed  [0.0${n}0s]
bin/sh/builtins:b  ->  $b  [0.100s]
sys/kern/x:y  ->  expected_failure: PR 1  [0.001s]
EOF
cat "$here/out/tests/kyua.log"
printf 'vtnet0: link state changed to UP\nTimecounter "TSC" frequency 359123%d Hz\n' $n \
	> "$here/out/dmesg.end"
tar -cf "$disk" -C "$here/out" .
echo "RB-DONE boot=pass smoke=2/2 tests=2/3 archive=disk"
"#;

fn rig(tag: &str) -> PathBuf {
    let top = std::env::temp_dir().join(format!("rb-baseline-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&top);
    std::fs::create_dir_all(top.join("bin")).unwrap();
    for (name, text) in [
        ("qemu-system-x86_64", GUEST),
        (
            "qemu-img",
            "#!/bin/sh\neval last=\\${$#}\necho overlay > \"$last\"\n",
        ),
    ] {
        let path = top.join("bin").join(name);
        std::fs::write(&path, text).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    std::fs::write(top.join("g.img"), "the image").unwrap();
    top
}

fn request(top: &Path, out: &Path) -> Request {
    Request {
        image: top.join("g.img"),
        out: out.to_path_buf(),
        conf: RbConf {
            suites: vec!["boot".into(), "smoke".into(), "tests".into()],
            ..RbConf::default()
        },
        qemu: top.join("bin/qemu-system-x86_64").display().to_string(),
        machine: "q35".into(),
        kvm: false,
        cpu: None,
        virtio: Bus::Pci,
        cpus: 2,
        memory: "1G".into(),
        firmware: Firmware::Seabios,
        boot_timeout: 20,
        timeout: 60,
        poweroff_timeout: 20,
        ddb_wait: 1,
        disk_size: 1 << 20,
        spare_disks: 0,
        reboots: 0,
        in_place: false,
        keep: false,
    }
}

#[test]
fn three_boots() {
    let top = rig("three");
    let runs = baseline::boot_runs(3, &top.join("work"), |out| Ok(request(&top, out))).unwrap();
    assert_eq!(runs.len(), 3);
    assert!(runs.iter().all(|r| r.outcome.passed()));
    let meta = Meta {
        pin: "freebsd-15.1".into(),
        system: "freebsd".into(),
        row: "amd64".into(),
        conf: "graded".into(),
        ..Meta::default()
    };
    let w = baseline::write(&top.join("baselines"), &meta, &runs).unwrap();
    assert!(w.complete);
    // boot, 3 smoke checks and 3 test cases.
    assert_eq!((w.cases, w.stable_pass), (7, 4));
    let read = |name: &str| std::fs::read_to_string(w.dir.join(name)).unwrap();
    assert_eq!(
        read("stable-pass.txt"),
        "boot\tboot\nsmoke\tnetwork\nsmoke\tshells\ntests\tbin/sh/builtins:a\n"
    );
    assert!(read("cases.tsv").contains(
        "tests\tbin/sh/builtins:b\tpassed\tfailed\tpassed\t0.100\t0.100\t0.100\tg-unstable\n"
    ));
    assert!(read("cases.tsv").contains(
        "tests\tbin/sh/builtins:a\tpassed\tpassed\tpassed\t0.010\t0.020\t0.030\tstable-pass\n"
    ));
    // The changing frequency is one message, seen in all three runs.
    assert!(read("kernel-log.txt").contains("3\tTimecounter \"TSC\" frequency # Hz\n"));
    std::fs::remove_dir_all(&top).unwrap();
}

#[test]
fn from_earlier_boots() {
    let top = rig("from");
    for n in 1..=3 {
        let out = top.join(format!("boot-{n}"));
        let o = rb::boot::run(&request(&top, &out)).unwrap();
        assert!(o.passed(), "{o:?}");
    }
    let from = (1..=3)
        .map(|n| top.join(format!("boot-{n}")).display().to_string())
        .collect::<Vec<_>>()
        .join(",");
    let dir = top.join("baselines").display().to_string();
    let words: Vec<String> = [
        "baseline", "netbsd-x", "--system", "netbsd", "--conf", "graded", "--from", &from, "--dir",
        &dir,
    ]
    .iter()
    .map(|w| (*w).to_string())
    .collect();
    let args = Args::parse(&words, accepted).unwrap();
    let rows = Rows::parse(include_str!("../../../rows.toml")).unwrap();
    let (report, complete) = baseline::cli::run(&args, &top, None, &rows).unwrap();
    assert!(complete, "{report}");
    assert!(
        report.contains("4 of 7 cases passed in every run."),
        "{report}"
    );
    let toml_text =
        std::fs::read_to_string(top.join("baselines/netbsd/amd64/x/graded/baseline.toml")).unwrap();
    // The image hash comes from the image the boots used: sha256 of "the image".
    assert!(toml_text.contains("image = \"g.img\""), "{toml_text}");
    let sha = "97e9d7a16021e6d07fbe28cce398f5847fc00c574991fbff65d95fd5e032d389";
    assert!(
        toml_text.contains(&format!("image_sha256 = \"{sha}\"")),
        "{toml_text}"
    );
    std::fs::remove_dir_all(&top).unwrap();
}
