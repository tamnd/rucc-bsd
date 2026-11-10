//! `mk-divergences.toml`: the differences between R's and G's configuration that are correct, each
//! with its reason (plan 05.5 and 02.2).
//!
//! `rb mk-diff` reads the entries that name a make variable, `rb flags-diff` reads the entries
//! that name a flag, and `rb probes` reads the entries that name a macro. A difference that an entry matches is listed with the entry's reason and is
//! not counted. A difference caused by rucc answering a probe wrongly is a bug, and never gets an
//! entry.

use serde::Deserialize;
use std::path::Path;

/// The file's name at the top of the repository.
pub const FILE_NAME: &str = "mk-divergences.toml";

/// The whole file.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Divergences {
    /// The entries.
    #[serde(default)]
    pub divergence: Vec<Divergence>,
}

/// One correct difference.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Divergence {
    /// `freebsd` or `netbsd`, or both when absent.
    pub system: Option<String>,
    /// The make variable, for `rb mk-diff`. A `*` matches any text.
    pub variable: Option<String>,
    /// Where `rb mk-diff` reads the variable, such as `top` or `kernel`, or every place when
    /// absent.
    pub point: Option<String>,
    /// A flag of a unit's command line, for `rb flags-diff`. A `*` matches any text.
    pub flag: Option<String>,
    /// A macro of `-dM -E`, for `rb probes`. A `*` matches any text.
    #[serde(rename = "macro")]
    pub predefined: Option<String>,
    /// Why the difference is correct.
    pub reason: String,
}

impl Divergences {
    /// Parse the file's text. Every entry must name one variable, flag or macro, and give a
    /// reason.
    pub fn parse(text: &str) -> Result<Self, String> {
        let file: Self = toml::from_str(text).map_err(|e| e.to_string())?;
        for (i, d) in file.divergence.iter().enumerate() {
            let names = [&d.variable, &d.flag, &d.predefined];
            if names.iter().filter(|n| n.is_some()).count() != 1 {
                return Err(format!(
                    "entry {} must name one of a variable, a flag or a macro",
                    i + 1
                ));
            }
            if d.reason.trim().is_empty() {
                return Err(format!("entry {} has no reason", i + 1));
            }
        }
        Ok(file)
    }

    /// Read a file, or the one at the top of the repository when `file` is `None`. A missing
    /// file at the top is the same as an empty one.
    pub fn load(root: &Path, file: Option<&Path>) -> Result<Self, String> {
        let path = file.map_or_else(|| root.join(FILE_NAME), Path::to_path_buf);
        match std::fs::read_to_string(&path) {
            Ok(text) => Self::parse(&text).map_err(|e| format!("{}: {e}", path.display())),
            Err(_) if file.is_none() => Ok(Self::default()),
            Err(e) => Err(format!("{}: {e}", path.display())),
        }
    }

    /// The reason for a difference in a make variable, if an entry allows it.
    #[must_use]
    pub fn variable(&self, system: &str, point: &str, name: &str) -> Option<&str> {
        self.divergence
            .iter()
            .find(|d| {
                d.system.as_deref().is_none_or(|s| s == system)
                    && d.point.as_deref().is_none_or(|p| p == point)
                    && d.variable.as_deref().is_some_and(|v| glob(v, name))
            })
            .map(|d| d.reason.as_str())
    }

    /// The reason for a flag that one side has and the other does not, if an entry allows it.
    #[must_use]
    pub fn flag(&self, system: &str, flag: &str) -> Option<&str> {
        self.divergence
            .iter()
            .find(|d| {
                d.system.as_deref().is_none_or(|s| s == system)
                    && d.flag.as_deref().is_some_and(|f| glob(f, flag))
            })
            .map(|d| d.reason.as_str())
    }

