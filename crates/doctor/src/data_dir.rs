//! Where the doctor keeps PoB installs.

use std::env;
use std::fs;
use std::path::PathBuf;

use crate::AnyResult;

#[derive(Debug)]
pub struct DataDir {
    root: PathBuf,
}

impl DataDir {
    pub fn new(explicit: Option<PathBuf>) -> AnyResult<Self> {
        let root = match explicit {
            Some(dir) => dir,
            None => default_root().ok_or("cannot find a data directory; pass --data-dir")?,
        };
        Ok(Self { root })
    }

    /// One subdirectory per PoB version, named by version number.
    pub fn pob_dir(&self) -> PathBuf {
        self.root.join("pob")
    }

    pub fn install(&self, version: &str) -> AnyResult<pob::Install> {
        let version = version.trim_start_matches('v');
        let dir = self.pob_dir().join(version);
        if !dir.exists() {
            return Err(format!(
                "PoB2 {version} is not installed. Run: doctor engine install --version {version}"
            )
            .into());
        }
        Ok(pob::Install::open(dir)?)
    }

    /// Every installed version, oldest first.
    pub fn installs(&self) -> AnyResult<Vec<pob::Install>> {
        let dir = self.pob_dir();
        if !dir.exists() {
            return Ok(Vec::new());
        }
        let mut installs = Vec::new();
        for entry in fs::read_dir(&dir)? {
            let path = entry?.path();
            let hidden = path
                .file_name()
                .and_then(|n| n.to_str())
                .is_none_or(|n| n.starts_with('.'));
            if !hidden && path.is_dir() {
                installs.push(pob::Install::open(path)?);
            }
        }
        installs.sort_by_key(|i| version_key(i.version().as_str()));
        Ok(installs)
    }
}

/// Numeric sort key for versions like `0.23.1`.
pub fn version_key(version: &str) -> Vec<u32> {
    version.split('.').map(|p| p.parse().unwrap_or(0)).collect()
}

fn default_root() -> Option<PathBuf> {
    if cfg!(windows) {
        env::var_os("LOCALAPPDATA").map(|d| PathBuf::from(d).join("build-doctor"))
    } else if let Some(xdg) = env::var_os("XDG_DATA_HOME") {
        Some(PathBuf::from(xdg).join("build-doctor"))
    } else {
        env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share/build-doctor"))
    }
}
