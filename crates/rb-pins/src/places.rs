//! `builders.toml` and `rows.toml`: where builds run, and how each architecture boots
//! (plan 11.1, 12.2, 12.3 and 14.6).

use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::Path;

/// The file of builders and machines.
pub const BUILDERS: &str = "builders.toml";
/// The file of architecture rows.
pub const ROWS: &str = "rows.toml";

/// The whole of `builders.toml`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Builders {
    /// The builders by name.
    pub builder: BTreeMap<String, Builder>,
    /// The machines by name.
    pub machine: BTreeMap<String, Machine>,
}

/// A builder VM or container.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Builder {
    /// The system it builds.
    pub system: String,
    /// `vm` or `container`.
    pub kind: String,
    /// The release a VM runs.
    pub release: Option<String>,
    /// The image file or container image.
    pub image: String,
    /// Where a VM image is published.
    pub url: Option<String>,
    /// The project's checksum file for the image.
    pub checksum_url: Option<String>,
    /// The image's SHA-256.
    pub sha256: Option<String>,
    /// A container image's digest.
    pub digest: Option<String>,
    /// The toolchains of `toolchains.toml` installed in it.
    #[serde(default)]
    pub toolchains: Vec<String>,
    /// The QEMU binary of a VM.
    pub qemu: Option<String>,
    /// The QEMU machine of a VM.
    pub machine: Option<String>,
    /// The QEMU accelerator.
    pub accel: Option<String>,
    /// Where it runs.
    pub host: Vec<Host>,
}

/// One place a builder runs.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Host {
    /// The machine.
    pub on: String,
    /// The vCPUs it gets: a number, or `all`.
    pub cpus: Option<Cpus>,
    /// The memory it gets.
    pub memory_gb: Option<u32>,
    /// The size of its build disk.
    pub build_disk_gb: Option<u32>,
}

/// A count of CPUs, or all of the machine's.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(untagged)]
pub enum Cpus {
    /// This many.
    Count(u32),
    /// `all`.
    Word(String),
}

/// A machine that runs builders or boots.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Machine {
    /// Its cores, where known.
    pub cores: Option<u32>,
    /// Its architecture.
    pub arch: String,
    /// What it does.
    pub role: String,
}

/// The whole of `rows.toml`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rows {
    /// The rows by name.
    pub row: BTreeMap<String, Row>,
}

/// One architecture row.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Row {
    /// The milestone that adds the row, per system.
    pub from: BTreeMap<String, String>,
    /// rucc's architecture name.
    pub rucc_arch: String,
    /// FreeBSD's names.
    pub freebsd: Names,
    /// NetBSD's names.
    pub netbsd: Names,
    /// A row tested inside another row's images.
    pub in_row: Option<String>,
    /// The QEMU binary.
    pub qemu: Option<String>,
    /// The QEMU machine.
    pub machine: Option<String>,
    /// `kvm` or `tcg`.
    pub accel: Option<String>,
    /// The firmware each run boots with.
    #[serde(default)]
    pub firmware: Vec<String>,
    /// The firmware of the boot the tests run on.
    pub test_firmware: Option<String>,
    /// The guest's vCPUs.
    pub cpus: Option<u32>,
    /// The guest's memory.
    pub memory_gb: Option<u32>,
    /// The console the markers are read from.
    pub console: Option<String>,
    /// The limit on a whole test run.
    pub run_limit_hours: Option<u32>,
    /// The machines that run the tests.
    #[serde(default)]
    pub test_on: Vec<String>,
}

/// What one system calls an architecture.
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Names {
    /// FreeBSD's `TARGET`.
    pub target: Option<String>,
    /// FreeBSD's `TARGET_ARCH`.
    pub target_arch: Option<String>,
    /// NetBSD's `MACHINE`.
    pub machine: Option<String>,
    /// NetBSD's `MACHINE_ARCH`.
    pub machine_arch: Option<String>,
    /// The build knob for a row built inside another.
    pub knob: Option<String>,
    /// The series the row exists for, when not all.
    #[serde(default)]
    pub series: Vec<String>,
}

