//! The tracked repos and PRs, persisted in `~/.ghui/config.yaml`.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Everything ghui tracks. The order of `repos` is the tab order.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub repos: Vec<RepoConfig>,
}

/// One tracked repository and the PR numbers tracked in it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepoConfig {
    /// `owner/repo`.
    pub name: String,
    #[serde(default)]
    pub prs: Vec<u64>,
}

#[derive(Debug)]
pub enum ConfigError {
    NoHome,
    Io(PathBuf, io::Error),
    Parse(PathBuf, serde_yaml_ng::Error),
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConfigError::NoHome => write!(f, "cannot locate the config: $HOME is not set"),
            ConfigError::Io(path, e) => write!(f, "{}: {e}", path.display()),
            ConfigError::Parse(path, e) => write!(f, "{}: invalid config: {e}", path.display()),
        }
    }
}

impl std::error::Error for ConfigError {}

/// `$HOME/.ghui/config.yaml`.
pub fn default_path() -> Result<PathBuf, ConfigError> {
    std::env::var_os("HOME")
        .filter(|h| !h.is_empty())
        .map(|h| PathBuf::from(h).join(".ghui").join("config.yaml"))
        .ok_or(ConfigError::NoHome)
}

impl Config {
    /// Loads the config, creating an empty one if the file does not exist.
    pub fn load_or_create(path: &Path) -> Result<Config, ConfigError> {
        match fs::read_to_string(path) {
            Ok(text) => Config::parse(&text).map_err(|e| ConfigError::Parse(path.into(), e)),
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                let config = Config::default();
                config.save(path)?;
                Ok(config)
            }
            Err(e) => Err(ConfigError::Io(path.into(), e)),
        }
    }

    pub fn parse(text: &str) -> Result<Config, serde_yaml_ng::Error> {
        if text.trim().is_empty() {
            return Ok(Config::default());
        }
        serde_yaml_ng::from_str(text)
    }

    pub fn to_yaml(&self) -> String {
        serde_yaml_ng::to_string(self).expect("config is always serializable")
    }

    /// Writes the config atomically (temp file + rename), creating the
    /// parent directory if needed.
    pub fn save(&self, path: &Path) -> Result<(), ConfigError> {
        let io_err = |e| ConfigError::Io(path.into(), e);
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir).map_err(io_err)?;
        }
        let tmp = path.with_extension("yaml.tmp");
        fs::write(&tmp, self.to_yaml()).map_err(io_err)?;
        fs::rename(&tmp, path).map_err(io_err)
    }

    /// Adds `pr` to `repo`. Returns `false` if it was already tracked or the
    /// repo is unknown.
    pub fn track(&mut self, repo: &str, pr: u64) -> bool {
        match self.repos.iter_mut().find(|r| r.name == repo) {
            Some(r) if !r.prs.contains(&pr) => {
                r.prs.push(pr);
                true
            }
            _ => false,
        }
    }

    /// Removes `pr` from `repo`. Returns `false` if it was not tracked.
    pub fn untrack(&mut self, repo: &str, pr: u64) -> bool {
        match self.repos.iter_mut().find(|r| r.name == repo) {
            Some(r) => {
                let before = r.prs.len();
                r.prs.retain(|n| *n != pr);
                r.prs.len() != before
            }
            None => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ghui-test-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir.join(".ghui").join("config.yaml")
    }

    #[test]
    fn parses_the_documented_shape() {
        let config = Config::parse(
            "repos:\n  - name: owner/repo\n    prs: [123, 456]\n  - name: owner/other\n",
        )
        .unwrap();
        assert_eq!(config.repos.len(), 2);
        assert_eq!(config.repos[0].prs, vec![123, 456]);
        assert!(config.repos[1].prs.is_empty());
    }

    #[test]
    fn empty_file_is_an_empty_config() {
        assert_eq!(Config::parse("").unwrap(), Config::default());
    }

    #[test]
    fn malformed_yaml_is_an_error() {
        assert!(Config::parse("repos: [name: 1").is_err());
    }

    #[test]
    fn creates_missing_file_and_round_trips() {
        let path = temp_path("roundtrip");
        let mut config = Config::load_or_create(&path).unwrap();
        assert!(path.exists());
        assert!(config.repos.is_empty());

        config.repos.push(RepoConfig {
            name: "a/b".into(),
            prs: vec![],
        });
        assert!(config.track("a/b", 7));
        assert!(!config.track("a/b", 7));
        assert!(!config.track("x/y", 7));
        config.save(&path).unwrap();

        let loaded = Config::load_or_create(&path).unwrap();
        assert_eq!(loaded, config);
        let _ = fs::remove_dir_all(path.parent().unwrap().parent().unwrap());
    }

    #[test]
    fn untrack_removes_only_existing_prs() {
        let mut config = Config::parse("repos:\n  - name: a/b\n    prs: [1, 2]\n").unwrap();
        assert!(config.untrack("a/b", 1));
        assert!(!config.untrack("a/b", 1));
        assert_eq!(config.repos[0].prs, vec![2]);
    }
}
