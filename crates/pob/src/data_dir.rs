//! Where the doctor keeps PoB installs: a `data` folder next to the program
//! when there is one (the portable "Build Doctor" folder from the release
//! zip), otherwise `%LOCALAPPDATA%\build-doctor` on Windows,
//! `$XDG_DATA_HOME/build-doctor` or `~/.local/share/build-doctor` elsewhere.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use crate::{Error, Install, Result};

#[derive(Debug)]
pub struct DataDir {
    root: PathBuf,
}

impl DataDir {
    /// `explicit`, else the portable folder, else the platform's data
    /// directory.
    pub fn new(explicit: Option<PathBuf>) -> Result<Self> {
        let root = match explicit.or_else(portable_root) {
            Some(dir) => dir,
            None => platform_root()
                .ok_or_else(|| Error::NotAnInstall("cannot find a data directory".into()))?,
        };
        Ok(Self { root })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Whether this is the `data` folder next to the program.
    pub fn is_portable(&self) -> bool {
        portable_root().is_some_and(|p| p == self.root)
    }

    /// Moves PoB installs from the platform data directory into this one,
    /// once: when this one has none and the platform one has some. A move
    /// that cannot be done as a rename (another drive) is skipped; the app
    /// then offers to install PoB here. Returns whether anything moved.
    pub fn adopt_platform_installs(&self) -> bool {
        platform_root().is_some_and(|old| self.adopt_installs(&old))
    }

    fn adopt_installs(&self, old: &Path) -> bool {
        let (from, to) = (old.join("pob"), self.pob_dir());
        if old == self.root || to.exists() || !from.is_dir() {
            return false;
        }
        if fs::rename(&from, &to).is_err() {
            return false;
        }
        // The old folder held only the installs; it stays if anything else is there.
        let _ = fs::remove_dir(old);
        true
    }

    /// One subdirectory per PoB version, named by version number.
    pub fn pob_dir(&self) -> PathBuf {
        self.root.join("pob")
    }

    pub fn install(&self, version: &str) -> Result<Install> {
        let version = version.trim_start_matches('v');
        let dir = self.pob_dir().join(version);
        if !dir.exists() {
            return Err(Error::NotInstalled(version.to_owned()));
        }
        let install = Install::open(&dir)?;
        if install.version().as_str() != version {
            return Err(Error::NotAnInstall(format!(
                "{} contains PoB2 {}, not {version}",
                dir.display(),
                install.version()
            )));
        }
        Ok(install)
    }

    /// Every installed version, oldest first.
    pub fn installs(&self) -> Result<Vec<Install>> {
        let dir = self.pob_dir();
        if !dir.exists() {
            return Ok(Vec::new());
        }
        let mut installs = Vec::new();
        for entry in fs::read_dir(&dir).map_err(|e| Error::io("list", &dir, e))? {
            let path = entry.map_err(|e| Error::io("list", &dir, e))?.path();
            let hidden = path
                .file_name()
                .and_then(|n| n.to_str())
                .is_none_or(|n| n.starts_with('.'));
            if !hidden && path.is_dir() {
                installs.push(Install::open(path)?);
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

/// The `data` folder next to the running program, if there is one.
fn portable_root() -> Option<PathBuf> {
    let exe = env::current_exe().ok()?;
    let dir = exe.parent()?.join("data");
    dir.is_dir().then_some(dir)
}

fn platform_root() -> Option<PathBuf> {
    if cfg!(windows) {
        env::var_os("LOCALAPPDATA").map(|d| PathBuf::from(d).join("build-doctor"))
    } else if let Some(xdg) = env::var_os("XDG_DATA_HOME") {
        Some(PathBuf::from(xdg).join("build-doctor"))
    } else {
        env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share/build-doctor"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adopts_installs_once_and_never_overwrites() {
        let base = env::temp_dir().join(format!("bd-adopt-{}", std::process::id()));
        let old = base.join("old");
        let new = DataDir::new(Some(base.join("data"))).unwrap();
        fs::create_dir_all(old.join("pob/0.23.1")).unwrap();
        fs::write(old.join("pob/0.23.1/manifest.xml"), "x").unwrap();
        fs::create_dir_all(new.root()).unwrap();

        assert!(new.adopt_installs(&old));
        assert!(new.pob_dir().join("0.23.1/manifest.xml").is_file());
        assert!(!old.exists(), "the emptied old folder is removed");

        // Already has installs: nothing moves.
        fs::create_dir_all(old.join("pob/0.22.0")).unwrap();
        assert!(!new.adopt_installs(&old));
        assert!(old.join("pob/0.22.0").is_dir());
        assert!(!new.adopt_installs(new.root()));
        fs::remove_dir_all(&base).unwrap();
    }
}
