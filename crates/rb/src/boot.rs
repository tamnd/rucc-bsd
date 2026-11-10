//! `rb boot`: one boot of a disk image under QEMU (plan 12.3).
//!
//! 1. The image is not written. The guest runs on a qcow2 overlay of it, so each boot starts from
//!    the same disk, unless `--in-place` is given.
//! 2. The second disk is the results disk. Before the boot it holds a tar archive with an
//!    `rb.conf` that names the suites. rb-rc reads it, runs the suites and writes its results to
//!    the same disk, as a tar archive with no file system (12.4).
//! 3. The serial console goes to `console.log`, and [`crate::console`] reads it while the guest
//!    runs. There are two limits: the boot, until `RB-READY` or a login prompt, and the whole run.
//!    When a limit passes, QEMU is killed.
//! 4. A panic stops in the kernel debugger. rb sends `bt` once, waits for the backtrace and kills
//!    QEMU. The run is a `panic`, with the test case that was running.
//! 5. A guest that reboots before rb-rc starts is started again, up to `--reboots` times. A NetBSD
//!    image grows its root and reboots at its first boot.
//! 6. The results are unpacked to `results/`, and `boot.json` says what happened.
//!
//! An image with no rb-rc, such as a release image, shows a login prompt. Then the result is
//! `login`, which passes the boot suite and nothing else.

use crate::console::{Lines, Seen};
use rb_image::conf::RbConf;
use rb_image::ustar;
use serde::{Deserialize, Serialize};
use std::fmt::Write as _;
use std::io::{Read as _, Write as _};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// The firmware of a boot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Firmware {
    /// QEMU's own `SeaBIOS`, through the BIOS boot blocks.
    Seabios,
    /// OVMF, through the EFI loader: the code, and the variables that each boot gets a copy of.
    Ovmf {
        /// `OVMF_CODE`.
        code: PathBuf,
        /// `OVMF_VARS`.
        vars: PathBuf,
    },
    /// EDK2 for aarch64, through the EFI loader, in the same two flash devices as OVMF.
    Edk2 {
        /// The code, such as `AAVMF_CODE.fd`.
        code: PathBuf,
        /// The variables, such as `AAVMF_VARS.fd`.
        vars: PathBuf,
    },
    /// QEMU's own `OpenSBI`, then U-Boot in S-mode as its next stage. U-Boot loads the EFI loader
    /// from the disk.
    UBoot {
        /// U-Boot's `u-boot.bin`.
        image: PathBuf,
    },
}

impl Firmware {
    /// The name, as in `rows.toml`.
    #[must_use]
    pub fn name(&self) -> &'static str {
        match self {
            Firmware::Seabios => "seabios",
            Firmware::Ovmf { .. } => "ovmf",
            Firmware::Edk2 { .. } => "edk2",
            Firmware::UBoot { .. } => "u-boot",
        }
    }

    /// The code and the variables of a firmware in flash.
    #[must_use]
    pub fn flash(&self) -> Option<(&Path, &Path)> {
        match self {
            Firmware::Ovmf { code, vars } | Firmware::Edk2 { code, vars } => Some((code, vars)),
            Firmware::Seabios | Firmware::UBoot { .. } => None,
        }
    }
}

/// Where Linux distributions, FreeBSD's `edk2-qemu-x64` and Homebrew put OVMF, as code and
/// variables.
pub const OVMF_PLACES: &[(&str, &str)] = &[
    (
        "/usr/share/OVMF/OVMF_CODE_4M.fd",
        "/usr/share/OVMF/OVMF_VARS_4M.fd",
    ),
    (
        "/usr/share/OVMF/OVMF_CODE.fd",
        "/usr/share/OVMF/OVMF_VARS.fd",
    ),
    (
        "/usr/share/edk2/x64/OVMF_CODE.4m.fd",
        "/usr/share/edk2/x64/OVMF_VARS.4m.fd",
    ),
    (
        "/usr/share/edk2/ovmf/OVMF_CODE.fd",
        "/usr/share/edk2/ovmf/OVMF_VARS.fd",
    ),
    (
        "/usr/local/share/edk2-qemu/QEMU_UEFI_CODE-x86_64.fd",
        "/usr/local/share/edk2-qemu/QEMU_UEFI_VARS-x86_64.fd",
    ),
    (
        "/opt/homebrew/share/qemu/edk2-x86_64-code.fd",
        "/opt/homebrew/share/qemu/edk2-i386-vars.fd",
    ),
];

/// Where Debian and Ubuntu (`qemu-efi-aarch64`), Fedora, Arch, FreeBSD's `qemu` and Homebrew put
/// EDK2 for aarch64, as code and variables.
pub const EDK2_PLACES: &[(&str, &str)] = &[
    (
        "/usr/share/AAVMF/AAVMF_CODE.fd",
        "/usr/share/AAVMF/AAVMF_VARS.fd",
    ),
    (
        "/usr/share/edk2/aarch64/QEMU_EFI-pflash.raw",
        "/usr/share/edk2/aarch64/vars-template-pflash.raw",
    ),
    (
        "/usr/share/edk2/aarch64/QEMU_CODE.fd",
        "/usr/share/edk2/aarch64/QEMU_VARS.fd",
    ),
    (
        "/usr/local/share/qemu/edk2-aarch64-code.fd",
        "/usr/local/share/qemu/edk2-arm-vars.fd",
    ),
    (
        "/opt/homebrew/share/qemu/edk2-aarch64-code.fd",
        "/opt/homebrew/share/qemu/edk2-arm-vars.fd",
    ),
];

/// Where Debian and Ubuntu (`u-boot-qemu`) and FreeBSD (`u-boot-qemu-riscv64`) put the S-mode
/// U-Boot for QEMU's riscv64 `virt` machine.
pub const U_BOOT_PLACES: &[&str] = &[
    "/usr/lib/u-boot/qemu-riscv64_smode/u-boot.bin",
    "/usr/local/share/u-boot/u-boot-qemu-riscv64/u-boot.bin",
];