    /// The reason for a macro of `-dM -E` that differs, if an entry allows it.
    #[must_use]
    pub fn predefined(&self, system: &str, name: &str) -> Option<&str> {
        self.divergence
            .iter()
            .find(|d| {
                d.system.as_deref().is_none_or(|s| s == system)
                    && d.predefined.as_deref().is_some_and(|m| glob(m, name))
            })
            .map(|d| d.reason.as_str())
    }
}

/// Whether `text` matches `pattern`, where `*` matches any text, also none.
#[must_use]
pub fn glob(pattern: &str, text: &str) -> bool {
    let mut parts = pattern.split('*');
    let first = parts.next().unwrap_or_default();
    let Some(mut rest) = text.strip_prefix(first) else {
        return false;
    };
    let parts: Vec<&str> = parts.collect();
    let Some((last, middle)) = parts.split_last() else {
        return rest.is_empty();
    };
    for part in middle {
        match rest.find(part) {
            Some(at) => rest = &rest[at + part.len()..],
            None => return false,
        }
    }
    rest.len() >= last.len() && rest.ends_with(last)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stars_match_any_text() {
        assert!(glob("X_COMPILER_*", "X_COMPILER_TYPE"));
        assert!(glob("*_RESOURCE_DIR", "X_COMPILER_RESOURCE_DIR"));
        assert!(glob("-f*-version=*", "-fgnuc-version=15.2.0"));
        assert!(glob("CC", "CC"));
        assert!(!glob("CC", "XCC"));
        assert!(!glob("a*b*c", "acb"));
        assert!(glob("a*a", "aa"));
        assert!(!glob("ab*ba", "aba"));
    }

    #[test]
    fn entries_match_by_system_point_and_name() {
        let d = Divergences::parse(
            r#"
[[divergence]]
variable = "*COMPILER_RESOURCE_DIR"
reason = "each compiler's own directory"

[[divergence]]
system = "netbsd"
point = "kernel"
variable = "CFLAGS"
reason = "for the test"

[[divergence]]
flag = "-frucc-*"
reason = "rucc's own"

[[divergence]]
system = "freebsd"
macro = "__rucc*"
reason = "rucc's name"
"#,
        )
        .unwrap();
        assert!(
            d.variable("freebsd", "top", "X_COMPILER_RESOURCE_DIR")
                .is_some()
        );
        assert!(d.variable("netbsd", "kernel", "CFLAGS").is_some());
        assert!(d.variable("netbsd", "lib", "CFLAGS").is_none());
        assert!(d.variable("freebsd", "kernel", "CFLAGS").is_none());
        assert_eq!(d.flag("netbsd", "-frucc-trace=x"), Some("rucc's own"));
        assert!(d.flag("netbsd", "-O2").is_none());
        assert_eq!(
            d.predefined("freebsd", "__rucc_major__"),
            Some("rucc's name")
        );
        assert!(d.predefined("netbsd", "__rucc__").is_none());
        assert!(d.predefined("freebsd", "__GNUC__").is_none());
        assert!(d.variable("freebsd", "top", "__rucc__").is_none());
    }

    #[test]
    fn an_entry_needs_one_name_and_a_reason() {
        let both =
            Divergences::parse("[[divergence]]\nvariable = \"A\"\nflag = \"-b\"\nreason = \"x\"\n");
        assert!(both.unwrap_err().contains("one of"));
        let two =
            Divergences::parse("[[divergence]]\nmacro = \"A\"\nflag = \"-b\"\nreason = \"x\"\n");
        assert!(two.is_err());
        let neither = Divergences::parse("[[divergence]]\nreason = \"x\"\n");
        assert!(neither.is_err());
        let bare = Divergences::parse("[[divergence]]\nvariable = \"A\"\nreason = \" \"\n");
        assert!(bare.unwrap_err().contains("no reason"));
    }

    #[test]
    fn the_file_in_the_repository_reads() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let d = Divergences::load(&root, None).unwrap();
        assert!(!d.divergence.is_empty());
    }
}
