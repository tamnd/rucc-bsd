//! The shim under its real names, with shell scripts standing in for the compilers.
#![cfg(unix)]

use harness_shim::record::{CompileRecord, read_log};
use std::path::{Path, PathBuf};
use std::process::Command;

/// A compiler that writes its name and arguments to `calls` and a fixed object to its `-o` file.
/// `NOISE=1` makes it write a different object every time.
const FAKE: &str = r#"#!/bin/sh
echo "$(basename "$0") $*" >> "$(dirname "$0")/calls"
out=
while [ $# -gt 0 ]; do
	if [ "$1" = -o ]; then out=$2; shift; fi
	shift
done
if [ -n "$out" ]; then
	if [ -n "$NOISE" ]; then echo "$$ $(date +%s%N)" > "$out"; else echo object > "$out"; fi
fi
exit 0
"#;

struct Rig {
    root: PathBuf,
    bin: PathBuf,
    work: PathBuf,
}

impl Rig {
    fn new(tag: &str, extra: &str) -> Self {
        let root = std::env::temp_dir().join(format!("rb-shim-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let bin = root.join("bin");
        let work = root.join("work");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::create_dir_all(&work).unwrap();
        for name in ["rucc", "gcc15", "g++15", "cpp15", "as"] {
            let path = bin.join(name);
            std::fs::write(&path, FAKE).unwrap();
            Command::new("chmod").arg("+x").arg(&path).status().unwrap();
        }
        let shim = Path::new(env!("CARGO_BIN_EXE_rb-cc"));
        for name in [
            "x86_64-unknown-freebsd15.1-rb-cc-rucc",
            "x86_64-unknown-freebsd15.1-rb-cc-ref",
            "rb-c++",
            "rb-cpp",
            "rb-as",
        ] {
            std::fs::hard_link(shim, bin.join(name))
                .or_else(|_| std::fs::copy(shim, bin.join(name)).map(|_| ()))
                .unwrap();
        }
        let config = format!(
            "log = \"{log}\"\n\
             rucc = \"{b}/rucc\"\n\
             rucc-args = [\"--target=x86_64-unknown-freebsd15.1\", \"-fgnuc-version=15.2.0\"]\n\
             ref-cc = \"{b}/gcc15\"\n\
             ref-cxx = \"{b}/g++15\"\n\
             ref-cpp = \"{b}/cpp15\"\n\
             ref-as = \"{b}/as\"\n\
             temp-dirs = [\"/no/such/probe/dir\"]\n{extra}",
            log = root.join("compile.jsonl").display(),
            b = bin.display(),
        );
        std::fs::write(bin.join("rb-shim.toml"), config).unwrap();
        std::fs::write(work.join("foo.c"), "int x;\n").unwrap();
        std::fs::write(work.join("a.cc"), "int y;\n").unwrap();
        std::fs::write(work.join("in.x"), "z\n").unwrap();
        std::fs::write(work.join("b.s"), "nop\n").unwrap();
        Self { root, bin, work }
    }

    fn call(&self, name: &str, line: &str) -> i32 {
        Command::new(self.bin.join(name))
            .args(line.split_whitespace())
            .current_dir(&self.work)
            .env_remove("RB_SIDE")
            .env_remove("RB_BRINGUP")
            .env_remove("RB_TWICE")
            .status()
            .unwrap()
            .code()
            .unwrap_or(-1)
    }

    fn records(&self) -> Vec<CompileRecord> {
        read_log(&self.root.join("compile.jsonl")).unwrap().0
    }

    fn calls(&self) -> String {
        std::fs::read_to_string(self.bin.join("calls")).unwrap_or_default()
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[test]
fn every_name_runs_its_compiler_and_leaves_a_record() {
    let rig = Rig::new("names", "");
    assert_eq!(
        rig.call("x86_64-unknown-freebsd15.1-rb-cc-rucc", "-c foo.c -o foo.o"),
        0
    );
    assert_eq!(
        rig.call("x86_64-unknown-freebsd15.1-rb-cc-ref", "-c foo.c -o foo.o"),
        0
    );
    assert_eq!(rig.call("rb-c++", "-c a.cc -o a.o"), 0);
    assert_eq!(rig.call("rb-cpp", "-P in.x out.y"), 0);
    assert_eq!(rig.call("rb-as", "-o b.o b.s"), 0);
    assert_eq!(
        rig.call("x86_64-unknown-freebsd15.1-rb-cc-rucc", "--version"),
        0
    );

    let calls = rig.calls();
    assert!(
        calls.contains(
            "rucc --target=x86_64-unknown-freebsd15.1 -fgnuc-version=15.2.0 -c foo.c -o foo.o"
        ),
        "{calls}"
    );
    assert!(calls.contains("gcc15 -c foo.c -o foo.o"), "{calls}");
    assert!(calls.contains("g++15 -c a.cc -o a.o"), "{calls}");
    assert!(calls.contains("cpp15 -P in.x out.y"), "{calls}");
    assert!(calls.contains("as -o b.o b.s"), "{calls}");

    let records = rig.records();
    let seen: Vec<String> = records
        .iter()
        .map(|r| {
            format!(
                "{} {} {} {}",
                r.slot.as_deref().unwrap_or("-"),
                r.side.as_deref().unwrap_or("-"),
                r.origin.as_deref().unwrap_or("-"),
                if r.probe { "probe" } else { "call" },
            )
        })
        .collect();
    assert_eq!(
        seen,
        [
            "cc R rucc-c call",
            "cc G ref call",
            "c++ - ref-cxx call",
            "cpp - - call",
            "as - other call",
            "cc R - probe",
        ]
    );
    assert_eq!(records[0].added.len(), 2);
    assert_eq!(records[0].outputs[0].path, "foo.o");
    assert_eq!(records[0].outputs[0].sha256.len(), 64);
    assert_eq!(records[3].outputs[0].path, "out.y");
    assert!(records[0].argv[0].ends_with("-rb-cc-rucc"));
}

#[test]
fn a_shim_that_would_run_itself_refuses() {
    let rig = Rig::new("loop", "");
    let path = rig.bin.join("rb-shim.toml");
    let text = std::fs::read_to_string(&path).unwrap().replace(
        &format!("ref-as = \"{}/as\"", rig.bin.display()),
        &format!("ref-as = \"{}/rb-as\"", rig.bin.display()),
    );
    std::fs::write(&path, text).unwrap();
    assert_eq!(rig.call("rb-as", "-o b.o b.s"), 127);
}

#[test]
fn twice_finds_a_compiler_that_writes_different_bytes() {
    let rig = Rig::new("twice", "twice = true\n");
    assert_eq!(
        rig.call("x86_64-unknown-freebsd15.1-rb-cc-ref", "-c foo.c -o foo.o"),
        0
    );
    let status = Command::new(rig.bin.join("x86_64-unknown-freebsd15.1-rb-cc-ref"))
        .args(["-c", "foo.c", "-o", "foo.o"])
        .current_dir(&rig.work)
        .env("NOISE", "1")
        .status()
        .unwrap();
    assert_eq!(status.code(), Some(1));
    let records = rig.records();
    assert!(records[0].twice.as_ref().unwrap().identical);
    assert!(!records[1].twice.as_ref().unwrap().identical);
}

#[test]
fn bringup_as_sends_assembly_to_the_reference_and_says_so() {
    let rig = Rig::new("bringup", "bringup = [\"as\"]\n");
    std::fs::write(rig.work.join("locore.S"), "nop\n").unwrap();
    assert_eq!(
        rig.call(
            "x86_64-unknown-freebsd15.1-rb-cc-rucc",
            "-c locore.S -o locore.o"
        ),
        0
    );
    let records = rig.records();
    assert_eq!(records[0].delegated.as_deref(), Some("as"));
    assert_eq!(records[0].origin.as_deref(), Some("ref-c"));
    assert!(rig.calls().contains("gcc15 -c locore.S -o locore.o"));
}

/// A rucc that cannot make an object of `asm.c` but can write it as assembly.
const NO_OBJECT: &str = r#"#!/bin/sh
echo "$(basename "$0") $*" >> "$(dirname "$0")/calls"
case " $* " in
*" -c "*asm.c*) echo "asm.c:1: rucc cannot assemble this" >&2; exit 1 ;;
esac
out=
while [ $# -gt 0 ]; do
	if [ "$1" = -o ]; then out=$2; shift; fi
	shift
done
if [ -n "$out" ]; then echo assembly > "$out"; fi
exit 0
"#;

#[test]
fn bringup_as_assembles_what_rucc_cannot_with_the_reference() {
    let rig = Rig::new("bringup-c", "bringup = [\"as\"]\n");
    std::fs::write(rig.bin.join("rucc"), NO_OBJECT).unwrap();
    std::fs::write(rig.work.join("asm.c"), "int z;\n").unwrap();
    let cc = "x86_64-unknown-freebsd15.1-rb-cc-rucc";
    assert_eq!(rig.call(cc, "-O2 -m32 -c asm.c -o asm.o"), 0);
    assert_eq!(rig.call(cc, "-c foo.c -o foo.o"), 0);
    let calls = rig.calls();
    assert!(
        calls.contains(
            "rucc --target=x86_64-unknown-freebsd15.1 -fgnuc-version=15.2.0 -O2 -m32 -S asm.c -o asm.o.rb-as.s"
        ),
        "{calls}"
    );
    assert!(
        calls.contains("gcc15 -c asm.o.rb-as.s -o asm.o -m32"),
        "{calls}"
    );
    assert!(!rig.work.join("asm.o.rb-as.s").exists());
    let records = rig.records();
    assert_eq!(records.len(), 2);
    assert!(records[0].succeeded());
    assert_eq!(records[0].delegated.as_deref(), Some("as"));
    assert_eq!(records[0].origin.as_deref(), Some("rucc-c"));
    assert!(records[0].stderr.contains("cannot assemble"));
    assert_eq!(records[0].outputs[0].path, "asm.o");
    assert_eq!(records[1].delegated, None);

    let graded = Rig::new("graded-c", "");
    std::fs::write(graded.bin.join("rucc"), NO_OBJECT).unwrap();
    std::fs::write(graded.work.join("asm.c"), "int z;\n").unwrap();
    assert_eq!(graded.call(cc, "-c asm.c -o asm.o"), 1);
    assert!(!graded.calls().contains("gcc15"));
    assert_eq!(graded.records()[0].delegated, None);
}
