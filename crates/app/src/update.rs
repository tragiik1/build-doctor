//! Version information and self-update from the GitHub release "latest".
//!
//! CI stamps each Windows build with `BUILD_DOCTOR_VERSION` (`0.1.<build>`)
//! and `BUILD_DOCTOR_COMMIT`, and publishes the newest `main` build as the
//! repository's latest release, next to a `version.json` naming its version
//! and the SHA-256 of the executable. Updating downloads that executable,
//! checks the hash, swaps it in beside the running one and restarts.

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const RELEASE: &str = "https://github.com/tragiik1/build-doctor/releases/latest/download/";
const EXE_ASSET: &str = "build-doctor.exe";

/// This build's version: `0.1.<build>` for CI builds, `<crate version>-dev`
/// for local ones.
pub fn version() -> String {
    option_env!("BUILD_DOCTOR_VERSION").map_or_else(
        || format!("{}-dev", env!("CARGO_PKG_VERSION")),
        str::to_owned,
    )
}

/// The commit this build was made from, when CI built it.
pub fn commit() -> Option<&'static str> {
    option_env!("BUILD_DOCTOR_COMMIT")
}

/// The build number in a version like `0.1.17`.
fn build_number(version: &str) -> Option<u64> {
    version.rsplit('.').next()?.parse().ok()
}

/// What `version.json` in the latest release says.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Release {
    pub version: String,
    pub commit: String,
    pub sha256: String,
}

fn agent(timeout: Duration) -> ureq::Agent {
    ureq::Agent::config_builder()
        .user_agent(concat!("build-doctor/", env!("CARGO_PKG_VERSION")))
        .timeout_connect(Some(Duration::from_secs(15)))
        .timeout_global(Some(timeout))
        .build()
        .into()
}

/// The latest release, if it is newer than this build. Local builds (no
/// build number) never offer updates.
pub fn check() -> Result<Option<Release>, String> {
    let Some(current) = build_number(&version()) else {
        return Ok(None);
    };
    let body = agent(Duration::from_secs(20))
        .get(&format!("{RELEASE}version.json"))
        .call()
        .map_err(|e| format!("Could not check for updates: {e}"))?
        .into_body()
        .read_to_vec()
        .map_err(|e| format!("Could not check for updates: {e}"))?;
    let release: Release = serde_json::from_slice(&body)
        .map_err(|e| format!("Could not read the update information: {e}"))?;
    Ok(build_number(&release.version)
        .filter(|latest| *latest > current)
        .map(|_| release))
}

/// Downloads the release's executable, checks its SHA-256, and puts it in
/// place of the running one (which is kept as `.old` until the next start).
/// `progress` gets (bytes so far, total bytes if known).
pub fn install(release: &Release, progress: &dyn Fn(u64, Option<u64>)) -> Result<(), String> {
    let exe =
        std::env::current_exe().map_err(|e| format!("Cannot find the running program: {e}"))?;
    let response = agent(Duration::from_secs(15 * 60))
        .get(&format!("{RELEASE}{EXE_ASSET}"))
        .call()
        .map_err(|e| format!("Download failed: {e}"))?;
    let total = response.body().content_length();
    let mut reader = response.into_body().into_reader();
    let mut bytes = Vec::with_capacity(total.unwrap_or(8 << 20) as usize);
    let mut chunk = [0u8; 64 * 1024];
    loop {
        let n = reader
            .read(&mut chunk)
            .map_err(|e| format!("Download failed: {e}"))?;
        if n == 0 {
            break;
        }
        bytes.extend_from_slice(&chunk[..n]);
        progress(bytes.len() as u64, total);
    }

    let actual = hex(&Sha256::digest(&bytes));
    if !actual.eq_ignore_ascii_case(&release.sha256) {
        return Err(format!(
            "The download does not match the release (SHA-256 {actual}, expected {}). Nothing was changed.",
            release.sha256
        ));
    }

    let new = with_suffix(&exe, "new");
    let old = with_suffix(&exe, "old");
    fs::write(&new, &bytes).map_err(|e| format!("Cannot write {}: {e}", new.display()))?;
    let _ = fs::remove_file(&old);
    // Windows lets a running program be renamed, not replaced.
    fs::rename(&exe, &old).map_err(|e| format!("Cannot move the running program aside: {e}"))?;
    if let Err(e) = fs::rename(&new, &exe) {
        let _ = fs::rename(&old, &exe);
        return Err(format!("Cannot put the new version in place: {e}"));
    }
    Ok(())
}

/// Removes the previous executable an update left behind.
pub fn clean_up() {
    if let Ok(exe) = std::env::current_exe() {
        let _ = fs::remove_file(with_suffix(&exe, "old"));
        let _ = fs::remove_file(with_suffix(&exe, "new"));
    }
}

fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_owned();
    name.push(format!(".{suffix}"));
    path.with_file_name(name)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_build_numbers() {
        assert_eq!(build_number("0.1.17"), Some(17));
        assert_eq!(build_number("0.1.0-dev"), None);
    }

    #[test]
    fn names_side_files() {
        assert_eq!(
            with_suffix(Path::new("C:/x/build-doctor.exe"), "old"),
            Path::new("C:/x/build-doctor.exe.old")
        );
    }

    #[test]
    fn hashes_like_sha256sum() {
        assert_eq!(
            hex(&Sha256::digest(b"abc")),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
