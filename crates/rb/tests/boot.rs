//! rb boot with a stand-in for QEMU. The stand-in is a shell script that plays a guest: it reads
//! the rb.conf from the results disk as rb-rc does, prints the console markers, and writes its
//! results back to the disk. Each test gives it a different ending. `real_boot` uses QEMU itself.
#![cfg(unix)]

use rb::boot::{self, Firmware, Request};
use rb_image::conf::RbConf;
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::time::Instant;

/// The part every stand-in starts with: find the results disk among the arguments, and read the
/// rb.conf on it.
const HEAD: &str = r#"#!/bin/sh
here=$(cd "$(dirname "$0")" && pwd)
echo "$*" > "$here/args"
for a in "$@"; do
	case $a in
	*id=rb,file=*) disk=${a#*id=rb,file=}; disk=${disk%%,*} ;;
	esac
done
mkdir -p "$here/in" && (cd "$here/in" && tar -xf "$disk")
. "$here/in/rb.conf"
echo "Copyright (c) 1992-2026 The FreeBSD Project."
"#;

/// A guest that runs its suites, saves to the disk and powers off.
const RUN: &str = r#"
echo "RB-READY FreeBSD rb 16.0-CURRENT amd64"
echo "RB-CONF /dev/vtbd1"
mkdir -p "$here/out"
for s in $rb_suites; do
	echo "RB-SUITE $s"
	echo "RB-CHECK pass shells"
	echo "$s pass" >> "$here/out/suites"
	echo "RB-SUITE-END $s pass"
done
uname > "$here/out/uname.txt"
tar -cf "$disk" -C "$here/out" .
echo "RB-DONE boot=pass archive=disk"
exit 0
"#;

struct Rig {
    top: PathBuf,
    req: Request,
}

impl Rig {
    fn new(tag: &str, body: &str) -> Self {
        let top = std::env::temp_dir().join(format!("rb-boot-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&top);
        std::fs::create_dir_all(top.join("bin")).unwrap();
        let qemu = top.join("bin/qemu-system-x86_64");
        std::fs::write(&qemu, format!("{HEAD}{body}")).unwrap();
        std::fs::set_permissions(&qemu, std::fs::Permissions::from_mode(0o755)).unwrap();
        let img = top.join("bin/qemu-img");
        std::fs::write(
            &img,
            "#!/bin/sh\neval last=\\${$#}\necho overlay > \"$last\"\n",
        )
        .unwrap();
        std::fs::set_permissions(&img, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::write(top.join("fb.img"), "the image").unwrap();
        let req = Request {
            image: top.join("fb.img"),
            out: top.join("run"),
            conf: RbConf {
                suites: vec!["boot".into(), "smoke".into()],
                ..RbConf::default()
            },
            qemu: qemu.display().to_string(),
            machine: "q35".into(),
            kvm: false,
            cpus: 2,
            memory: "1G".into(),
            firmware: Firmware::Seabios,
            boot_timeout: 20,
            timeout: 60,
            poweroff_timeout: 20,
            ddb_wait: 1,
            disk_size: 1 << 20,
            spare_disks: 0,
            reboots: 1,
            in_place: false,
            keep: false,
        };
        Rig { top, req }
    }

    fn file(&self, rel: &str) -> String {
        std::fs::read_to_string(self.top.join(rel)).unwrap_or_default()
    }

    fn done(self) {
        std::fs::remove_dir_all(&self.top).unwrap();
    }
}

#[test]
fn a_boot_that_passes() {
    let rig = Rig::new("pass", RUN);
    let o = boot::run(&rig.req).unwrap();
    assert_eq!(o.result, "pass", "{o:?}");
    assert!(o.passed());
    assert_eq!(o.qemu_exit, Some(0));
    assert_eq!(o.results.as_deref(), Some("disk"));
    assert_eq!(rig.file("run/results/suites"), "boot pass\nsmoke pass\n");
    assert!(o.seen.conf_from_disk);
    assert_eq!(o.seen.checks.len(), 2);
    assert!(o.ready_seconds.is_some() && o.done_seconds.is_some());
    // The guest saw the suites rb boot asked for, on the overlay.
    assert!(
        rig.file("bin/in/rb.conf")
            .contains("rb_suites='boot smoke'")
    );
    assert!(rig.file("bin/args").contains("file="));
    assert!(
        rig.file("bin/args")
            .contains("/run/disk.qcow2,format=qcow2")
    );
    assert_eq!(rig.file("fb.img"), "the image");
    // The disks are gone, and the logs and the outcome are there.
    assert!(!rig.top.join("run/disk.qcow2").exists());
    assert!(!rig.top.join("run/results.disk").exists());
    assert!(rig.file("run/console.log").contains("RB-DONE boot=pass"));
    let json: serde_json::Value = serde_json::from_str(&rig.file("run/boot.json")).unwrap();
    assert_eq!(json["result"], "pass");
    assert_eq!(json["seen"]["suites"][1]["name"], "smoke");
    let text = boot::summary(&o, &rig.req.out);
    assert!(text.starts_with("# rb boot: pass\n"), "{text}");
    rig.done();
}

#[test]
fn a_reboot_before_rb_rc_starts_it_again() {
    let body = format!(
        "if [ ! -f \"$here/booted\" ]; then touch \"$here/booted\"; echo resizing; exit 0; fi\n{RUN}"
    );
    let mut rig = Rig::new("reboot", &body);
    rig.req.in_place = true;
    rig.req.keep = true;
    let o = boot::run(&rig.req).unwrap();
    assert_eq!((o.result.as_str(), o.restarts), ("pass", 1), "{o:?}");
    assert!(rig.file("bin/args").contains("/fb.img,format=raw"));
    assert!(rig.top.join("run/results.disk").exists());
    assert!(rig.file("run/qemu.log").contains("starting it again"));

    // With no restarts left, the same guest is a boot that never got to rb-rc.
    let _ = std::fs::remove_file(rig.top.join("bin/booted"));
    rig.req.reboots = 0;
    let o = boot::run(&rig.req).unwrap();
    assert_eq!(o.result, "no-ready");
    assert!(!o.passed());
    rig.done();
}

#[test]
fn a_panic_in_ddb() {
    let body = r#"
echo "RB-READY NetBSD rb 11.99 amd64"
echo "RB-SUITE tests"
echo "RB-CASE lib/libc/t_x:y"
echo "fatal page fault in supervisor mode"
echo "panic: trap"
printf 'db{0}> '
dd bs=1 count=3 2>/dev/null | tr '\r' '\n' > "$here/ddb"
echo "trap() at netbsd:trap+0x100"
printf 'db{0}> '
exec sleep 30
"#;
    let rig = Rig::new("panic", body);
    let clock = Instant::now();
    let o = boot::run(&rig.req).unwrap();
    assert!(clock.elapsed().as_secs() < 15);
    assert_eq!(o.result, "panic", "{o:?}");
    assert_eq!(o.seen.panic.as_deref(), Some("panic: trap"));
    assert_eq!(o.seen.panic_case.as_deref(), Some("lib/libc/t_x:y"));
    assert_eq!(o.seen.panic_suite.as_deref(), Some("tests"));
    assert_eq!(rig.file("bin/ddb").trim(), "bt");
    assert!(
        rig.file("run/console.log")
            .contains("trap() at netbsd:trap+0x100")
    );
    assert_eq!(o.results, None);
    let text = boot::summary(&o, &rig.req.out);
    assert!(
        text.contains("| running at the panic | lib/libc/t_x:y |"),
        "{text}"
    );
    rig.done();
}

#[test]
fn limits() {
    let mut rig = Rig::new("limits", "exec sleep 30\n");
    rig.req.boot_timeout = 1;
    let clock = Instant::now();
    let o = boot::run(&rig.req).unwrap();
    assert_eq!(o.result, "boot-timeout");
    assert!(clock.elapsed().as_secs() < 10);
    rig.done();

    let body = "echo 'RB-READY FreeBSD'\necho 'RB-SUITE tests'\nexec sleep 30\n";
    let mut rig = Rig::new("limits-run", body);
    rig.req.timeout = 2;
    let o = boot::run(&rig.req).unwrap();
    assert_eq!(o.result, "timeout");
    assert_eq!(o.seen.running(), Some("tests"));
    rig.done();

    let body = "echo 'RB-READY FreeBSD'\necho 'RB-DONE boot=pass archive=disk'\nexec sleep 30\n";
    let mut rig = Rig::new("limits-off", body);
    rig.req.poweroff_timeout = 1;
    let o = boot::run(&rig.req).unwrap();
    assert_eq!(o.result, "no-poweroff");
    rig.done();
}

#[test]
fn an_image_with_no_rb_rc() {
    let body = "printf '\\nFreeBSD/amd64 (freebsd) (ttyu0)\\n\\nlogin: '\nexec sleep 30\n";
    let mut rig = Rig::new("login", body);
    rig.req.conf.suites = vec!["boot".into()];
    let o = boot::run(&rig.req).unwrap();
    assert_eq!(o.result, "login");
    assert!(o.passed() && o.seen.login && o.seen.ready.is_none());
    rig.req.conf.suites = vec!["boot".into(), "smoke".into()];
    assert!(!boot::run(&rig.req).unwrap().passed());
    rig.done();
}

#[test]
fn results_on_the_console() {
    let body = r#"
echo "RB-READY FreeBSD"
echo "RB-NODISK /dev/vtbd1"
mkdir -p "$here/out" && echo "boot pass" > "$here/out/suites"
tar -cf "$here/rb.tar" -C "$here/out" .
echo "begin-base64 644 rb.tar"
base64 < "$here/rb.tar"
echo "===="
echo "RB-DONE boot=pass archive=console"
"#;
    let rig = Rig::new("console", body);
    let o = boot::run(&rig.req).unwrap();
    assert_eq!(o.result, "pass", "{o:?}");
    assert!(o.seen.no_disk);
    assert_eq!(o.results.as_deref(), Some("console"));
    assert_eq!(rig.file("run/results/suites"), "boot pass\n");
    rig.done();
}

/// A real boot. Set `RB_BOOT_IMAGE` to an image from `rb image`, with qemu-system-x86_64 and
/// qemu-img on `PATH`. It boots with `SeaBIOS` and runs the boot suite.
#[test]
#[ignore = "needs QEMU and an image"]
fn real_boot() {
    let image = PathBuf::from(std::env::var("RB_BOOT_IMAGE").expect("RB_BOOT_IMAGE"));
    let out = std::env::temp_dir().join(format!("rb-boot-real-{}", std::process::id()));
    let req = Request {
        image,
        out: out.clone(),
        conf: RbConf::default(),
        qemu: "qemu-system-x86_64".into(),
        machine: "q35".into(),
        kvm: boot::kvm_usable("qemu-system-x86_64"),
        cpus: 2,
        memory: "2G".into(),
        firmware: Firmware::Seabios,
        boot_timeout: 1800,
        timeout: 3600,
        poweroff_timeout: 300,
        ddb_wait: 30,
        disk_size: 64 << 20,
        spare_disks: 0,
        reboots: 1,
        in_place: false,
        keep: false,
    };
    let o = boot::run(&req).unwrap();
    println!("{}", boot::summary(&o, Path::new(&out)));
    assert!(o.passed(), "{o:?}");
}