/// The first code and variables of `places` that are there.
fn find_flash(places: &[(&str, &str)]) -> Option<(PathBuf, PathBuf)> {
    places
        .iter()
        .find(|(c, v)| Path::new(c).is_file() && Path::new(v).is_file())
        .map(|(c, v)| (PathBuf::from(c), PathBuf::from(v)))
}

/// The first OVMF of [`OVMF_PLACES`] that is there.
#[must_use]
pub fn find_ovmf() -> Option<(PathBuf, PathBuf)> {
    find_flash(OVMF_PLACES)
}

/// The first EDK2 of [`EDK2_PLACES`] that is there.
#[must_use]
pub fn find_edk2() -> Option<(PathBuf, PathBuf)> {
    find_flash(EDK2_PLACES)
}

/// The first U-Boot of [`U_BOOT_PLACES`] that is there.
#[must_use]
pub fn find_u_boot() -> Option<PathBuf> {
    U_BOOT_PLACES
        .iter()
        .map(PathBuf::from)
        .find(|p| p.is_file())
}

/// The bus of the virtio devices.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bus {
    /// PCI: `virtio-blk-pci` and `virtio-net-pci`.
    Pci,
    /// virtio-mmio: `virtio-blk-device` and `virtio-net-device`.
    Mmio,
}

/// One boot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    /// The disk image.
    pub image: PathBuf,
    /// Where the console, `boot.json` and the results go.
    pub out: PathBuf,
    /// The `rb.conf` on the results disk.
    pub conf: RbConf,
    /// The QEMU program.
    pub qemu: String,
    /// The QEMU machine.
    pub machine: String,
    /// Whether to use KVM.
    pub kvm: bool,
    /// The QEMU CPU under TCG. Under KVM the CPU is always `host`.
    pub cpu: Option<String>,
    /// The bus of the virtio disks and network.
    pub virtio: Bus,
    /// vCPUs.
    pub cpus: u32,
    /// Memory, as QEMU takes it, such as `8G`.
    pub memory: String,
    /// The firmware.
    pub firmware: Firmware,
    /// Seconds until `RB-READY` or a login prompt.
    pub boot_timeout: u64,
    /// Seconds for the whole run.
    pub timeout: u64,
    /// Seconds from `RB-DONE` to the power off.
    pub poweroff_timeout: u64,
    /// Seconds at the kernel debugger's prompt, for the backtrace.
    pub ddb_wait: u64,
    /// The size of the results disk.
    pub disk_size: u64,
    /// Empty 128 MiB disks after the results disk, for tests that take a disk.
    pub spare_disks: u32,
    /// How many times a guest that reboots before rb-rc starts is started again.
    pub reboots: u32,
    /// Boot the image itself, not an overlay.
    pub in_place: bool,
    /// Keep the overlay and the results disk.
    pub keep: bool,
}

/// How a boot ended.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Outcome {
    /// `pass`, `login`, `panic`, `boot-timeout`, `timeout`, `no-poweroff`, `no-done` or
    /// `no-ready`.
    pub result: String,
    /// The image.
    pub image: String,
    /// `seabios`, `ovmf`, `edk2` or `u-boot`.
    pub firmware: String,
    /// `kvm` or `tcg`.
    pub accel: String,
    /// The suites asked for.
    pub asked: Vec<String>,
    /// The QEMU command of the first start.
    pub command: Vec<String>,
    /// How many times the guest was started again after a reboot.
    pub restarts: u32,
    /// Seconds until `RB-READY` or the login prompt.
    pub ready_seconds: Option<f64>,
    /// Seconds until `RB-DONE`.
    pub done_seconds: Option<f64>,
    /// Seconds in all.
    pub seconds: f64,
    /// QEMU's last exit status, if it exited by itself.
    pub qemu_exit: Option<i32>,
    /// Where the results came from: `disk` or `console`. None if there were none.
    pub results: Option<String>,
    /// What the console showed.
    pub seen: Seen,
}

impl Outcome {
    /// Whether the boot passed. A login prompt passes when only the boot suite was asked for.
    #[must_use]
    pub fn passed(&self) -> bool {
        self.result == "pass" || (self.result == "login" && self.asked == ["boot"])
    }
}

/// The paths of one boot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paths {
    /// The disk the guest boots: the overlay, or the image.
    pub disk: PathBuf,
    /// Its QEMU format.
    pub format: &'static str,
    /// The results disk.
    pub results_disk: PathBuf,
    /// The copy of the firmware's variables.
    pub vars: PathBuf,
}

/// The paths in `out`.
#[must_use]
pub fn paths(req: &Request) -> Paths {
    let qcow2 = |p: &Path| p.extension().is_some_and(|e| e == "qcow2");
    let (disk, format) = if req.in_place {
        (
            req.image.clone(),
            if qcow2(&req.image) { "qcow2" } else { "raw" },
        )
    } else {
        (req.out.join("disk.qcow2"), "qcow2")
    };
    Paths {
        disk,
        format,
        results_disk: req.out.join("results.disk"),
        vars: req.out.join(format!("{}-vars.fd", req.firmware.name())),
    }
}

/// `qemu-img`, next to the QEMU program when that is a path, else from `PATH`.
#[must_use]
pub fn qemu_img(req: &Request) -> String {
    match Path::new(&req.qemu).parent() {
        Some(dir) if !dir.as_os_str().is_empty() => dir.join("qemu-img").display().to_string(),
        _ => "qemu-img".to_string(),
    }
}

