//! The files rb adds to every image (plan 12.3): rb-rc, the suite scripts and `rb.conf`.
//!
//! The scripts live in `guest/rb-rc` and `smoke` at the top of the repository, and are built into
//! rb, so the binary that makes an image always carries the scripts of its own commit.

use crate::conf::RbConf;
use std::path::Path;

/// `/etc/rc.d/rb`, the rc script.
pub const RC_SCRIPT: &str = include_str!("../../../guest/rb-rc/rb");
/// `/usr/libexec/rb/run`, the body of rb-rc.
pub const RUN: &str = include_str!("../../../guest/rb-rc/run.sh");
/// `/usr/libexec/rb/tests`, kyua or atf-run.
pub const TESTS: &str = include_str!("../../../guest/rb-rc/tests.sh");
/// `/usr/libexec/rb/modules`, the modules suite of rb cross-modules.
pub const MODULES: &str = include_str!("../../../guest/rb-rc/modules.sh");
/// `/usr/libexec/rb/smoke`, the smoke suite.
pub const SMOKE: &str = include_str!("../../../smoke/smoke.sh");

/// The line that enables rb-rc in `rc.conf`.
pub const RC_CONF: &str = "rb_enable=\"YES\"\n";

/// One file of the overlay.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuestFile {
    /// The path in the image, relative to its root.
    pub path: &'static str,
    /// The permission bits.
    pub mode: u32,
    /// The contents.
    pub data: String,
}

/// The directories the overlay needs, parents first.
pub const DIRS: &[&str] = &["usr/libexec/rb"];

/// The files rb adds to an image with these settings.
#[must_use]
pub fn files(conf: &RbConf) -> Vec<GuestFile> {
    let file = |path, mode, data: &str| GuestFile {
        path,
        mode,
        data: data.to_string(),
    };
    vec![
        file("etc/rc.d/rb", 0o555, RC_SCRIPT),
        file("usr/libexec/rb/run", 0o555, RUN),
        file("usr/libexec/rb/modules", 0o555, MODULES),
        file("usr/libexec/rb/smoke", 0o555, SMOKE),
        file("usr/libexec/rb/tests", 0o555, TESTS),
        file("etc/rb.conf", 0o644, &conf.render()),
    ]
}

/// Write the overlay under `root`, a staging tree or a scratch directory. A file that is already
/// there is removed first, because rb's scripts are read-only and a second run must replace them.
pub fn write_tree(root: &Path, files: &[GuestFile]) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt as _;
    for f in files {
        let path = root.join(f.path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
        }
        match std::fs::remove_file(&path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
                return Err(format!("{}: {e}", path.display()));
            }
            _ => {}
        }
        std::fs::write(&path, &f.data).map_err(|e| format!("{}: {e}", path.display()))?;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(f.mode))
            .map_err(|e| format!("{}: {e}", path.display()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt as _;

    #[test]
    fn scripts_are_built_in() {
        assert!(RC_SCRIPT.contains("# PROVIDE: rb"));
        assert!(RC_SCRIPT.contains("rcvar=rb_enable"));
        assert!(RUN.contains("RB-READY") && RUN.contains("RB-DONE"));
        assert!(SMOKE.contains("RB-CHECK"));
        assert!(TESTS.contains("kyua") && TESTS.contains("atf-run"));
        assert!(MODULES.contains("modload") && MODULES.contains("RB-CASE"));
        for s in [RC_SCRIPT, RUN, MODULES, SMOKE, TESTS] {
            assert!(s.starts_with("#!/bin/sh\n"));
        }
    }

    #[test]
    fn overlay() {
        let files = files(&RbConf::default());
        let paths: Vec<_> = files.iter().map(|f| f.path).collect();
        assert_eq!(
            paths,
            [
                "etc/rc.d/rb",
                "usr/libexec/rb/run",
                "usr/libexec/rb/modules",
                "usr/libexec/rb/smoke",
                "usr/libexec/rb/tests",
                "etc/rb.conf"
            ]
        );
        for f in &files {
            assert!(DIRS.iter().any(|d| f.path.starts_with(d)) || f.path.starts_with("etc/"));
        }
        let root = std::env::temp_dir().join(format!("rb-image-overlay-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        write_tree(&root, &files).unwrap();
        let meta = std::fs::metadata(root.join("usr/libexec/rb/run")).unwrap();
        assert_eq!(meta.permissions().mode() & 0o777, 0o555);
        std::fs::remove_dir_all(&root).unwrap();
    }
}