impl Builders {
    /// Parse the text of `builders.toml` and check that every host is a machine.
    pub fn parse(text: &str) -> Result<Self, String> {
        let file: Self = toml::from_str(text).map_err(|e| format!("{BUILDERS}: {e}"))?;
        for (name, b) in &file.builder {
            for host in &b.host {
                if !file.machine.contains_key(&host.on) {
                    return Err(format!(
                        "{BUILDERS}: {name} runs on {}, which is not a machine",
                        host.on
                    ));
                }
                if let Some(Cpus::Word(w)) = &host.cpus
                    && w != "all"
                {
                    return Err(format!(
                        "{BUILDERS}: {name} cpus is {w:?}; give a number or \"all\""
                    ));
                }
            }
            if b.kind == "vm" && b.sha256.is_none() {
                return Err(format!("{BUILDERS}: the VM {name} has no sha256"));
            }
            if b.kind == "container" && b.digest.is_none() {
                return Err(format!("{BUILDERS}: the container {name} has no digest"));
            }
        }
        Ok(file)
    }

    /// Read `builders.toml` in `root`.
    pub fn load(root: &Path) -> Result<Self, String> {
        Self::parse(&read(root, BUILDERS)?)
    }
}

impl Rows {
    /// Parse the text of `rows.toml` and check that a row inside another names a row.
    pub fn parse(text: &str) -> Result<Self, String> {
        let file: Self = toml::from_str(text).map_err(|e| format!("{ROWS}: {e}"))?;
        for (name, row) in &file.row {
            if let Some(inside) = &row.in_row
                && !file.row.contains_key(inside)
            {
                return Err(format!(
                    "{ROWS}: {name} is inside {inside}, which is not a row"
                ));
            }
            if row.in_row.is_none() && (row.qemu.is_none() || row.test_firmware.is_none()) {
                return Err(format!("{ROWS}: {name} has no qemu or test_firmware"));
            }
            if let Some(f) = &row.test_firmware
                && !row.firmware.contains(f)
            {
                return Err(format!(
                    "{ROWS}: {name} tests on {f}, which is not in its firmware"
                ));
            }
        }
        Ok(file)
    }

    /// Read `rows.toml` in `root`.
    pub fn load(root: &Path) -> Result<Self, String> {
        Self::parse(&read(root, ROWS)?)
    }
}

fn read(root: &Path, name: &str) -> Result<String, String> {
    let path = root.join(name);
    std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mistakes_in_builders_are_named() {
        let base = "[machine.m]\narch = \"x86_64\"\nrole = \"r\"\n\
                    [builder.b]\nsystem = \"freebsd\"\nkind = \"vm\"\nimage = \"i\"\nsha256 = \"s\"\n\
                    [[builder.b.host]]\non = \"m\"\ncpus = 4\n";
        assert!(Builders::parse(base).is_ok());
        let wrong_host = base.replace("on = \"m\"", "on = \"n\"");
        assert!(
            Builders::parse(&wrong_host)
                .unwrap_err()
                .contains("not a machine")
        );
        let wrong_cpus = base.replace("cpus = 4", "cpus = \"many\"");
        assert!(Builders::parse(&wrong_cpus).unwrap_err().contains("cpus"));
        let no_sum = base.replace("sha256 = \"s\"\n", "");
        assert!(Builders::parse(&no_sum).unwrap_err().contains("sha256"));
    }

    #[test]
    fn mistakes_in_rows_are_named() {
        let base = "[row.a]\nfrom = { netbsd = \"B3\" }\nrucc_arch = \"x86_64\"\n\
                    freebsd = { target = \"amd64\" }\nnetbsd = { machine = \"amd64\" }\n\
                    qemu = \"q\"\nfirmware = [\"ovmf\"]\ntest_firmware = \"ovmf\"\n";
        assert!(Rows::parse(base).is_ok());
        let firmware = base.replace("test_firmware = \"ovmf\"", "test_firmware = \"edk2\"");
        assert!(Rows::parse(&firmware).unwrap_err().contains("edk2"));
        let inside = format!(
            "{base}[row.b]\nfrom = {{}}\nrucc_arch = \"i686\"\nin_row = \"c\"\nfreebsd = {{}}\nnetbsd = {{}}\n"
        );
        assert!(Rows::parse(&inside).unwrap_err().contains("not a row"));
    }
}