/// The command that makes the overlay.
#[must_use]
pub fn overlay_command(req: &Request) -> Vec<String> {
    let p = paths(req);
    let image = std::path::absolute(&req.image).unwrap_or_else(|_| req.image.clone());
    let backing = if image.extension().is_some_and(|e| e == "qcow2") {
        "qcow2"
    } else {
        "raw"
    };
    [
        qemu_img(req).as_str(),
        "create",
        "-q",
        "-f",
        "qcow2",
        "-F",
        backing,
        "-b",
        &image.display().to_string(),
        &p.disk.display().to_string(),
    ]
    .iter()
    .map(|s| (*s).to_string())
    .collect()
}

/// The QEMU command.
#[must_use]
pub fn qemu_command(req: &Request) -> Vec<String> {
    let p = paths(req);
    let accel = if req.kvm { "kvm" } else { "tcg" };
    let mut words: Vec<String> = vec![
        req.qemu.clone(),
        "-machine".into(),
        format!("{},accel={accel}", req.machine),
    ];
    if req.kvm {
        words.extend(["-cpu".into(), "host".into()]);
    } else if let Some(cpu) = &req.cpu {
        words.extend(["-cpu".into(), cpu.clone()]);
    }
    words.extend([
        "-smp".into(),
        req.cpus.to_string(),
        "-m".into(),
        req.memory.clone(),
        "-display".into(),
        "none".into(),
        "-monitor".into(),
        "none".into(),
        "-serial".into(),
        "stdio".into(),
        "-no-reboot".into(),
    ]);
    if let Some((code, _)) = req.firmware.flash() {
        words.extend([
            "-drive".into(),
            format!("if=pflash,format=raw,readonly=on,file={}", code.display()),
            "-drive".into(),
            format!("if=pflash,format=raw,file={}", p.vars.display()),
        ]);
    }
    if let Firmware::UBoot { image } = &req.firmware {
        words.extend(["-kernel".into(), image.display().to_string()]);
    }
    let mut disks = vec![
        ("root".to_string(), p.disk.clone(), p.format),
        ("rb".to_string(), p.results_disk.clone(), "raw"),
    ];
    for i in 1..=req.spare_disks {
        disks.push((
            format!("spare{i}"),
            req.out.join(format!("spare{i}.disk")),
            "raw",
        ));
    }
    // The virt machine gives the first virtio-mmio device on the command line the highest
    // address, and the guests number the devices from the lowest. In reverse, the root disk is
    // the first disk, vtbd0 or ld0, and the results disk the second, as on PCI.
    let mmio = req.virtio == Bus::Mmio;
    if mmio {
        disks.reverse();
    }
    let bus = if mmio { "device" } else { "pci" };
    for (id, file, format) in &disks {
        words.extend([
            "-drive".into(),
            format!(
                "if=none,id={id},file={},format={format},cache=unsafe",
                file.display()
            ),
            "-device".into(),
            if id == "root" && !mmio {
                format!("virtio-blk-pci,drive={id},bootindex=0")
            } else {
                format!("virtio-blk-{bus},drive={id}")
            },
        ]);
    }
    // QEMU's user network, with DHCP for the smoke suite's check and no route out.
    words.extend([
        "-netdev".into(),
        "user,id=n0,restrict=on".into(),
        "-device".into(),
        format!("virtio-net-{bus},netdev=n0"),
    ]);
    words
}

/// Whether KVM can run a guest of this QEMU here.
#[must_use]
pub fn kvm_usable(qemu: &str) -> bool {
    let native = std::env::consts::ARCH == "x86_64"
        && (qemu.ends_with("qemu-system-x86_64") || qemu.ends_with("qemu-system-i386"));
    native
        && std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open("/dev/kvm")
            .is_ok()
}

/// The results disk before the boot: an archive with the `rb.conf`, and room after it.
pub fn results_disk(conf: &RbConf, size: u64) -> Result<Vec<u8>, String> {
    let tar = ustar::write(&[ustar::Entry::file("rb.conf", 0o644, conf.render())])?;
    if (tar.len() as u64) > size {
        return Err(format!("a results disk of {size} bytes is too small"));
    }
    Ok(tar)
}

fn create_file(path: &Path, data: &[u8], size: u64) -> Result<(), String> {
    let err = |e: std::io::Error| format!("{}: {e}", path.display());
    let mut f = std::fs::File::create(path).map_err(err)?;
    f.write_all(data).map_err(err)?;
    f.set_len(size).map_err(err)
}

/// Make the overlay, the results disk, the spare disks and the copy of the firmware's variables.
pub fn prepare(req: &Request) -> Result<(), String> {
    std::fs::create_dir_all(&req.out).map_err(|e| format!("{}: {e}", req.out.display()))?;
    if !req.image.is_file() {
        return Err(format!("no image at {}", req.image.display()));
    }
    let p = paths(req);
    let _ = std::fs::remove_dir_all(req.out.join("results"));
    if !req.in_place {
        let _ = std::fs::remove_file(&p.disk);
        let argv = overlay_command(req);
        let out = Command::new(&argv[0])
            .args(&argv[1..])
            .output()
            .map_err(|e| {
                format!(
                    "{}: {e}; install qemu-utils, or boot the image itself with --in-place",
                    argv[0]
                )
            })?;
        if !out.status.success() {
            return Err(format!(
                "{} failed: {}",
                argv.join(" "),
                String::from_utf8_lossy(&out.stderr).trim()
            ));
        }
    }
    create_file(
        &p.results_disk,
        &results_disk(&req.conf, req.disk_size)?,
        req.disk_size,
    )?;
    for i in 1..=req.spare_disks {
        create_file(&req.out.join(format!("spare{i}.disk")), &[], 128 << 20)?;
    }
    if let Some((_, vars)) = req.firmware.flash() {
        std::fs::copy(vars, &p.vars).map_err(|e| format!("{}: {e}", vars.display()))?;
    }
    Ok(())
}

/// What the reader threads send.
enum Msg {
    Out(Vec<u8>),
    Err(String),
    End,
}

