//! Where the repository and the cache are.

use std::path::{Path, PathBuf};

/// The rucc-bsd checkout `rb` works in, and the cache for downloads and trees.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Repo {
    /// The directory with `sets.toml`.
    pub root: PathBuf,
    /// `RB_CACHE`, or `~/.cache/rb`.
    pub cache: PathBuf,
}

impl Repo {
    /// Find the repository from `RB_ROOT`, or by walking up from the current directory to
    /// `sets.toml`.
    pub fn find() -> Result<Self, String> {
        let cwd = std::env::current_dir().map_err(|e| format!("no current directory: {e}"))?;
        Self::find_from(
            &cwd,
            std::env::var_os("RB_ROOT").map(PathBuf::from),
            std::env::var_os("RB_CACHE").map(PathBuf::from),
            std::env::var_os("HOME").map(PathBuf::from),
        )
    }

    /// [`Repo::find`] with the environment given.
    pub fn find_from(
        cwd: &Path,
        root: Option<PathBuf>,
        cache: Option<PathBuf>,
        home: Option<PathBuf>,
    ) -> Result<Self, String> {
        let root = match root {
            Some(root) if root.join(rb_pins::sets::FILE_NAME).is_file() => root,
            Some(root) => {
                return Err(format!(
                    "RB_ROOT is {}, which has no {}",
                    root.display(),
                    rb_pins::sets::FILE_NAME
                ));
            }
            None => cwd
                .ancestors()
                .find(|d| d.join(rb_pins::sets::FILE_NAME).is_file())
                .map(Path::to_path_buf)
                .ok_or_else(|| {
                    format!(
                        "no {} in {} or above; run rb in a rucc-bsd checkout or set RB_ROOT",
                        rb_pins::sets::FILE_NAME,
                        cwd.display()
                    )
                })?,
        };
        let cache = match (cache, home) {
            (Some(cache), _) => cache,
            (None, Some(home)) => home.join(".cache").join("rb"),
            (None, None) => return Err("neither RB_CACHE nor HOME is set".into()),
        };
        Ok(Self { root, cache })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_root_is_found_above_and_the_cache_defaults_to_home() {
        let top = std::env::temp_dir().join(format!("rb-repo-{}", std::process::id()));
        let deep = top.join("crates").join("rb");
        std::fs::create_dir_all(&deep).unwrap();
        std::fs::write(top.join("sets.toml"), "").unwrap();
        let repo = Repo::find_from(&deep, None, None, Some("/home/x".into())).unwrap();
        assert_eq!(repo.root, top);
        assert_eq!(repo.cache, PathBuf::from("/home/x/.cache/rb"));
        let repo =
            Repo::find_from(Path::new("/"), Some(top.clone()), Some("/c".into()), None).unwrap();
        assert_eq!(repo.cache, PathBuf::from("/c"));
        let wrong = Repo::find_from(&deep, Some(deep.clone()), None, Some("/h".into()));
        assert!(wrong.unwrap_err().contains("RB_ROOT"));
        std::fs::remove_dir_all(&top).unwrap();
    }
}
