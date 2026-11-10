//! `/etc/rb.conf`: what rb-rc runs at boot (plan 12.4).
//!
//! `rb image` writes it into the image. `rb boot` can put another one on the results disk, and
//! rb-rc then reads that one after the image's. The file is shell: one `rb_<name>=<value>` line
//! per setting that is not the default. The settings are listed in `guest/rb-rc/run.sh`,
//! `guest/rb-rc/tests.sh` and `smoke/smoke.sh`.

use harness_agent::job::quote;
use std::fmt::Write as _;

/// The suites rb-rc knows (plan 12.2).
pub const SUITES: &[&str] = &["boot", "smoke", "tests"];

/// The settings of one boot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RbConf {
    /// The suites to run, in order.
    pub suites: Vec<String>,
    /// The results disk. `None` takes the system's default: `/dev/vtbd1` on FreeBSD, and on
    /// NetBSD the raw partition of the `ld` disk after the root's disk, such as `/dev/rld1d` on
    /// amd64 and `/dev/rld5c` on evbarm and riscv. An empty string means no disk, and the
    /// results go to the console.
    pub disk: Option<String>,
    /// Whether rb-rc powers the VM off when it is done.
    pub poweroff: bool,
    /// The local package repository in the image.
    pub pkg_dir: Option<String>,
    /// The package the smoke suite installs.
    pub smoke_package: Option<String>,
    /// The module the smoke suite loads and unloads.
    pub smoke_module: Option<String>,
    /// The partition the smoke suite checks and mounts.
    pub smoke_spare: Option<String>,
    /// Run only these top-level test directories.
    pub tests_only: Vec<String>,
    /// Run every top-level test directory but these.
    pub tests_except: Vec<String>,
    /// kyua's parallelism.
    pub kyua_parallelism: Option<u32>,
}

impl Default for RbConf {
    fn default() -> Self {
        RbConf {
            suites: vec!["boot".to_string()],
            disk: None,
            poweroff: true,
            pkg_dir: None,
            smoke_package: None,
            smoke_module: None,
            smoke_spare: None,
            tests_only: Vec::new(),
            tests_except: Vec::new(),
            kyua_parallelism: None,
        }
    }
}

/// Read a comma-separated list of suites, such as `boot,smoke,tests`.
pub fn parse_suites(list: &str) -> Result<Vec<String>, String> {
    let mut suites = Vec::new();
    for s in list.split(',').map(str::trim).filter(|s| !s.is_empty()) {
        if !SUITES.contains(&s) {
            return Err(format!(
                "unknown suite {s}, the suites are {}",
                SUITES.join(", ")
            ));
        }
        if !suites.iter().any(|x| x == s) {
            suites.push(s.to_string());
        }
    }
    if suites.is_empty() {
        return Err("no suites".to_string());
    }
    Ok(suites)
}

impl RbConf {
    /// The file, as shell.
    #[must_use]
    pub fn render(&self) -> String {
        let mut s = String::from("# rb-rc settings (plan 12.4), written by rb.\n");
        let mut set = |name: &str, value: &str| {
            let _ = writeln!(s, "rb_{name}={}", quote(value));
        };
        set("suites", &self.suites.join(" "));
        if let Some(d) = &self.disk {
            set("disk", d);
        }
        set("poweroff", if self.poweroff { "YES" } else { "NO" });
        for (name, value) in [
            ("pkg_dir", &self.pkg_dir),
            ("smoke_package", &self.smoke_package),
            ("smoke_module", &self.smoke_module),
            ("smoke_spare", &self.smoke_spare),
        ] {
            if let Some(v) = value {
                set(name, v);
            }
        }
        if !self.tests_only.is_empty() {
            set("tests_only", &self.tests_only.join(" "));
        }
        if !self.tests_except.is_empty() {
            set("tests_except", &self.tests_except.join(" "));
        }
        if let Some(n) = self.kyua_parallelism {
            set("kyua_parallelism", &n.to_string());
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn suites() {
        assert_eq!(parse_suites("boot, smoke,boot").unwrap(), ["boot", "smoke"]);
        assert!(
            parse_suites("boot,kyua")
                .unwrap_err()
                .contains("unknown suite kyua")
        );
        assert!(parse_suites(" , ").is_err());
    }

    #[test]
    fn render() {
        let conf = RbConf::default();
        assert_eq!(
            conf.render(),
            "# rb-rc settings (plan 12.4), written by rb.\nrb_suites=boot\nrb_poweroff=YES\n"
        );
        let conf = RbConf {
            suites: vec!["boot".into(), "tests".into()],
            disk: Some(String::new()),
            poweroff: false,
            smoke_package: Some("it's".into()),
            tests_except: vec!["net".into(), "lib".into()],
            kyua_parallelism: Some(2),
            ..RbConf::default()
        };
        let text = conf.render();
        assert!(text.contains("rb_suites='boot tests'\n"), "{text}");
        assert!(text.contains("rb_disk=''\n"), "{text}");
        assert!(text.contains("rb_poweroff=NO\n"), "{text}");
        assert!(text.contains(r"rb_smoke_package='it'\''s'"), "{text}");
        assert!(text.contains("rb_tests_except='net lib'\n"), "{text}");
        assert!(text.contains("rb_kyua_parallelism=2\n"), "{text}");
    }
}