/// Why rb stopped QEMU.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stop {
    BootTimeout,
    Timeout,
    NoPoweroff,
    Ddb,
    Login,
}

/// The console as it comes, over every start of QEMU in one boot.
struct Watch {
    console: std::fs::File,
    qemu_log: std::fs::File,
    lines: Lines,
    seen: Seen,
    clock: Instant,
    ready_at: Option<Instant>,
    done_at: Option<Instant>,
    ddb_at: Option<Instant>,
    stop: Option<Stop>,
}

impl Watch {
    fn new(out: &Path) -> Result<Self, String> {
        let create = |name: &str| {
            let path = out.join(name);
            std::fs::File::create(&path).map_err(|e| format!("{}: {e}", path.display()))
        };
        Ok(Watch {
            console: create("console.log")?,
            qemu_log: create("qemu.log")?,
            lines: Lines::default(),
            seen: Seen::default(),
            clock: Instant::now(),
            ready_at: None,
            done_at: None,
            ddb_at: None,
            stop: None,
        })
    }

    fn take(&mut self, msg: Msg) -> bool {
        match msg {
            Msg::Out(bytes) => {
                let _ = self.console.write_all(&bytes);
                for line in self.lines.push(&bytes) {
                    self.seen.line(&line);
                }
                self.seen.prompt(&self.lines.partial());
                true
            }
            Msg::Err(line) => {
                let _ = writeln!(self.qemu_log, "{line}");
                true
            }
            Msg::End => false,
        }
    }

    /// Note the times of the markers, and say whether to stop QEMU, and why.
    fn check(&mut self, req: &Request) -> Option<Stop> {
        let now = Instant::now();
        let seen = &self.seen;
        if self.ready_at.is_none() && (seen.ready.is_some() || seen.login) {
            self.ready_at = Some(now);
        }
        if self.done_at.is_none() && seen.done.is_some() {
            self.done_at = Some(now);
        }
        if seen.ddb && self.ddb_at.is_none() {
            self.ddb_at = Some(now);
        }
        let secs = Duration::from_secs;
        if self.ddb_at.is_some_and(|t| now - t > secs(req.ddb_wait)) {
            Some(Stop::Ddb)
        } else if seen.login && seen.ready.is_none() {
            Some(Stop::Login)
        } else if self.ready_at.is_none() && now - self.clock > secs(req.boot_timeout) {
            Some(Stop::BootTimeout)
        } else if self
            .done_at
            .is_some_and(|t| now - t > secs(req.poweroff_timeout) || seen.login)
        {
            Some(Stop::NoPoweroff)
        } else if now - self.clock > secs(req.timeout) {
            Some(Stop::Timeout)
        } else {
            None
        }
    }

    /// Run QEMU once, until it exits or rb stops it.
    fn once(
        &mut self,
        req: &Request,
        command: &[String],
    ) -> Result<std::process::ExitStatus, String> {
        let mut child = Command::new(&command[0])
            .args(&command[1..])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("{}: {e}", command[0]))?;
        let mut stdin = child.stdin.take();
        let mut stdout = child.stdout.take().ok_or("no stdout from QEMU")?;
        let stderr = child.stderr.take().ok_or("no stderr from QEMU")?;
        let (send, recv) = mpsc::channel();
        let err_send = send.clone();
        std::thread::spawn(move || {
            let mut buf = [0u8; 4096];
            while let Ok(n) = stdout.read(&mut buf) {
                if n == 0 || send.send(Msg::Out(buf[..n].to_vec())).is_err() {
                    break;
                }
            }
            let _ = send.send(Msg::End);
        });
        std::thread::spawn(move || {
            use std::io::BufRead as _;
            for line in std::io::BufReader::new(stderr)
                .lines()
                .map_while(Result::ok)
            {
                let _ = err_send.send(Msg::Err(line));
            }
        });
        let mut sent_bt = self.ddb_at.is_some();
        loop {
            match recv.recv_timeout(Duration::from_millis(250)) {
                Ok(msg) => {
                    if !self.take(msg) {
                        break;
                    }
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
                Err(mpsc::RecvTimeoutError::Timeout) => {}
            }
            let why = self.check(req);
            if self.ddb_at.is_some() && !sent_bt {
                sent_bt = true;
                // FreeBSD's ddb.conf has printed the backtrace already, and NetBSD needs the
                // command. A second one does no harm.
                if let Some(s) = stdin.as_mut() {
                    let _ = s.write_all(b"bt\r");
                    let _ = s.flush();
                }
            }
            if let Some(why) = why {
                self.stop = Some(why);
                let _ = child.kill();
                break;
            }
        }
        drop(stdin);
        let status = child.wait().map_err(|e| format!("waiting for QEMU: {e}"))?;
        // What the readers have not sent yet.
        while let Ok(msg) = recv.recv_timeout(Duration::from_millis(100)) {
            if !self.take(msg) {
                break;
            }
        }
        Ok(status)
    }
}

