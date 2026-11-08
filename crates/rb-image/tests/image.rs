//! rb image with stand-in tools: a fake `makefs` and `mkimg` over a fake FreeBSD staging tree, and
//! a fake `nbmake-amd64` wrapper over a fake NetBSD release. The stand-ins log their arguments
//! and write their outputs, so the tests check what rb gives them and what it does with the
//! result. The tests marked `#[ignore]` use the real tools: see each one.
#![cfg(unix)]

use rb_image::conf::RbConf;
use rb_image::image::{self, Request, System};
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};

fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rb-image-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn write(path: &Path, text: &str, mode: u32) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// `makefs`: the image is the second last argument. It logs the directory it runs in.
const MAKEFS: &str = r#"#!/bin/sh
echo "makefs $* (in $(pwd))" >> "$(dirname "$0")/calls"
n=$(($# - 1))
eval img=\${$n}
echo fs > "$img"
"#;

/// `mkimg`: the image follows `-o`.
const MKIMG: &str = r#"#!/bin/sh
echo "mkimg $*" >> "$(dirname "$0")/calls"
while [ $# -gt 0 ]; do
	if [ "$1" = -o ]; then echo disk > "$2"; fi
	shift
done
"#;

struct FreeBsd {
    top: PathBuf,
    req: Request,
}

impl FreeBsd {
    fn new(tag: &str, metalog: bool) -> Self {
        let top = scratch(tag);
        let stage = top.join("stage");
        for f in [
            "boot/pmbr",
            "boot/gptboot",
            "boot/loader.efi",
            "bin/freebsd-version",
        ] {
            write(&stage.join(f), f, 0o644);
        }
        write(&stage.join("etc/rc.conf"), "keymap=\"us\"\n", 0o644);
        if metalog {
            write(
                &stage.join("METALOG"),
                "#mtree 2.0\n./etc type=dir mode=0755\n./etc/rc.conf type=file mode=0644 size=12\n",
                0o644,
            );
        }
        write(&top.join("tools/makefs"), MAKEFS, 0o755);
        write(&top.join("tools/mkimg"), MKIMG, 0o755);
        write(&top.join("pkg/All/pkg-2.0.pkg"), "pkg", 0o644);
        let req = Request {
            build: stage,
            out: top.join("fb.img"),
            system: System::Freebsd,
            conf: RbConf {
                suites: vec!["boot".into(), "smoke".into()],
                ..RbConf::default()
            },
            packages: Some(top.join("pkg")),
            size: Some(1 << 30),
            tools: Some(top.join("tools")),
            work: top.join("work"),
        };
        FreeBsd { top, req }
    }
}

#[test]
fn freebsd_tree() {
    let rig = FreeBsd::new("fb-tree", false);
    assert_eq!(System::detect(&rig.req.build).unwrap(), System::Freebsd);
    let made = image::make(&rig.req, false).unwrap();
    assert_eq!(read(&rig.top.join("fb.img")), "disk\n");
    assert_eq!(made.bytes, 5);
    assert!(!rig.top.join("fb.img.tmp").exists());

    let stage = &rig.req.build;
    let rc = read(&stage.join("etc/rc.conf"));
    assert!(rc.starts_with("keymap=\"us\"\n# rb image: begin\n"), "{rc}");
    assert!(rc.contains("rb_enable=\"YES\"\n"), "{rc}");
    assert!(read(&stage.join("etc/rb.conf")).contains("rb_suites='boot smoke'"));
    assert!(read(&stage.join("boot/loader.conf")).contains("console=\"comconsole\""));
    assert_eq!(read(&stage.join("boot.config")), "-S115200 -h\n");
    assert_eq!(read(&stage.join("var/db/rb-pkg/All/pkg-2.0.pkg")), "pkg");
    let run = std::fs::metadata(stage.join("usr/libexec/rb/run")).unwrap();
    assert_eq!(run.permissions().mode() & 0o777, 0o555);
    assert!(
        made.added
            .contains(&"var/db/rb-pkg/All/pkg-2.0.pkg".to_string())
    );
    assert_eq!(
        read(&rig.req.work.join("esp/EFI/BOOT/BOOTX64.EFI")),
        "boot/loader.efi"
    );
    assert!(rig.req.work.join("spare/rb-spare").is_file());
    assert!(!stage.join("METALOG.rb").exists());

    let calls = read(&rig.top.join("tools/calls"));
    let calls: Vec<&str> = calls.lines().collect();
    assert_eq!(calls.len(), 4, "{calls:?}");
    assert!(calls[2].contains(" -s 1073741824 "), "{}", calls[2]);
    assert!(calls[3].starts_with("mkimg -s gpt -f raw"), "{}", calls[3]);
    for name in ["esp", "spare", "root", "disk"] {
        assert!(rig.req.work.join(format!("logs/{name}.log")).is_file());
    }

    // A second run replaces rb's block and does not add one more.
    image::make(&rig.req, false).unwrap();
    let again = read(&stage.join("etc/rc.conf"));
    assert_eq!(again, rc);
    std::fs::remove_dir_all(&rig.top).unwrap();
}

#[test]
fn freebsd_metalog() {
    let rig = FreeBsd::new("fb-metalog", true);
    image::make(&rig.req, false).unwrap();
    let stage = &rig.req.build;
    let metalog = read(&stage.join("METALOG.rb"));
    assert!(
        metalog.starts_with("#mtree 2.0\n./etc type=dir mode=0755\n"),
        "{metalog}"
    );
    assert_eq!(metalog.matches("./etc/rc.conf ").count(), 1, "{metalog}");
    for line in [
        "./usr/libexec/rb type=dir uname=root gname=wheel mode=0755",
        "./var/db/rb-pkg/All type=dir uname=root gname=wheel mode=0755",
        "./etc/rc.d/rb type=file uname=root gname=wheel mode=0555 size=",
        "./var/db/rb-pkg/All/pkg-2.0.pkg type=file uname=root gname=wheel mode=0644 size=3",
    ] {
        assert!(metalog.contains(line), "no {line} in\n{metalog}");
    }
    let rc_size = std::fs::metadata(stage.join("etc/rc.conf")).unwrap().len();
    assert!(metalog.contains(&format!(
        "./etc/rc.conf type=file uname=root gname=wheel mode=0644 size={rc_size}\n"
    )));
    let calls = read(&rig.top.join("tools/calls"));
    let root = calls.lines().nth(2).unwrap();
    // On macOS the temporary directory is a link, and pwd prints where it points.
    let real = stage.canonicalize().unwrap();
    let in_stage = format!("(in {})", real.display());
    assert!(
        root.contains(" -D -N ") && root.contains(" METALOG.rb ") && root.ends_with(&in_stage),
        "{root}"
    );
    std::fs::remove_dir_all(&rig.top).unwrap();
}

#[test]
fn freebsd_needs_the_boot_files() {
    let rig = FreeBsd::new("fb-noboot", false);
    std::fs::remove_file(rig.req.build.join("boot/gptboot")).unwrap();
    let err = image::make(&rig.req, false).unwrap_err();
    assert!(err.contains("boot/gptboot"), "{err}");
    assert!(!rig.top.join("tools/calls").exists());
    std::fs::remove_dir_all(&rig.top).unwrap();
}

#[test]
fn freebsd_tool_failure() {
    let rig = FreeBsd::new("fb-fail", false);
    write(
        &rig.top.join("tools/mkimg"),
        "#!/bin/sh\necho mkimg: no space >&2\nexit 1\n",
        0o755,
    );
    let err = image::make(&rig.req, false).unwrap_err();
    assert!(
        err.contains("mkimg: no space") && err.contains("disk.log"),
        "{err}"
    );
    assert!(!rig.top.join("fb.img").exists());
    std::fs::remove_dir_all(&rig.top).unwrap();
}

/// `nbmake-amd64`: it logs its arguments, checks rb's files, and for `live-image` writes a gzip
/// image to `LIVEIMG_RELEASEDIR`.
const NBMAKE: &str = r#"#!/bin/sh
DESTDIR=@TOP@/destdir; export DESTDIR
MACHINE=amd64; export MACHINE
NETBSDSRCDIR=@TOP@/src; export NETBSDSRCDIR
RELEASEDIR=@TOP@/release; export RELEASEDIR
TOOLDIR=@TOP@/tools; export TOOLDIR
for a in "$@"; do echo "$a" >> "$TOOLDIR/calls"; done
echo -- >> "$TOOLDIR/calls"
for a in "$@"; do
	case $a in
	SPEC_EXTRA=*) cat ${a#SPEC_EXTRA=} > "$TOOLDIR/spec" ;;
	RC_CONF_EXTRA=*) cat ${a#RC_CONF_EXTRA=} > "$TOOLDIR/rc.conf" ;;
	LIVEIMG_RELEASEDIR=*)
		d=${a#LIVEIMG_RELEASEDIR=}
		mkdir -p "$d"
		printf 'netbsd disk' | gzip > "$d/NetBSD-11.0-amd64-live.img.gz" ;;
	esac
done
"#;

struct NetBsd {
    top: PathBuf,
    req: Request,
}

impl NetBsd {
    fn new(tag: &str) -> Self {
        let top = scratch(tag);
        let text = NBMAKE.replace("@TOP@", &top.display().to_string());
        write(&top.join("tools/bin/nbmake-amd64"), &text, 0o755);
        let emu = top.join("src/distrib/amd64/liveimage/emuimage");
        write(
            &emu.join("spec.emuimage"),
            "./etc/rc.d/ec2_init type=file\n",
            0o644,
        );
        write(&emu.join("rc.conf.emuimage"), "resize_root=YES\n", 0o644);
        write(
            &top.join("destdir/boot.cfg"),
            "consdev=pc\ntimeout=5\n",
            0o644,
        );
        let req = Request {
            build: top.join("tools"),
            out: top.join("nb.img"),
            system: System::Netbsd,
            conf: RbConf::default(),
            packages: None,
            size: Some(1 << 20),
            tools: None,
            work: top.join("work"),
        };
        NetBsd { top, req }
    }
}

#[test]
fn netbsd_live_image() {
    let rig = NetBsd::new("nb");
    assert_eq!(System::detect(&rig.req.build).unwrap(), System::Netbsd);
    let made = image::make(&rig.req, false).unwrap();
    let img = std::fs::read(rig.top.join("nb.img")).unwrap();
    assert_eq!(img.len(), 1 << 20);
    assert_eq!(made.bytes, 1 << 20);
    assert!(img.starts_with(b"netbsd disk") && img[11..].iter().all(|b| *b == 0));

    let tools = rig.top.join("tools");
    let calls = read(&tools.join("calls"));
    let runs: Vec<&str> = calls.split("--\n").collect();
    let live = format!("{}/src/distrib/amd64/liveimage", rig.top.display());
    assert_eq!(runs[0], format!("-C\n{live}\nobj\n"));
    assert_eq!(runs[1], format!("-C\n{live}\nclean\n"));
    let w = rig.req.work.display().to_string();
    assert!(
        runs[2].contains(&format!("IMGDIR_EXTRA={w}/overlay .\n")),
        "{}",
        runs[2]
    );
    assert!(runs[2].contains("INSTALLBOOTOPTIONS=-o console=com0,speed=115200\n"));

    let spec = read(&tools.join("spec"));
    assert!(
        spec.starts_with("./etc/rc.d/ec2_init type=file\n"),
        "{spec}"
    );
    assert!(spec.contains("./usr/libexec/rb type=dir uname=root gname=wheel mode=0755\n"));
    assert!(spec.contains("./etc/rc.d/rb type=file uname=root gname=wheel mode=0555\n"));
    assert!(
        !spec.contains("./boot.cfg"),
        "the etc set lists boot.cfg: {spec}"
    );
    let rc = read(&tools.join("rc.conf"));
    assert!(
        rc.starts_with("resize_root=YES\n") && rc.ends_with("rb_enable=\"YES\"\n"),
        "{rc}"
    );

    let overlay = rig.req.work.join("overlay");
    assert_eq!(read(&overlay.join("boot.cfg")), "consdev=com0\ntimeout=5\n");
    assert!(read(&overlay.join("etc/rb.conf")).contains("rb_suites=boot"));
    assert!(overlay.join("etc/rc.d/rb").is_file());
    assert!(made.added.contains(&"boot.cfg".to_string()));
    std::fs::remove_dir_all(&rig.top).unwrap();
}

#[test]
fn netbsd_wrapper_must_be_one() {
    let rig = NetBsd::new("nb-two");
    write(&rig.top.join("tools/bin/nbmake-i386"), "", 0o755);
    let err = image::make(&rig.req, false).unwrap_err();
    assert!(err.contains("more than one wrapper"), "{err}");
    let mut req = rig.req.clone();
    req.build = rig.top.join("tools/bin/nbmake-amd64");
    image::make(&req, true).unwrap();
    std::fs::remove_dir_all(&rig.top).unwrap();
}

/// The real FreeBSD tools. Set `RB_IMAGE_FREEBSD_STAGE` to a staging tree you can change, and put
/// `makefs` and `mkimg` on `PATH`, for example on a FreeBSD host.
#[test]
#[ignore = "needs a FreeBSD staging tree and makefs and mkimg"]
fn real_freebsd() {
    let stage =
        PathBuf::from(std::env::var("RB_IMAGE_FREEBSD_STAGE").expect("RB_IMAGE_FREEBSD_STAGE"));
    let top = scratch("real-fb");
    let req = Request {
        build: stage,
        out: top.join("fb.img"),
        system: System::Freebsd,
        conf: RbConf::default(),
        packages: None,
        size: None,
        tools: None,
        work: top.join("work"),
    };
    let made = image::make(&req, false).unwrap();
    assert!(made.bytes > 1 << 30, "{made:?}");
}

/// The real NetBSD `live-image` target. Set `RB_IMAGE_NETBSD_TOOLDIR` to the `TOOLDIR` of a
/// `build.sh release` run.
#[test]
#[ignore = "needs a NetBSD build.sh release"]
fn real_netbsd() {
    let tooldir =
        PathBuf::from(std::env::var("RB_IMAGE_NETBSD_TOOLDIR").expect("RB_IMAGE_NETBSD_TOOLDIR"));
    let top = scratch("real-nb");
    let req = Request {
        build: tooldir,
        out: top.join("nb.img"),
        system: System::Netbsd,
        conf: RbConf::default(),
        packages: None,
        size: Some(4 << 30),
        tools: None,
        work: top.join("work"),
    };
    let made = image::make(&req, false).unwrap();
    assert_eq!(made.bytes, 4 << 30);
}
