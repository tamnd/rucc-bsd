//! The commands that make an image, and how they run.
//!
//! An image is made in two parts. The files rb adds are written by rb itself. The file systems and
//! the disk are made by the systems' own tools, as a list of [`Step`]s. The list is computed
//! first, so `--dry-run` can print it and the tests can check it with no tools installed.

use harness_agent::job::{self, Host, Job, quote};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

/// One command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    /// A short name, also the name of its log file.
    pub name: String,
    /// The command and its arguments.
    pub argv: Vec<String>,
    /// The directory it runs in.
    pub cwd: Option<PathBuf>,
}

impl Step {
    /// A step from words.
    #[must_use]
    pub fn new(name: &str, argv: &[&str]) -> Self {
        Step {
            name: name.to_string(),
            argv: argv.iter().map(|s| (*s).to_string()).collect(),
            cwd: None,
        }
    }

    /// The same step, run in `dir`.
    #[must_use]
    pub fn in_dir(mut self, dir: &Path) -> Self {
        self.cwd = Some(dir.to_path_buf());
        self
    }

    /// The step as one shell line.
    #[must_use]
    pub fn line(&self) -> String {
        let words = self
            .argv
            .iter()
            .map(|w| quote(w))
            .collect::<Vec<_>>()
            .join(" ");
        match &self.cwd {
            Some(dir) => format!("(cd {} && {words})", quote(&dir.display().to_string())),
            None => words,
        }
    }
}

/// The steps as shell lines, one per line.
#[must_use]
pub fn script(steps: &[Step]) -> String {
    let mut s = String::new();
    for step in steps {
        let _ = writeln!(s, "{}", step.line());
    }
    s
}

/// Run the steps in order, each with its output in `logs/<name>.log`. Stops at the first step
/// that fails.
pub fn run(steps: &[Step], logs: &Path) -> Result<(), String> {
    std::fs::create_dir_all(logs).map_err(|e| format!("{}: {e}", logs.display()))?;
    for step in steps {
        let log = logs.join(format!("{}.log", step.name));
        let mut j = Job::new(&step.name, Host::Local, &[]);
        j.argv.clone_from(&step.argv);
        j.cwd = step.cwd.as_ref().map(|d| d.display().to_string());
        j.log = Some(log.clone());
        let outcome = job::run(&j);
        if !outcome.succeeded() {
            let why = outcome.error.unwrap_or_else(|| match outcome.exit {
                Some(code) => format!("exit status {code}"),
                None => "killed".to_string(),
            });
            let tail = std::fs::read_to_string(&log).unwrap_or_default();
            let tail: Vec<&str> = tail.lines().rev().take(20).collect();
            let tail: Vec<&str> = tail.into_iter().rev().collect();
            return Err(format!(
                "{} failed ({why}), log {}:\n{}",
                step.line(),
                log.display(),
                tail.join("\n")
            ));
        }
    }
    Ok(())
}

/// Read a size such as `10g`, `512m`, `64k` or a number of bytes.
pub fn parse_size(text: &str) -> Result<u64, String> {
    let t = text.trim().to_ascii_lowercase();
    let (digits, unit) = match t.char_indices().find(|(_, c)| !c.is_ascii_digit()) {
        Some((i, _)) => t.split_at(i),
        None => (t.as_str(), ""),
    };
    let shift = match unit {
        "" | "b" => 0,
        "k" | "kb" | "kib" => 10,
        "m" | "mb" | "mib" => 20,
        "g" | "gb" | "gib" => 30,
        "t" | "tb" | "tib" => 40,
        _ => return Err(format!("{text} is not a size, such as 10g or 512m")),
    };
    let n: u64 = digits
        .parse()
        .map_err(|_| format!("{text} is not a size, such as 10g or 512m"))?;
    n.checked_mul(1 << shift)
        .ok_or_else(|| format!("{text} is too large"))
}

/// Make a file at least `size` bytes long. The new part is a hole.
pub fn grow(path: &Path, size: u64) -> Result<bool, String> {
    let file = std::fs::OpenOptions::new()
        .write(true)
        .open(path)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    let now = file
        .metadata()
        .map_err(|e| format!("{}: {e}", path.display()))?
        .len();
    if now >= size {
        return Ok(false);
    }
    file.set_len(size)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes() {
        assert_eq!(parse_size("10g").unwrap(), 10 << 30);
        assert_eq!(parse_size("512M").unwrap(), 512 << 20);
        assert_eq!(parse_size("4096").unwrap(), 4096);
        assert!(parse_size("ten").is_err());
        assert!(parse_size("10x").is_err());
        assert!(parse_size("99999999999t").is_err());
    }

    #[test]
    fn lines() {
        let s =
            Step::new("root", &["makefs", "-o", "label=rb root", "a b"]).in_dir(Path::new("/d x"));
        assert_eq!(s.line(), "(cd '/d x' && makefs -o 'label=rb root' 'a b')");
    }

    #[test]
    fn runs_and_stops() {
        let dir = std::env::temp_dir().join(format!("rb-image-plan-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let steps = [
            Step::new("one", &["sh", "-c", "echo one > out"]).in_dir(&dir),
            Step::new("two", &["sh", "-c", "echo bad; exit 3"]),
            Step::new("three", &["sh", "-c", "echo never"]),
        ];
        let err = run(&steps, &dir).unwrap_err();
        assert!(
            err.contains("exit status 3") && err.contains("bad"),
            "{err}"
        );
        assert!(dir.join("one.log").exists() && dir.join("out").exists());
        assert!(!dir.join("three.log").exists());
        let file = dir.join("grow");
        std::fs::write(&file, "x").unwrap();
        assert!(grow(&file, 1 << 20).unwrap());
        assert!(!grow(&file, 10).unwrap());
        assert_eq!(std::fs::metadata(&file).unwrap().len(), 1 << 20);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