/// Run the boot, and write `console.log`, `qemu.log`, `boot.json` and `results/`.
pub fn run(req: &Request) -> Result<Outcome, String> {
    prepare(req)?;
    let command = qemu_command(req);
    let mut outcome = Outcome {
        image: req.image.display().to_string(),
        firmware: req.firmware.name().to_string(),
        accel: if req.kvm { "kvm" } else { "tcg" }.to_string(),
        asked: req.conf.suites.clone(),
        command: command.clone(),
        ..Outcome::default()
    };
    let mut w = Watch::new(&req.out)?;
    let _ = writeln!(w.qemu_log, "{}", command.join(" "));
    loop {
        let status = w.once(req, &command)?;
        if w.stop.is_some() {
            break;
        }
        outcome.qemu_exit = status.code();
        let s = &w.seen;
        let rebooted =
            status.success() && s.ready.is_none() && !s.login && s.panic.is_none() && !s.ddb;
        if !rebooted || outcome.restarts >= req.reboots {
            break;
        }
        outcome.restarts += 1;
        let _ = writeln!(
            w.qemu_log,
            "rb boot: the guest rebooted before rb-rc started, starting it again"
        );
    }
    if let Some(rest) = w.lines.finish() {
        w.seen.line(&rest);
    }
    let since = |t: Option<Instant>| t.map(|t| (t - w.clock).as_secs_f64());
    outcome.ready_seconds = since(w.ready_at);
    outcome.done_seconds = since(w.done_at);
    outcome.seconds = w.clock.elapsed().as_secs_f64();
    outcome.result = result(w.stop, &w.seen).to_string();
    outcome.results = collect(req, &w.seen)?;
    outcome.seen = w.seen;
    clean(req);
    let json = serde_json::to_string_pretty(&outcome).map_err(|e| e.to_string())?;
    std::fs::write(req.out.join("boot.json"), json + "\n")
        .map_err(|e| format!("boot.json: {e}"))?;
    Ok(outcome)
}

/// Unpack the results to `results/`, from the results disk or else from the console, and say
/// which.
fn collect(req: &Request, seen: &Seen) -> Result<Option<String>, String> {
    let entries = ustar::read_disk(&paths(req).results_disk).unwrap_or_default();
    let results = req.out.join("results");
    if entries.iter().any(|e| e.path != "rb.conf") {
        ustar::unpack(&entries, &results)?;
        return Ok(Some("disk".into()));
    }
    if let Some(tar) = &seen.console_tar {
        let entries = ustar::read(tar).map_err(|e| format!("the console archive: {e}"))?;
        ustar::unpack(&entries, &results)?;
        return Ok(Some("console".into()));
    }
    Ok(None)
}

/// Remove the disks, unless `--keep`.
fn clean(req: &Request) {
    if req.keep {
        return;
    }
    let p = paths(req);
    let _ = std::fs::remove_file(&p.results_disk);
    if !req.in_place {
        let _ = std::fs::remove_file(&p.disk);
    }
    let _ = std::fs::remove_file(&p.vars);
    for i in 1..=req.spare_disks {
        let _ = std::fs::remove_file(req.out.join(format!("spare{i}.disk")));
    }
}

/// The result word.
fn result(stop: Option<Stop>, seen: &Seen) -> &'static str {
    if seen.panic.is_some() || seen.ddb || stop == Some(Stop::Ddb) {
        return "panic";
    }
    match stop {
        Some(Stop::BootTimeout) => "boot-timeout",
        Some(Stop::Timeout) => "timeout",
        Some(Stop::NoPoweroff) => "no-poweroff",
        Some(Stop::Login) => "login",
        Some(Stop::Ddb) => "panic",
        None if seen.done.is_some() => "pass",
        None if seen.ready.is_some() => "no-done",
        None => "no-ready",
    }
}

