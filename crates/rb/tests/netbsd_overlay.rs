//! The R TOOLDIR of scripts/netbsd/overlay.sh must keep its shims for the whole R build.
//!
//! tools/Makefile.gnuhost configures a GNU tool again when its `.configure_done` names another
//! host, and the install of that configure puts GCC over the shim of the R TOOLDIR. overlay.sh
//! therefore runs "build.sh -u tools" in the G TOOLDIR first when the tools are stale, and
//! scripts/netbsd/phase.sh fails a build after which the C compiler slot is no longer the shim.
#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const GNU: &str = "x86_64--netbsd";

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("rb-netbsd-overlay-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn executable(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
    Command::new("chmod").arg("+x").arg(path).status().unwrap();
}

/// The "uname -srm" of the host as tools/Makefile.gnuhost writes it in `.configure_done`.
fn platform() -> String {
    let o = Command::new("uname").arg("-srm").output().unwrap();
    String::from_utf8(o.stdout)
        .unwrap()
        .trim()
        .chars()
        .map(|c| if matches!(c, ' ' | '(' | ')') { '-' } else { c })
        .collect()
}

/// A root with a G TOOLDIR, a GNU tool configured on CONFIGURED, a build.sh that writes its
/// arguments to build.sh.args, and a rucc on the PATH.
fn root(tag: &str, configured: &str) -> PathBuf {
    let d = dir(tag);
    for name in ["gcc", "cpp"] {
        executable(&d.join(format!("tools/bin/{GNU}-{name}")), "\x7fELF");
    }
    executable(&d.join(format!("tools/{GNU}/bin/ld")), "\x7fELF");
    std::fs::create_dir_all(d.join("obj/tools/gcc")).unwrap();
    std::fs::write(
        d.join("obj/tools/gcc/.configure_done"),
        format!("{configured}\n"),
    )
    .unwrap();
    executable(
        &d.join("src/build.sh"),
        &format!(
            "#!/bin/sh\necho \"$@\" >> {}\n",
            d.join("build.sh.args").display()
        ),
    );
    executable(&d.join("bin/rucc"), "#!/bin/sh\necho rucc 0\n");
    d
}

fn run(d: &Path, program: &str, args: &[&str], tooldir: Option<&Path>) -> Output {
    let mut c = Command::new("bash");
    c.arg(repo().join(program)).args(args);
    c.env("NB_ROOT", d)
        .env("NB_JOBS", "1")
        .env_remove("NB_TOOLDIR")
        .env_remove("NB_VARS");
    c.env(
        "PATH",
        format!(
            "{}:{}",
            d.join("bin").display(),
            std::env::var("PATH").unwrap()
        ),
    );
    if let Some(t) = tooldir {
        c.env("NB_TOOLDIR", t);
    }
    c.output().unwrap()
}

fn overlay(d: &Path) -> Option<String> {
    let o = run(d, "scripts/netbsd/overlay.sh", &["rucc"], None);
    assert!(
        o.status.success(),
        "{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    );
    std::fs::read_to_string(d.join("build.sh.args")).ok()
}

#[test]
fn tools_from_another_host_are_made_current_before_the_copy() {
    let d = root("stale", "Linux-6.0.0-other-x86_64");
    let args = overlay(&d).expect("overlay.sh did not run build.sh");
    let tools = d.join("tools");
    assert!(args.contains(&format!("-T {} ", tools.display())), "{args}");
    assert!(args.trim_end().ends_with("-u tools"), "{args}");
    let shim = std::fs::read_to_string(d.join(format!("tools-r/bin/{GNU}-gcc"))).unwrap();
    assert!(shim.starts_with("#!"), "the R TOOLDIR has no shim");
}

#[test]
fn tools_of_this_host_are_copied_as_they_are() {
    let d = root("fresh", &platform());
    assert_eq!(
        overlay(&d),
        None,
        "overlay.sh ran build.sh for current tools"
    );
}

#[test]
fn a_phase_that_puts_gcc_over_the_shim_fails() {
    let d = root("phase", &platform());
    assert_eq!(overlay(&d), None);
    let r = d.join("tools-r");
    let o = run(
        &d,
        "scripts/netbsd/phase.sh",
        &["ok", "--", "true"],
        Some(&r),
    );
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let gcc = r.join(format!("bin/{GNU}-gcc"));
    std::fs::copy(r.join(format!("bin/{GNU}-gcc.ref")), &gcc).unwrap();
    let o = run(
        &d,
        "scripts/netbsd/phase.sh",
        &["gcc", "--", "true"],
        Some(&r),
    );
    assert_eq!(o.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&o.stderr).contains("is no longer the shim of overlay.sh"));
    let o = run(&d, "scripts/netbsd/phase.sh", &["g", "--", "true"], None);
    assert!(
        o.status.success(),
        "a phase in the G TOOLDIR is not an R build"
    );
}
