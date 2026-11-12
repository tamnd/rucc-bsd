//! The rusage of the NetBSD shims: conf/netbsd/toolchain/shim-cc.sh times each call with GNU time
//! into LOG.rusage, and scripts/netbsd/shim-records.py puts the times in the records that
//! `rb speed` reads.
#![cfg(unix)]

use harness_shim::record::read_log;
use std::path::{Path, PathBuf};
use std::process::Command;

/// A rucc that writes the object named by -o.
const FAKE_RUCC: &str = r#"#!/bin/sh
out=
while [ $# -gt 0 ]; do
	if [ "$1" = -o ]; then out=$2; shift; fi
	shift
done
if [ -n "$out" ]; then echo object > "$out"; fi
exit 0
"#;

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("rb-netbsd-shim-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn executable(path: &Path, text: &str) {
    std::fs::write(path, text).unwrap();
    Command::new("chmod").arg("+x").arg(path).status().unwrap();
}

/// GNU time, as overlay.sh finds it, or `None` on a host without it, such as macOS.
fn gnu_time() -> Option<&'static str> {
    let ok = Command::new("/usr/bin/time")
        .args(["-f", "%e", "-o", "/dev/null", "true"])
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success());
    ok.then_some("/usr/bin/time")
}

/// shim-records.py on LOG, as r-kernel.sh runs it.
fn records(log: &Path, side: &str, from: &str, to: &str) -> PathBuf {
    let out = log.with_extension("jsonl");
    let o = Command::new("python3")
        .arg("-I")
        .arg(repo().join("scripts/netbsd/shim-records.py"))
        .arg(log)
        .args([side, from, to])
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    std::fs::write(&out, &o.stdout).unwrap();
    out
}

#[test]
fn the_cc_shim_times_each_call() {
    let Some(time) = gnu_time() else {
        eprintln!("no GNU time on this host, so the shim does not time its calls");
        return;
    };
    let d = dir("cc");
    let rucc = d.join("rucc");
    executable(&rucc, FAKE_RUCC);
    let log = d.join("shim.tsv");
    let text = std::fs::read_to_string(repo().join("conf/netbsd/toolchain/shim-cc.sh"))
        .unwrap()
        .replace("@RUCC@", rucc.to_str().unwrap())
        .replace("@LOG@", log.to_str().unwrap())
        .replace("@TARGET@", "x86_64--netbsd")
        .replace("@GNUC@", "14.4.0")
        .replace("@SYSROOT@", "/dest")
        .replace("@TIME@", time)
        .replace("@LD_DIR@", "/tools/x86_64--netbsd/bin");
    let shim = d.join("x86_64--netbsd-gcc");
    executable(&shim, &text);
    std::fs::write(d.join("a.c"), "int a;\n").unwrap();
    let status = Command::new(&shim)
        .current_dir(&d)
        .args(["-O2", "-c", "a.c", "-o", "a.o"])
        .status()
        .unwrap();
    assert!(status.success());
    assert!(d.join("a.o").exists());

    let line = std::fs::read_to_string(&log).unwrap();
    let timed = std::fs::read_to_string(log.with_extension("tsv.rusage")).unwrap();
    let (head, rest) = timed.split_once('\t').unwrap();
    assert_eq!(rest, line, "the rusage line ends with the log line");
    let fields: Vec<&str> = head.split(' ').collect();
    assert_eq!(fields.len(), 4, "{head}");
    for f in &fields[..3] {
        assert!(f.parse::<f64>().is_ok(), "{head}");
    }
    assert!(fields[3].parse::<u64>().unwrap() > 0, "{head}");

    let (recs, bad) = read_log(&records(&log, "R", "", "")).unwrap();
    assert_eq!(bad, 0);
    assert_eq!(recs.len(), 1);
    assert!(recs[0].user_seconds.is_some());
    assert!(recs[0].peak_rss_kb.is_some_and(|k| k > 0));
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn the_records_carry_the_rusage_to_rb_speed() {
    let d = dir("records");
    let call = "0\t/nb/src/sys/kern\tcc\t-O2\t-c\tkern_x.c\t-o\t/nb/obj/kern_x.o";
    let probe = "0\t/nb/src/sys/kern\tcc\t--version";
    let g = d.join("g.tsv");
    std::fs::write(&g, format!("{call}\n{probe}\n")).unwrap();
    std::fs::write(
        d.join("g.tsv.rusage"),
        format!("0.50 0.40 0.10 81920\t{call}\nbad line\n"),
    )
    .unwrap();
    let r = d.join("r.tsv");
    std::fs::write(&r, format!("{call}\n{probe}\n")).unwrap();
    std::fs::write(
        d.join("r.tsv.rusage"),
        format!("0.30 0.20 0.05 40960\t{call}\n"),
    )
    .unwrap();

    let (g, bad_g) = read_log(&records(&g, "G", "/nb/obj", "/cmp/ref/obj")).unwrap();
    let (r, bad_r) = read_log(&records(&r, "R", "/nb/obj", "/cmp/rucc/obj")).unwrap();
    assert_eq!((bad_g, bad_r), (0, 0));
    let close = |a: Option<f64>, b: f64| a.is_some_and(|a| (a - b).abs() < 1e-9);
    assert!(close(Some(g[0].wall_seconds), 0.5));
    assert!(close(g[0].user_seconds, 0.4));
    assert!(close(g[0].system_seconds, 0.1));
    assert_eq!(g[0].peak_rss_kb, Some(81920));
    assert_eq!(g[1].user_seconds, None, "an untimed call has no rusage");

    let c = rb::speed::compare(&g, "/cmp/ref", &r, "/cmp/rucc");
    assert_eq!(c.paired.len(), 1, "{c:?}");
    assert_eq!(c.untimed, 0);
    let ratio = c.ratio().unwrap();
    assert!((ratio - 0.5).abs() < 1e-9, "{ratio}");
    assert_eq!(c.max_rss.as_ref().map(|m| m.0), Some(40960));
    let _ = std::fs::remove_dir_all(&d);
}