/// The boot as markdown.
#[must_use]
pub fn summary(o: &Outcome, out: &Path) -> String {
    let mut s = String::new();
    let _ = writeln!(
        s,
        "# rb boot: {}\n\n{} with {} under {}, in {:.0} seconds.\n",
        o.result, o.image, o.firmware, o.accel, o.seconds
    );
    let _ = writeln!(s, "| | |\n|---|---|");
    let _ = writeln!(s, "| suites asked | {} |", o.asked.join(", "));
    if let Some(r) = &o.seen.ready {
        let _ = writeln!(
            s,
            "| rb-rc started | {:.0} s: `{r}` |",
            o.ready_seconds.unwrap_or(0.0)
        );
    } else if o.seen.login {
        let _ = writeln!(
            s,
            "| login prompt | {:.0} s, no rb-rc |",
            o.ready_seconds.unwrap_or(0.0)
        );
    }
    if o.restarts > 0 {
        let _ = writeln!(s, "| started again after a reboot | {} |", o.restarts);
    }
    for suite in &o.seen.suites {
        let _ = writeln!(
            s,
            "| {} | {} |",
            suite.name,
            suite.result.as_deref().unwrap_or("did not end")
        );
    }
    let failed: Vec<&str> = o
        .seen
        .checks
        .iter()
        .filter(|(_, r)| r == "fail")
        .map(|(n, _)| n.as_str())
        .collect();
    if !failed.is_empty() {
        let _ = writeln!(s, "| smoke checks failed | {} |", failed.join(", "));
    }
    if let Some(panic) = &o.seen.panic {
        let _ = writeln!(s, "| panic | `{}` |", panic.replace('|', "\\|"));
    }
    if o.result == "panic" {
        let _ = writeln!(
            s,
            "| running at the panic | {} |",
            o.seen.panic_case.as_deref().unwrap_or("no test case")
        );
    }
    if o.seen.forced_poweroff {
        let _ = writeln!(
            s,
            "| shutdown | did not end in time, so rb-rc powered off with no sync; see `RB-PS` and `RB-MOUNT` in the console |"
        );
    }
    if let Some(code) = o.qemu_exit
        && code != 0
    {
        let _ = writeln!(s, "| QEMU exit status | {code} |");
    }
    let _ = writeln!(
        s,
        "| results | {} |",
        match &o.results {
            Some(from) => format!("{} (from the {from})", out.join("results").display()),
            None => "none".to_string(),
        }
    );
    let _ = writeln!(
        s,
        "\nThe console is in {}, and QEMU's messages are in {}.",
        out.join("console.log").display(),
        out.join("qemu.log").display()
    );
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request() -> Request {
        Request {
            image: "/img/fb.img".into(),
            out: "/runs/1".into(),
            conf: RbConf::default(),
            qemu: "qemu-system-x86_64".into(),
            machine: "q35".into(),
            kvm: true,
            cpu: None,
            virtio: Bus::Pci,
            cpus: 2,
            memory: "8G".into(),
            firmware: Firmware::Ovmf {
                code: "/ovmf/CODE.fd".into(),
                vars: "/ovmf/VARS.fd".into(),
            },
            boot_timeout: 600,
            timeout: 14400,
            poweroff_timeout: 300,
            ddb_wait: 30,
            disk_size: 1 << 30,
            spare_disks: 1,
            reboots: 1,
            in_place: false,
            keep: false,
        }
    }

    #[test]
    fn the_qemu_command() {
        let line = qemu_command(&request()).join(" ");
        assert_eq!(
            line,
            "qemu-system-x86_64 -machine q35,accel=kvm -cpu host -smp 2 -m 8G -display none \
             -monitor none -serial stdio -no-reboot \
             -drive if=pflash,format=raw,readonly=on,file=/ovmf/CODE.fd \
             -drive if=pflash,format=raw,file=/runs/1/ovmf-vars.fd \
             -drive if=none,id=root,file=/runs/1/disk.qcow2,format=qcow2,cache=unsafe \
             -device virtio-blk-pci,drive=root,bootindex=0 \
             -drive if=none,id=rb,file=/runs/1/results.disk,format=raw,cache=unsafe \
             -device virtio-blk-pci,drive=rb \
             -drive if=none,id=spare1,file=/runs/1/spare1.disk,format=raw,cache=unsafe \
             -device virtio-blk-pci,drive=spare1 \
             -netdev user,id=n0,restrict=on -device virtio-net-pci,netdev=n0"
        );
        let mut req = request();
        req.kvm = false;
        req.firmware = Firmware::Seabios;
        req.in_place = true;
        req.spare_disks = 0;
        let line = qemu_command(&req).join(" ");
        assert!(line.contains("-machine q35,accel=tcg -smp 2"), "{line}");
        assert!(
            !line.contains("pflash") && !line.contains("spare"),
            "{line}"
        );
        assert!(line.contains("file=/img/fb.img,format=raw,"), "{line}");
    }

    #[test]
    fn the_qemu_commands_of_the_virt_machines() {
        let mut req = request();
        req.qemu = "qemu-system-aarch64".into();
        req.machine = "virt".into();
        req.kvm = false;
        req.cpu = Some("max,pauth-impdef=on".into());
        req.spare_disks = 0;
        req.firmware = Firmware::Edk2 {
            code: "/edk2/CODE.fd".into(),
            vars: "/edk2/VARS.fd".into(),
        };
        let line = qemu_command(&req).join(" ");
        assert!(
            line.starts_with(
                "qemu-system-aarch64 -machine virt,accel=tcg -cpu max,pauth-impdef=on -smp 2 "
            ),
            "{line}"
        );
        assert!(
            line.contains(
                "-drive if=pflash,format=raw,readonly=on,file=/edk2/CODE.fd \
                 -drive if=pflash,format=raw,file=/runs/1/edk2-vars.fd "
            ),
            "{line}"
        );
        assert!(
            line.contains("virtio-blk-pci,drive=root,bootindex=0") && line.contains("-pci,netdev"),
            "{line}"
        );
        req.qemu = "qemu-system-riscv64".into();
        req.cpu = None;
        req.virtio = Bus::Mmio;
        req.spare_disks = 1;
        req.firmware = Firmware::UBoot {
            image: "/u/u-boot.bin".into(),
        };
        let line = qemu_command(&req).join(" ");
        assert_eq!(
            line,
            "qemu-system-riscv64 -machine virt,accel=tcg -smp 2 -m 8G -display none \
             -monitor none -serial stdio -no-reboot -kernel /u/u-boot.bin \
             -drive if=none,id=spare1,file=/runs/1/spare1.disk,format=raw,cache=unsafe \
             -device virtio-blk-device,drive=spare1 \
             -drive if=none,id=rb,file=/runs/1/results.disk,format=raw,cache=unsafe \
             -device virtio-blk-device,drive=rb \
             -drive if=none,id=root,file=/runs/1/disk.qcow2,format=qcow2,cache=unsafe \
             -device virtio-blk-device,drive=root \
             -netdev user,id=n0,restrict=on -device virtio-net-device,netdev=n0"
        );
        req.kvm = true;
        req.cpu = Some("max".into());
        assert!(qemu_command(&req).join(" ").contains("-cpu host -smp"));
    }

    #[test]
    fn the_overlay() {
        let mut req = request();
        req.qemu = "/opt/q/bin/qemu-system-x86_64".into();
        assert_eq!(
            overlay_command(&req).join(" "),
            "/opt/q/bin/qemu-img create -q -f qcow2 -F raw -b /img/fb.img /runs/1/disk.qcow2"
        );
        assert_eq!(qemu_img(&request()), "qemu-img");
    }

    #[test]
    fn the_results_disk_names_the_suites() {
        let conf = RbConf {
            suites: vec!["boot".into(), "smoke".into()],
            ..RbConf::default()
        };
        let tar = results_disk(&conf, 1 << 20).unwrap();
        let entries = ustar::read(&tar).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].path, "rb.conf");
        let text = String::from_utf8(entries[0].data.clone()).unwrap();
        assert!(text.contains("rb_suites='boot smoke'\n"), "{text}");
        assert!(text.contains("rb_poweroff=YES\n"), "{text}");
        assert!(results_disk(&conf, 512).is_err());
    }

    #[test]
    fn results() {
        let mut seen = Seen::default();
        assert_eq!(result(None, &seen), "no-ready");
        assert_eq!(result(Some(Stop::BootTimeout), &seen), "boot-timeout");
        seen.ready = Some("FreeBSD".into());
        assert_eq!(result(None, &seen), "no-done");
        assert_eq!(result(Some(Stop::Timeout), &seen), "timeout");
        seen.done = Some("boot=pass archive=disk".into());
        assert_eq!(result(None, &seen), "pass");
        assert_eq!(result(Some(Stop::NoPoweroff), &seen), "no-poweroff");
        seen.ddb = true;
        assert_eq!(result(None, &seen), "panic");
        let o = Outcome {
            result: "login".into(),
            asked: vec!["boot".into()],
            ..Outcome::default()
        };
        assert!(o.passed());
        let o = Outcome {
            asked: vec!["boot".into(), "smoke".into()],
            ..o
        };
        assert!(!o.passed());
    }
}

/// The command line: `rb boot --image IMG --out DIR ...`, with the row's QEMU settings from
/// `rows.toml`.
pub mod cli {
    use super::{Bus, Firmware, Request, find_edk2, find_ovmf, find_u_boot, kvm_usable};
    use crate::cli::Args;
    use rb_image::conf::{RbConf, parse_suites};
    use rb_image::plan::parse_size;
    use rb_pins::places::Rows;
    use std::path::PathBuf;

    fn number<T: std::str::FromStr>(args: &Args, name: &str, default: T) -> Result<T, String> {
        match args.get(name) {
            Some(v) => v
                .parse()
                .map_err(|_| format!("--{name} {v} is not a number")),
            None => Ok(default),
        }
    }

    fn list(args: &Args, name: &str) -> Vec<String> {
        args.get(name)
            .map(|v| {
                v.split(',')
                    .filter(|s| !s.is_empty())
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default()
    }

    /// The request the command line asks for. `kvm` says whether KVM can be used here.
    pub fn request(
        args: &Args,
        rows: &Rows,
        kvm: impl Fn(&str) -> bool,
    ) -> Result<Request, String> {
        let image = PathBuf::from(args.get("image").ok_or("give --image IMG")?);
        let out = PathBuf::from(args.get("out").ok_or("give --out DIR")?);
        let row_name = args.get("row").unwrap_or("amd64");
        let row = rows
            .row
            .get(row_name)
            .ok_or_else(|| format!("no row {row_name} in rows.toml"))?;
        let machine = row
            .machine
            .clone()
            .ok_or_else(|| format!("the row {row_name} has no QEMU machine"))?;
        if machine != "q35" && machine != "virt" {
            return Err(format!(
                "rb boot boots the q35 and virt machines, and {row_name} is {machine}"
            ));
        }
        let qemu = match args.get("qemu") {
            Some(q) => q.to_string(),
            None => row
                .qemu
                .clone()
                .ok_or_else(|| format!("the row {row_name} has no qemu"))?,
        };
        let firmware_name = args
            .get("firmware")
            .or(row.test_firmware.as_deref())
            .unwrap_or("seabios");
        if !row.firmware.iter().any(|f| f == firmware_name) {
            return Err(format!(
                "the row {row_name} boots with {}, not {firmware_name}",
                row.firmware.join(" or ")
            ));
        }
        let firmware = match firmware_name {
            "seabios" => Firmware::Seabios,
            "ovmf" => {
                let (code, vars) = match (args.get("ovmf-code"), args.get("ovmf-vars")) {
                    (Some(c), Some(v)) => (PathBuf::from(c), PathBuf::from(v)),
                    (None, None) => find_ovmf().ok_or(
                        "no OVMF found; install ovmf, give --ovmf-code and --ovmf-vars, or use --firmware seabios",
                    )?,
                    _ => return Err("give both --ovmf-code and --ovmf-vars".into()),
                };
                Firmware::Ovmf { code, vars }
            }
            "edk2" => {
                let (code, vars) = match (args.get("edk2-code"), args.get("edk2-vars")) {
                    (Some(c), Some(v)) => (PathBuf::from(c), PathBuf::from(v)),
                    (None, None) => find_edk2().ok_or(
                        "no EDK2 for aarch64 found; install qemu-efi-aarch64, or give --edk2-code and --edk2-vars",
                    )?,
                    _ => return Err("give both --edk2-code and --edk2-vars".into()),
                };
                Firmware::Edk2 { code, vars }
            }
            "u-boot" => Firmware::UBoot {
                image: match args.get("u-boot") {
                    Some(u) => PathBuf::from(u),
                    None => find_u_boot().ok_or(
                        "no S-mode U-Boot for riscv64 found; install u-boot-qemu, or give --u-boot",
                    )?,
                },
            },
            other => return Err(format!("rb boot cannot boot with {other} yet")),
        };
        let kvm = row.accel.as_deref() == Some("kvm") && kvm(&qemu);
        let hours = row.run_limit_hours.unwrap_or(4);
        let conf = RbConf {
            suites: parse_suites(args.get("suite").unwrap_or("boot"))?,
            tests_only: list(args, "tests-only"),
            tests_except: list(args, "tests-except"),
            ..RbConf::default()
        };
        Ok(Request {
            image,
            out,
            conf,
            qemu,
            machine,
            kvm,
            cpu: row.cpu.clone(),
            virtio: if row.virtio.as_deref() == Some("mmio") {
                Bus::Mmio
            } else {
                Bus::Pci
            },
            cpus: number(args, "cpus", row.cpus.unwrap_or(2))?,
            memory: args.get("memory").map_or_else(
                || format!("{}G", row.memory_gb.unwrap_or(8)),
                str::to_string,
            ),
            firmware,
            boot_timeout: number(args, "boot-timeout", if kvm { 600 } else { 1800 })?,
            timeout: number(args, "timeout", u64::from(hours) * 3600)?,
            poweroff_timeout: 300,
            ddb_wait: 30,
            disk_size: args.get("disk-size").map_or(Ok(1 << 30), parse_size)?,
            spare_disks: number(args, "spare-disks", 0)?,
            reboots: number(args, "reboots", 1)?,
            in_place: args.has("in-place"),
            keep: args.has("keep"),
        })
    }

    /// Run `rb boot`, and return the report and whether the boot passed.
    pub fn run(args: &Args, rows: &Rows) -> Result<(String, bool), String> {
        let req = request(args, rows, kvm_usable)?;
        if args.has("dry-run") {
            let mut s = String::from("# rb boot: a dry run\n\n```\n");
            if !req.in_place {
                s.push_str(&super::overlay_command(&req).join(" "));
                s.push('\n');
            }
            s.push_str(&super::qemu_command(&req).join(" "));
            s.push_str("\n```\n\nThe rb.conf on the results disk:\n\n```\n");
            s.push_str(&req.conf.render());
            s.push_str("```\n");
            return Ok((s, true));
        }
        let outcome = super::run(&req)?;
        Ok((super::summary(&outcome, &req.out), outcome.passed()))
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use crate::cli as top;

        fn args(words: &[&str]) -> Args {
            let words: Vec<String> = words.iter().map(|w| (*w).to_string()).collect();
            Args::parse(&words, top::accepted).unwrap()
        }

        fn rows() -> Rows {
            Rows::parse(include_str!("../../../rows.toml")).unwrap()
        }

        #[test]
        fn requests_from_the_row() {
            let a = args(&[
                "boot",
                "--image",
                "fb.img",
                "--out",
                "run",
                "--firmware",
                "seabios",
                "--suite",
                "boot,smoke",
                "--tests-only",
                "sys,lib",
            ]);
            let req = request(&a, &rows(), |_| true).unwrap();
            assert_eq!(req.qemu, "qemu-system-x86_64");
            assert_eq!(req.machine, "q35");
            assert!(req.kvm);
            assert_eq!((req.cpus, req.memory.as_str()), (2, "8G"));
            assert_eq!(req.firmware, Firmware::Seabios);
            assert_eq!((req.boot_timeout, req.timeout), (600, 4 * 3600));
            assert_eq!(req.conf.suites, ["boot", "smoke"]);
            assert_eq!(req.conf.tests_only, ["sys", "lib"]);
            let req = request(&a, &rows(), |_| false).unwrap();
            assert!(!req.kvm);
            assert_eq!(req.boot_timeout, 1800);

            let a = args(&[
                "boot",
                "--image",
                "i",
                "--out",
                "o",
                "--ovmf-code",
                "/c",
                "--ovmf-vars",
                "/v",
                "--memory",
                "2G",
                "--timeout",
                "60",
            ]);
            let req = request(&a, &rows(), |_| true).unwrap();
            assert_eq!(
                req.firmware,
                Firmware::Ovmf {
                    code: "/c".into(),
                    vars: "/v".into()
                }
            );
            assert_eq!((req.memory.as_str(), req.timeout), ("2G", 60));
        }

        #[test]
        fn requests_for_the_virt_rows() {
            let a = args(&[
                "boot",
                "--image",
                "i",
                "--out",
                "o",
                "--row",
                "aarch64",
                "--edk2-code",
                "/c",
                "--edk2-vars",
                "/v",
            ]);
            let req = request(&a, &rows(), |_| false).unwrap();
            assert_eq!(
                (req.qemu.as_str(), req.machine.as_str()),
                ("qemu-system-aarch64", "virt")
            );
            assert_eq!(req.cpu.as_deref(), Some("max,pauth-impdef=on"));
            assert!(!req.kvm && req.virtio == Bus::Pci);
            assert_eq!((req.cpus, req.boot_timeout), (4, 1800));
            assert_eq!(
                req.firmware,
                Firmware::Edk2 {
                    code: "/c".into(),
                    vars: "/v".into()
                }
            );
            let a = args(&[
                "boot", "--image", "i", "--out", "o", "--row", "riscv64", "--u-boot", "/u",
            ]);
            let req = request(&a, &rows(), |_| true).unwrap();
            assert_eq!(req.qemu, "qemu-system-riscv64");
            assert!(!req.kvm, "the row is TCG");
            assert!(req.virtio == Bus::Mmio && req.cpu.is_none());
            assert_eq!(
                req.firmware,
                Firmware::UBoot {
                    image: "/u".into()
                }
            );
        }

        #[test]
        fn mistakes() {
            let err = |words: &[&str]| request(&args(words), &rows(), |_| true).unwrap_err();
            assert!(err(&["boot", "--out", "o"]).contains("--image"));
            assert!(
                err(&["boot", "--image", "i", "--out", "o", "--row", "i386", "--firmware", "ovmf"])
                    .contains("seabios, not ovmf")
            );
            assert!(
                err(&[
                    "boot",
                    "--image",
                    "i",
                    "--out",
                    "o",
                    "--row",
                    "aarch64",
                    "--edk2-code",
                    "/c"
                ])
                .contains("both --edk2-code")
            );
            assert!(
                err(&["boot", "--image", "i", "--out", "o", "--firmware", "edk2"])
                    .contains("ovmf or seabios")
            );
            assert!(
                err(&["boot", "--image", "i", "--out", "o", "--ovmf-code", "/c"]).contains("both")
            );
            assert!(
                err(&[
                    "boot",
                    "--image",
                    "i",
                    "--out",
                    "o",
                    "--firmware",
                    "seabios",
                    "--suite",
                    "kyua"
                ])
                .contains("kyua")
            );
        }

        #[test]
        fn dry_run() {
            let a = args(&[
                "boot",
                "--image",
                "/img/fb.img",
                "--out",
                "/nowhere/run",
                "--firmware",
                "seabios",
                "--dry-run",
            ]);
            let (text, passed) = run(&a, &rows()).unwrap();
            assert!(passed);
            assert!(
                text.contains(
                    "qemu-img create -q -f qcow2 -F raw -b /img/fb.img /nowhere/run/disk.qcow2\n"
                ),
                "{text}"
            );
            assert!(text.contains("rb_suites=boot\n"), "{text}");
            assert!(!std::path::Path::new("/nowhere").exists());
        }
    }
}
